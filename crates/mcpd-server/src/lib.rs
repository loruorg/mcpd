//! The mcpd HTTP server.
//!
//! One process serves five things:
//!
//! * **OTLP ingest** at `/v1/traces`, `/v1/logs`, `/v1/metrics`, so any
//!   OpenTelemetry SDK works with no mcpd-specific code.
//! * **Native ingest** at `/v1/ingest`, which carries the full model.
//! * **The query API** under `/v1/api`, which the UI and the SDKs both use.
//! * **The run surface**, also under `/v1/api`: definitions, runs, events and
//!   the worker protocol, in `runs`.
//! * **The operator UI**, embedded in the binary, at `/`.
//!
//! Serving all five from one port is what makes the deployment story a single
//! container with no sidecar and no reverse proxy to configure.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod accounts;
pub mod aspects;
pub mod auth;
pub mod derive;
pub mod error;
pub mod eval;
pub mod forward;
pub mod identity;
pub mod ingest;
pub mod judge;
pub mod layers;
pub mod mail;
pub mod monitor;
pub mod orgs;
pub mod pipeline;
pub mod query;
pub mod runs;
pub mod scim;
pub mod sso;
pub mod state;
pub mod totp;
pub mod ui;

use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::routing::{delete, get, post};
use axum::Router;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub use state::{AppState, ServerConfig};

/// Build the full application router.
///
/// The routes are grouped by what they need: ingest takes no CORS, the query
/// API takes the configured origin policy, and the operational probes stay
/// readable from anywhere. Everything that applies to every request is layered
/// once, here, around the merged router. `layers` holds those layers, and the
/// order below is the order a request meets them from the outside in, read
/// bottom to top.
pub fn router(state: Arc<AppState>) -> Router {
    let routes = Router::new()
        .merge(ingest_router())
        .merge(api_router(&state.config))
        .merge(ops_router())
        .merge(scim::router())
        .merge(ui::router());
    layered(routes, state)
}

/// Wrap routes in the layers every request meets.
///
/// Apart from [`router`] so the stack can be put around a route that exists
/// only in a test: a handler that panics on purpose is the one way to prove a
/// panic leaves as a 500 in the JSON shape, and no shipped route panics on
/// purpose.
pub fn layered(routes: Router<Arc<AppState>>, state: Arc<AppState>) -> Router {
    let max_body = state.config.max_body_bytes;
    let deadline = Duration::from_secs(state.config.request_timeout_seconds.max(1));

    routes
        .layer(CompressionLayer::new())
        // One body limit, and it is `max_body_bytes`. axum applies a limit of
        // its own inside every body extractor, 2 MiB by default, and nothing
        // turned it off: the configured 32 MiB was documented, the 2 MiB was
        // enforced, and a 3 MiB batch was refused with an empty 413 from a
        // limit no flag names. Disabling axum's leaves the layer below as the
        // only one, and it refuses a declared length before reading a byte.
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(max_body))
        .layer(from_fn_with_state(Arc::clone(&state), layers::refuse_oversize))
        // The deadline sits outside the body limit so it covers reading the
        // body, and inside the trace layer so a timed-out request is still
        // logged as one. The layer answers with a bare 408, which nothing else
        // in this daemon produces; that is what the layer above it recognises
        // and rewrites into the JSON 503 a client can act on.
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, deadline))
        .layer(from_fn_with_state(Arc::clone(&state), layers::answer_for_timeout))
        // A panic anywhere inside this point becomes a 500 in the JSON shape.
        // Without it the panic unwound into the connection task and the
        // client saw a dropped connection with nothing to read. Inside the
        // trace layer and the error counter, so the 500 is logged as a
        // request and counted as a server error like any other.
        .layer(CatchPanicLayer::custom(layers::answer_for_panic))
        .layer(TraceLayer::new_for_http())
        .layer(from_fn_with_state(Arc::clone(&state), layers::count_server_errors))
        .with_state(state)
}

/// The four ingest doors. No CORS: an SDK is not a browser, and a browser page
/// posting telemetry across origins is not a case this daemon serves.
fn ingest_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/traces", post(ingest::otlp_traces))
        .route("/v1/logs", post(ingest::otlp_logs))
        .route("/v1/metrics", post(ingest::otlp_metrics))
        .route("/v1/ingest", post(ingest::native))
}

/// The query API under `/v1/api`, with its origin policy.
///
/// This is the router to extend when an endpoint is added: a route here gets
/// the same origin policy and the same layers as every other read or write.
/// The policy comes from `cors_origins`, and with none configured no layer is
/// added at all, so a page on another origin gets no allow header and the
/// browser refuses it the data. The bundled interface is served from this
/// daemon's own origin and never needs one.
fn api_router(config: &ServerConfig) -> Router<Arc<AppState>> {
    let routes = Router::new()
        .route("/v1/api/traces", get(query::list_traces))
        .route("/v1/api/traces/{trace_id}", get(query::get_trace))
        .route("/v1/api/traces/{trace_id}", delete(query::delete_trace))
        .route("/v1/api/spans", get(query::list_spans))
        .route("/v1/api/logs", get(query::list_logs))
        .route("/v1/api/metrics", get(query::query_metrics))
        .route("/v1/api/scores", get(query::list_scores))
        .route("/v1/api/scores", post(query::create_score))
        .route("/v1/api/feedback", get(query::list_feedback))
        .route("/v1/api/feedback", post(query::create_feedback))
        .route("/v1/api/analytics/summary", get(query::analytics_summary))
        .route("/v1/api/analytics/cost", get(query::analytics_cost))
        .route("/v1/api/projects", get(query::list_projects))
        // What this deployment's storage backend can do. Under the query API
        // rather than beside /healthz because it takes the read scope: it says
        // which backend is behind the daemon, which is not a fact for anyone
        // who can reach the port.
        .route("/v1/api/capabilities", get(query::get_capabilities))
        // The two doors that depend on what the backend can do. Both refuse
        // with 501 naming the capability from the route above, so a caller
        // reads that once instead of learning the difference between two
        // backends from an empty list.
        .route("/v1/api/discovery", get(query::discover))
        .route("/v1/api/deltas", get(query::poll_deltas))
        .route("/v1/api/scorers", get(query::list_scorers))
        .route("/v1/api/score", post(query::score_runs))
        .route("/v1/api/experiments/compare", get(query::compare_experiments))
        .route("/v1/api/datasets", get(query::list_datasets))
        .route("/v1/api/datasets", post(query::create_dataset))
        .route("/v1/api/datasets/{id}/items", get(query::list_dataset_items))
        .route("/v1/api/datasets/{id}/items", post(query::add_dataset_items))
        .route("/v1/api/datasets/{id}/capture", post(query::capture_into_dataset))
        .route("/v1/api/experiments", get(query::list_experiments))
        .route("/v1/api/experiments", post(query::create_experiment))
        .route("/v1/api/monitors", get(query::list_monitors))
        .route("/v1/api/monitors/{id}/ack", post(query::ack_monitor))
        .route("/v1/api/slos", get(query::list_slos))
        .route("/v1/api/slos/{id}/ack", post(query::ack_slo))
        // The nine Intelligence routes. Every one of them is a lookup of what
        // the analysis pass wrote: none recomputes a clustering, so refreshing
        // a screen re-reads one snapshot rather than building another.
        //
        // Eight of the nine refuse with 501 on a backend that does not hold the
        // note and theme tables, naming `theme_analysis`, the same word
        // /v1/api/capabilities publishes. The catalogue is the exception: what
        // the four aspects are is a fact about the daemon and not about its
        // storage, so it answers everywhere.
        .route("/v1/api/aspects", get(query::list_aspects))
        .route("/v1/api/aspects/entities", get(query::list_aspect_entities))
        .route("/v1/api/aspects/progress", get(query::get_aspect_progress))
        .route("/v1/api/themes/snapshots", get(query::list_theme_snapshots))
        .route("/v1/api/themes/flow", get(query::get_theme_flow))
        .route("/v1/api/themes/{theme_id}", get(query::get_theme))
        .route("/v1/api/themes/{theme_id}/examples", get(query::list_theme_examples))
        .route("/v1/api/themes/{theme_id}/history", get(query::list_theme_history))
        // Not entity scoped: the only one of the nine addressed by a trace. It
        // takes an optional project, because a trace id two projects share is
        // a run in each.
        .route("/v1/api/traces/{trace_id}/aspects", get(query::list_trace_aspect_notes))
        // The run surface. Definitions and runs take the write, read and execute
        // scopes; the worker protocol takes the worker scope, which is the key
        // that lives in the process running step bodies.
        .route("/v1/api/definitions", post(runs::put_definition))
        .route("/v1/api/definitions", get(runs::list_definitions))
        .route("/v1/api/definitions/{id}", get(runs::get_definition))
        .route("/v1/api/runs", post(runs::start_run))
        .route("/v1/api/runs", get(runs::list_runs))
        .route("/v1/api/runs/{run_id}", get(runs::get_run))
        .route("/v1/api/runs/{run_id}/cancel", post(runs::cancel_run))
        .route("/v1/api/runs/{run_id}/events", get(runs::list_run_events))
        .route("/v1/api/runs/{run_id}/cause", get(runs::get_step_cause))
        .route("/v1/api/runs/{run_id}/signal", post(runs::signal_run))
        .route("/v1/api/events", post(runs::publish_event))
        .route("/v1/api/workers/register", post(runs::register_worker))
        .route("/v1/api/steps/claim", post(runs::claim_steps))
        .route("/v1/api/steps/{lease_id}/started", post(runs::step_started))
        .route("/v1/api/steps/{lease_id}/heartbeat", post(runs::step_heartbeat))
        .route("/v1/api/steps/{lease_id}/complete", post(runs::complete_step))
        .route("/v1/api/steps/{lease_id}/fail", post(runs::fail_step))
        // Admin scope. The audit log is read through the same router as the data
        // it describes, so a deployment that reaches one reaches the other and the
        // key scope is what separates them.
        .route("/v1/api/audit", get(query::list_audit))
        // Admin scope. Erasing by subject is the one destructive operation that
        // is not addressed by trace id, so it is the one an operator reaches for
        // when a person asks rather than when a run is wrong.
        .route("/v1/api/subjects/{user_id}", delete(query::erase_subject))
        // Accounts, for `--signup`. Every one answers 404 without it. Here
        // rather than in a router of their own so a browser on an allowed
        // origin can sign up and log in under the same CORS policy as the
        // reads it then makes. Sign-up and log-in take no key; the rest take
        // the session token and refuse an API key.
        .route("/v1/api/account/signup", post(accounts::signup))
        .route("/v1/api/account/login", post(accounts::login))
        .route("/v1/api/account/logout", post(accounts::logout))
        .route("/v1/api/account", get(accounts::get_account))
        .route("/v1/api/account/keys", get(accounts::list_account_keys))
        .route("/v1/api/account/keys", post(accounts::create_account_key))
        .route("/v1/api/account/keys/{key_id}", delete(accounts::revoke_account_key))
        // The rest of the account lifecycle, organisations and single sign-on,
        // each assembled in its own module.
        .merge(accounts::routes())
        .merge(orgs::routes())
        .merge(sso::routes());
    match layers::cors_for(&config.cors_origins) {
        Some(cors) => routes.layer(cors),
        None => routes,
    }
}

/// The operational probes and the scrape endpoint.
///
/// The operator interface is a static page that may be served from another
/// origin, and the first thing it does is probe /healthz. Without CORS here
/// the probe fails and the interface can never connect, however healthy the
/// daemon actually is. These three endpoints expose version, uptime and
/// counters, nothing that needs protecting by origin, so they stay open to
/// every origin whatever `cors_origins` says.
fn ops_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/healthz", get(query::healthz))
        .route("/readyz", get(query::readyz))
        .route("/metrics", get(query::self_metrics))
        .layer(CorsLayer::permissive())
}

/// When a background loop takes its first pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstPass {
    /// At once, as the loop starts. Retention does this, so a window that has
    /// been passed while the daemon was down is applied at boot.
    AtStart,
    /// One interval after the loop starts, so a pass never judges an empty
    /// window before the daemon has taken a single request.
    AfterInterval,
}

/// Run one pass of a background loop, and survive a panic inside it.
///
/// Returns whether the pass finished. A panic is logged with the loop's name
/// and the pass is abandoned; the caller carries on to its next tick. Without
/// this a panic ended the task that ran the loop, and nothing said so: the
/// monitors stopped evaluating, or retention stopped deleting, while every
/// health check stayed green. A loop that dies in silence is worse than a
/// crash, because a crash is at least seen.
pub async fn run_tick<F>(loop_name: &str, pass: F) -> bool
where
    F: Future<Output = ()>,
{
    use futures::FutureExt;
    // Unwind safety is asserted, not proven. What a pass touches is the store,
    // which owns its own locks and recovers a poisoned one, and derived state
    // that the next pass rebuilds from readings. A half-finished pass is
    // therefore a pass that did less, never one that left something wrong
    // for the next to read as true.
    match std::panic::AssertUnwindSafe(pass).catch_unwind().await {
        Ok(()) => true,
        Err(payload) => {
            tracing::error!(
                task = loop_name,
                panic = %layers::panic_message(&*payload),
                "a pass of the {loop_name} loop panicked; the pass was abandoned and the loop \
                 carries on at its next tick"
            );
            false
        }
    }
}

/// Run `pass` every `every` for the life of the process, surviving a panic in
/// any one pass.
///
/// Tracked by `state.tasks` and stopped by `state.shutdown`, so a stop between
/// passes is immediate and a stop during one waits for it, the same terms the
/// retention timer has always run on. Every loop the daemon owns goes through
/// here, so each of them gets the same answer to a panic rather than one each.
pub fn spawn_loop<P, F>(
    state: &Arc<AppState>,
    loop_name: &'static str,
    every: Duration,
    first: FirstPass,
    mut pass: P,
) where
    P: FnMut() -> F + Send + 'static,
    F: Future<Output = ()> + Send + 'static,
{
    let stop = state.shutdown.clone();
    state.tasks.spawn(async move {
        let mut timer = tokio::time::interval(every);
        if first == FirstPass::AfterInterval {
            // The first tick of an interval fires at once.
            timer.tick().await;
        }
        loop {
            tokio::select! {
                () = stop.cancelled() => return,
                _ = timer.tick() => {}
            }
            // Called inside the guarded future, so a panic while building the
            // pass is caught as well as one while running it.
            run_tick(loop_name, async { pass().await }).await;
        }
    });
}

/// A failed outbound request, described without its URL.
///
/// reqwest puts the whole URL in its message, path and query string included,
/// and a webhook, a judge endpoint or a forwarding destination often carries
/// its credential in one of those: a Slack incoming webhook's token is its
/// path. Every log line and every reason that quotes an outbound error goes
/// through here. The URL is dropped, and the causes the bare message hides
/// behind `error sending request` are added, so the line still says what went
/// wrong, such as a refused connection or a name that does not resolve.
#[must_use]
pub fn outbound_error(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut out = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(cause) = source {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        source = cause.source();
    }
    out
}
