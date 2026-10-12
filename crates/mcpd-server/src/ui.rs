//! The embedded operator UI.
//!
//! The built web application is compiled into the binary, so `mcpd` serves its
//! own interface on the same port as its API with no static-file server, no
//! volume mount and no second container. That is most of why the deployment
//! story fits on one line.
//!
//! When the binary is built without running the web build first, the routes
//! below still work and say exactly what to do about it — an empty page with
//! no explanation would be the worse failure.

use axum::body::Body;
use axum::extract::Path;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use include_dir::{include_dir, Dir};
use std::sync::Arc;

/// The built web application, embedded at compile time.
static UI: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui");

/// Whether a real UI was compiled in.
fn have_ui() -> bool {
    UI.get_file("index.html").is_some()
}

fn content_type_for(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, e)| e) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml",
        Some("webmanifest") => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

/// Hashed asset filenames are immutable; everything else must revalidate or a
/// user is stuck with a stale shell after an upgrade.
fn cache_control_for(path: &str) -> &'static str {
    if path.starts_with("_astro/") || path.contains(".hashed.") {
        "public, max-age=31536000, immutable"
    } else if path.ends_with(".html") {
        "no-cache"
    } else {
        "public, max-age=3600"
    }
}

fn serve(path: &str) -> Response {
    let cleaned = path.trim_start_matches('/');
    // Reject traversal outright rather than relying on the embedder to have
    // normalised it.
    if cleaned.contains("..") {
        return (StatusCode::BAD_REQUEST, "invalid path").into_response();
    }

    let candidate = if cleaned.is_empty() { "index.html" } else { cleaned };

    // Resolve the request to a file, and remember WHICH file: the content type
    // has to come from what is actually served, not from what was asked for.
    // `/app` has no extension, so typing it from the request path yielded
    // `application/octet-stream` and browsers downloaded the page instead of
    // rendering it.
    let resolved = UI
        .get_file(candidate)
        .map(|f| (f, candidate.to_owned()))
        // A directory URL means its index.
        .or_else(|| {
            let p = format!("{candidate}/index.html");
            UI.get_file(&p).map(|f| (f, p))
        })
        // Astro emits `about.html` for `/about`.
        .or_else(|| {
            let p = format!("{candidate}.html");
            UI.get_file(&p).map(|f| (f, p))
        });

    match resolved {
        Some((f, served)) => {
            let mut res = Response::new(Body::from(f.contents().to_vec()));
            let headers = res.headers_mut();
            headers
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type_for(&served)));
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(cache_control_for(&served)),
            );
            // The UI is served from the same origin as the API and never needs
            // to be framed by anyone.
            headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
            headers.insert("x-frame-options", HeaderValue::from_static("SAMEORIGIN"));
            res
        }
        None if have_ui() => {
            // A real UI is present but this route is not one of its pages.
            match UI.get_file("404.html") {
                Some(f) => (
                    StatusCode::NOT_FOUND,
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    f.contents().to_vec(),
                )
                    .into_response(),
                None => (StatusCode::NOT_FOUND, "not found").into_response(),
            }
        }
        None => placeholder().into_response(),
    }
}

/// Shown when the binary was built without the web application.
///
/// Says what happened and what to run, because a blank page here reads as a
/// broken install and generates a support request.
fn placeholder() -> impl IntoResponse {
    const BODY: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>mcpd — API is running, UI was not built</title>
<style>
  :root { color-scheme: light }
  body { margin:0; min-height:100vh; display:grid; place-items:center;
         background:#e8e6e1; color:#1a1a19;
         font:16px/1.6 ui-sans-serif, system-ui, sans-serif }
  main { max-width:34rem; padding:2.5rem }
  h1 { font-size:1.25rem; margin:0 0 .75rem; letter-spacing:.02em }
  p { margin:0 0 1rem; color:#4a4a46 }
  code { background:#dbd8d1; padding:.15em .4em; border-radius:2px;
         font:0.875em ui-monospace, monospace }
  ul { padding-left:1.1rem; color:#4a4a46 }
</style></head>
<body><main>
  <h1>mcpd is running. Its interface was not built into this binary.</h1>
  <p>The API is live and accepting data — only the operator interface is
     missing. Build it and rebuild the binary:</p>
  <p><code>make ui &amp;&amp; cargo build --release</code></p>
  <p>Meanwhile these all work:</p>
  <ul>
    <li><code>POST /v1/traces</code> — OpenTelemetry ingest</li>
    <li><code>POST /v1/ingest</code> — native ingest</li>
    <li><code>GET  /v1/api/traces</code> — the trace list</li>
    <li><code>GET  /healthz</code> — liveness</li>
  </ul>
</main></body></html>"#;
    (StatusCode::OK, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], BODY)
}

/// Routes that serve the embedded UI.
pub fn router() -> Router<Arc<crate::AppState>> {
    Router::new()
        .route("/", get(|| async { serve("index.html") }))
        .route("/{*path}", get(|Path(path): Path<String>| async move { serve(&path) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types_cover_what_a_built_site_emits() {
        assert_eq!(content_type_for("a/b.js"), "text/javascript; charset=utf-8");
        assert_eq!(content_type_for("x.woff2"), "font/woff2");
        assert_eq!(content_type_for("x.svg"), "image/svg+xml");
        assert_eq!(content_type_for("noextension"), "application/octet-stream");
    }

    #[test]
    fn hashed_assets_are_immutable_and_html_is_not() {
        assert!(cache_control_for("_astro/app.abc123.js").contains("immutable"));
        assert_eq!(cache_control_for("index.html"), "no-cache");
    }

    #[test]
    fn an_extensionless_route_is_typed_from_the_file_it_resolves_to() {
        // `/app` resolves to `app.html`. Typing it from the request path gives
        // `application/octet-stream`, and the browser downloads the page
        // instead of rendering it.
        assert_eq!(content_type_for("app"), "application/octet-stream");
        assert_eq!(content_type_for("app.html"), "text/html; charset=utf-8");
        assert_eq!(content_type_for("docs/quickstart.html"), "text/html; charset=utf-8");
    }

    #[test]
    fn traversal_is_refused() {
        let res = serve("../../etc/passwd");
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
