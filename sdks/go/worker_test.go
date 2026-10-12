package mcpd

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

const (
	leaseTrace = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
	leaseSpan  = "b2b2b2b2b2b2b2b2"
)

type reply struct {
	status int
	body   any
}

type recorded struct {
	method string
	path   string
	query  string
	body   map[string]any
	raw    []byte
}

// daemon is a stand-in for the run surface: it answers each path from a queue
// of replies, repeating the last one, and records what it was sent.
type daemon struct {
	mu     sync.Mutex
	routes map[string][]reply
	calls  []recorded
	server *httptest.Server
}

func newDaemon(t *testing.T, routes map[string][]reply) *daemon {
	t.Helper()
	d := &daemon{routes: routes}
	d.server = httptest.NewServer(d)
	t.Cleanup(d.server.Close)
	return d
}

func (d *daemon) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	raw, _ := readRequestBody(r)
	var body map[string]any
	_ = json.Unmarshal(raw, &body)
	d.mu.Lock()
	d.calls = append(d.calls, recorded{method: r.Method, path: r.URL.Path, query: r.URL.RawQuery, body: body, raw: raw})
	answer := reply{status: http.StatusOK, body: map[string]any{}}
	for suffix, queue := range d.routes {
		if strings.HasSuffix(r.URL.Path, suffix) && len(queue) > 0 {
			answer = queue[0]
			if len(queue) > 1 {
				d.routes[suffix] = queue[1:]
			}
			break
		}
	}
	d.mu.Unlock()
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(answer.status)
	_ = json.NewEncoder(w).Encode(answer.body)
}

func (d *daemon) paths() []string {
	d.mu.Lock()
	defer d.mu.Unlock()
	out := make([]string, 0, len(d.calls))
	for _, c := range d.calls {
		out = append(out, c.method+" "+c.path)
	}
	return out
}

func (d *daemon) call(i int) recorded {
	d.mu.Lock()
	defer d.mu.Unlock()
	return d.calls[i]
}

func (d *daemon) count() int {
	d.mu.Lock()
	defer d.mu.Unlock()
	return len(d.calls)
}

func wireLease(overrides map[string]any) map[string]any {
	lease := map[string]any{
		"lease_id": "lease-1", "run_id": "run-1", "project": "test", "definition_id": "support",
		"definition_version": 1, "step_key": "classify", "kind": "task", "attempt": 0,
		"effect": "at_least_once", "expires_at": "2026-09-01T10:00:00Z", "idempotency_key": "run-1:classify:0",
		"input":       map[string]any{"ticket_id": "T-1"},
		"prior":       map[string]any{"triage": map[string]any{"status": "succeeded", "output": map[string]any{"queue_name": "billing"}}},
		"state":       map[string]any{"seen_count": 1},
		"correlation": map[string]any{"project": "test", "session_id": "s-1"},
		"trace_id":    leaseTrace, "span_id": leaseSpan, "parent_span_id": "c3c3c3c3c3c3c3c3",
	}
	for k, v := range overrides {
		lease[k] = v
	}
	return lease
}

func claimed(leases ...map[string]any) reply {
	if leases == nil {
		leases = []map[string]any{}
	}
	return reply{http.StatusOK, map[string]any{"leases": leases}}
}

var (
	started    = reply{http.StatusOK, map[string]any{"expires_at": "2026-09-01T10:00:30Z", "reclaimed": false}}
	acked      = reply{http.StatusOK, map[string]any{"run_status": "running", "next": "claim"}}
	registered = reply{http.StatusOK, map[string]any{"worker_id": "w-1",
		"registered":   []map[string]any{{"id": "support", "version": 1, "assigned": true}},
		"lease_ttl_ms": 3000, "heartbeat_ms": 1000}}
)

func conflict(reason string, extra map[string]any) reply {
	body := map[string]any{"error": "conflict", "reason": reason, "message": "the daemon says " + reason,
		"attempt": 0, "run_status": "running"}
	for k, v := range extra {
		body[k] = v
	}
	return reply{http.StatusConflict, body}
}

// The daemon holds an idle claim open for the whole of Wait, so the answer
// arrives after the client's own timeout and within the wait. A claim cut off
// by that timeout is what made a healthy idle worker report a failure every
// ten seconds.
func TestAnIdleClaimOutlivesTheClientTimeoutByTheWait(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		time.Sleep(300 * time.Millisecond)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"leases": []}`))
	}))
	t.Cleanup(server.Close)
	client := New(WithEndpoint(server.URL), WithProject("test"), WithFlushInterval(time.Hour),
		WithTimeout(100*time.Millisecond), WithLogger(nil))
	t.Cleanup(func() { _ = client.Shutdown(context.Background()) })

	response, err := client.ClaimSteps(context.Background(), Claim{WorkerID: "w-1", Max: 1, Wait: 2 * time.Second})
	if err != nil {
		t.Fatalf("an idle claim answered within the wait failed: %v", err)
	}
	if len(response.Leases) != 0 {
		t.Fatalf("expected no leases, got %d", len(response.Leases))
	}
}

type harness struct {
	daemon *daemon
	client *Client
	worker *Worker
	mu     sync.Mutex
	errors []string
}

func newHarness(t *testing.T, routes map[string][]reply, opts ...WorkerOption) *harness {
	t.Helper()
	h := &harness{daemon: newDaemon(t, routes)}
	h.client = New(WithEndpoint(h.daemon.server.URL), WithProject("test"), WithFlushInterval(time.Hour),
		WithTimeout(2*time.Second), WithLogger(nil))
	t.Cleanup(func() { _ = h.client.Shutdown(context.Background()) })
	base := []WorkerOption{WorkerID("w-1"), ClaimWait(0), WorkerOnError(func(message string, err error) {
		h.mu.Lock()
		h.errors = append(h.errors, message+": "+err.Error())
		h.mu.Unlock()
	})}
	h.worker = NewWorker(h.client, append(base, opts...)...)
	return h
}

func (h *harness) reported() []string {
	h.mu.Lock()
	defer h.mu.Unlock()
	return append([]string(nil), h.errors...)
}

func (h *harness) claim(t *testing.T, max int) int {
	t.Helper()
	n, err := h.worker.Claim(context.Background(), max, "")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	h.worker.Drain()
	return n
}

func equalJSON(t *testing.T, got any, want any) {
	t.Helper()
	g, _ := json.Marshal(got)
	w, _ := json.Marshal(want)
	var gv, wv any
	_ = json.Unmarshal(g, &gv)
	_ = json.Unmarshal(w, &wv)
	gs, _ := json.Marshal(gv)
	ws, _ := json.Marshal(wv)
	if string(gs) != string(ws) {
		t.Fatalf("\n got: %s\nwant: %s", gs, ws)
	}
}

func expectPaths(t *testing.T, got []string, want ...string) {
	t.Helper()
	if strings.Join(got, "\n") != strings.Join(want, "\n") {
		t.Fatalf("paths\n got: %v\nwant: %v", got, want)
	}
}

func TestOneLeaseRunsStartedTheBodyAndComplete(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started}, "/complete": {acked}})
	var seen []any
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		var input map[string]any
		var prior map[string]any
		var state map[string]any
		if err := step.Input(&input); err != nil {
			return nil, err
		}
		if err := step.Prior(&prior); err != nil {
			return nil, err
		}
		if err := step.State(&state); err != nil {
			return nil, err
		}
		seen = append(seen, input, prior, state, step.IdempotencyKey, step.Attempt, step.RunID, step.StepKey)
		step.SetState(map[string]any{"seen_count": 2, "last_ticket": "T-1"})
		step.SetTokens(10, 5)
		return map[string]any{"kind": "question"}, nil
	}))
	if n := h.claim(t, 1); n != 1 {
		t.Fatalf("claimed %d", n)
	}
	expectPaths(t, h.daemon.paths(), "POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/complete")
	equalJSON(t, h.daemon.call(0).body, map[string]any{"project": "test", "worker_id": "w-1", "max": 1, "wait_ms": 0})
	// Opaque objects reach the body spelled as the daemon sent them.
	equalJSON(t, seen, []any{
		map[string]any{"ticket_id": "T-1"},
		map[string]any{"triage": map[string]any{"status": "succeeded", "output": map[string]any{"queue_name": "billing"}}},
		map[string]any{"seen_count": 1}, "run-1:classify:0", 0, "run-1", "classify",
	})
	equalJSON(t, h.daemon.call(2).body, map[string]any{
		"output": map[string]any{"kind": "question"},
		"state":  map[string]any{"seen_count": 2, "last_ticket": "T-1"},
		"usage":  map[string]any{"input_tokens": 10, "output_tokens": 5},
	})
	if errs := h.reported(); len(errs) != 0 {
		t.Fatalf("reported: %v", errs)
	}
	if h.worker.Inflight() != 0 {
		t.Fatal("inflight should be zero")
	}
}

func TestABodyThatReturnsNothingCompletesWithNoOutput(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started}, "/complete": {acked}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) { return nil, nil }))
	h.claim(t, 1)
	equalJSON(t, h.daemon.call(2).body, map[string]any{})
}

func TestASpanFromTheBodyLandsUnderTheStepSpanTheDaemonOpened(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started}, "/complete": {acked},
		"/v1/ingest": {{http.StatusOK, map[string]any{"accepted": 1, "rejected": 0, "duplicate": 0}}}})
	h.worker.Register(NewWorkflow("support").Agent("classify", func(ctx context.Context, step *StepContext) (any, error) {
		// The handler's ctx carries the step span as the remote parent.
		_, span := h.client.Start(ctx, "model_call", WithModel("claude-sonnet-4", "anthropic"))
		span.End()
		_, explicit := step.Start(context.Background(), "explicit_child")
		explicit.End()
		return "ok", nil
	}))
	h.claim(t, 1)
	if err := h.client.Flush(context.Background()); err != nil {
		t.Fatal(err)
	}
	var batch IngestBatch
	for i := 0; i < h.daemon.count(); i++ {
		if c := h.daemon.call(i); strings.HasSuffix(c.path, "/v1/ingest") {
			if err := json.Unmarshal(c.raw, &batch); err != nil {
				t.Fatal(err)
			}
		}
	}
	if len(batch.Spans) != 2 {
		t.Fatalf("spans sent: %d", len(batch.Spans))
	}
	for _, span := range batch.Spans {
		if span.TraceID != leaseTrace {
			t.Fatalf("%s trace: %s", span.Name, span.TraceID)
		}
		if span.ParentSpanID == nil || *span.ParentSpanID != leaseSpan {
			t.Fatalf("%s parent: %v", span.Name, span.ParentSpanID)
		}
		if span.Correlation.SessionID == nil || *span.Correlation.SessionID != "s-1" {
			t.Fatalf("%s should carry the run's session: %+v", span.Name, span.Correlation)
		}
	}
}

func TestABranchAnItemAndAnIterationKeyFindTheirHandlers(t *testing.T) {
	h := newHarness(t, map[string][]reply{
		"/steps/claim": {claimed(
			wireLease(map[string]any{"lease_id": "l-a", "step_key": "checks/fraud"}),
			wireLease(map[string]any{"lease_id": "l-b", "step_key": "notify[2]"}),
			wireLease(map[string]any{"lease_id": "l-c", "step_key": "refine[1]"}))},
		"/started": {started}, "/complete": {acked},
	}, WorkerConcurrency(3))
	var mu sync.Mutex
	var ran []string
	record := func(name string) StepHandler {
		return func(ctx context.Context, step *StepContext) (any, error) {
			mu.Lock()
			ran = append(ran, name)
			mu.Unlock()
			return name, nil
		}
	}
	h.worker.Register(NewWorkflow("support").
		Parallel("checks", func(b *Branch) { b.Step("fraud", record("fraud")) }).
		ForEach("notify", "input.to", 3, func(b *Branch) { b.Step("send", record("send")) }).
		Loop("refine", 2, func(b *Branch) { b.Step("rewrite", record("rewrite")) }))
	if n := h.claim(t, 3); n != 3 {
		t.Fatalf("claimed %d", n)
	}
	if len(ran) != 3 {
		t.Fatalf("ran: %v", ran)
	}
	completes := 0
	for _, p := range h.daemon.paths() {
		if strings.HasSuffix(p, "/complete") {
			completes++
		}
	}
	if completes != 3 {
		t.Fatalf("completes: %d", completes)
	}
}

func TestARetryableErrorIsReportedWithItsClassAndDelay(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started},
		"/fail": {{http.StatusOK, map[string]any{"run_status": "running", "next": "idle", "attempts_left": 2}}}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		return nil, Retryable(errors.New("rate limited"), 250*time.Millisecond)
	}))
	h.claim(t, 1)
	expectPaths(t, h.daemon.paths(), "POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/fail")
	equalJSON(t, h.daemon.call(2).body, map[string]any{"error": map[string]any{"class": "retryable", "message": "rate limited", "retry_after_ms": 250}})
}

func TestAnyOtherErrorIsFatalAndAFailureNamesItsClass(t *testing.T) {
	h := newHarness(t, map[string][]reply{
		"/steps/claim": {claimed(wireLease(nil), wireLease(map[string]any{"lease_id": "lease-2", "step_key": "draft"}))},
		"/started":     {started}, "/fail": {acked},
	}, WorkerConcurrency(2))
	h.worker.Register(NewWorkflow("support").
		Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
			return nil, errors.New("the model returned nothing")
		}).
		Step("draft", func(ctx context.Context, step *StepContext) (any, error) {
			return nil, &Failure{Class: StepErrorClassInvalidInput, Message: "no such ticket", Details: map[string]any{"field": "ticket"}}
		}))
	h.claim(t, 2)
	fails := map[string]any{}
	for i := 0; i < h.daemon.count(); i++ {
		if c := h.daemon.call(i); strings.HasSuffix(c.path, "/fail") {
			fails[c.path] = c.body
		}
	}
	equalJSON(t, fails, map[string]any{
		"/v1/api/steps/lease-1/fail": map[string]any{"error": map[string]any{"class": "fatal", "message": "the model returned nothing"}},
		"/v1/api/steps/lease-2/fail": map[string]any{"error": map[string]any{"class": "invalid_input", "message": "no such ticket", "details": map[string]any{"field": "ticket"}}},
	})
}

func TestAPanicInABodyIsReportedAsFatalNotCrashed(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started}, "/fail": {acked}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) { panic("boom") }))
	h.claim(t, 1)
	body := h.daemon.call(2).body["error"].(map[string]any)
	if body["class"] != "fatal" || !strings.Contains(body["message"].(string), "boom") {
		t.Fatalf("fail body: %v", body)
	}
}

func TestALeaseItCannotServeIsHandedBackWithoutRunningAnything(t *testing.T) {
	h := newHarness(t, map[string][]reply{
		"/steps/claim": {claimed(wireLease(map[string]any{"definition_id": "billing"}), wireLease(map[string]any{"lease_id": "lease-2", "definition_version": 7}))},
		"/fail":        {acked},
	}, WorkerConcurrency(2))
	ran := false
	h.worker.Register(NewWorkflow("support", WithVersion(1)).Step("classify", func(ctx context.Context, step *StepContext) (any, error) { ran = true; return nil, nil }))
	h.claim(t, 2)
	if ran {
		t.Fatal("the body ran")
	}
	for i := 1; i < h.daemon.count(); i++ {
		c := h.daemon.call(i)
		if !strings.HasSuffix(c.path, "/fail") {
			t.Fatalf("unexpected call %s", c.path)
		}
		errBody := c.body["error"].(map[string]any)
		if errBody["class"] != "unservable" || !strings.Contains(errBody["message"].(string), "w-1 does not serve") {
			t.Fatalf("fail body: %v", errBody)
		}
	}
}

func TestAlreadyAppliedIsDoneAndSaysNothing(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started},
		"/complete": {conflict("already_applied", map[string]any{"attempt_status": "succeeded"})}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) { return 1, nil }))
	h.claim(t, 1)
	if h.daemon.count() != 3 {
		t.Fatalf("calls: %v", h.daemon.paths())
	}
	if errs := h.reported(); len(errs) != 0 {
		t.Fatalf("reported: %v", errs)
	}
}

func TestABodyWhoseStartedWasFencedNeverRuns(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))},
		"/started": {conflict("fenced", map[string]any{"attempt_status": "orphaned"})}})
	ran := false
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) { ran = true; return nil, nil }))
	h.claim(t, 1)
	if ran {
		t.Fatal("the body ran")
	}
	expectPaths(t, h.daemon.paths(), "POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started")
	if errs := h.reported(); len(errs) != 0 {
		t.Fatalf("reported: %v", errs)
	}
}

func TestALeaseWhoseRunEndedIsAbandonedWithNothingMorePosted(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started},
		"/complete": {conflict("run_terminal", map[string]any{"run_status": "cancelled"})}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) { return 1, nil }))
	h.claim(t, 1)
	if h.daemon.count() != 3 || len(h.reported()) != 0 {
		t.Fatalf("calls %v reported %v", h.daemon.paths(), h.reported())
	}
}

func TestNotStartedIsLoggedLoudlyBecauseItIsABugInTheWorker(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(map[string]any{"effect": "at_most_once"}))},
		"/started": {started}, "/complete": {conflict("not_started", nil)}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) { return 1, nil }))
	h.claim(t, 1)
	errs := h.reported()
	if len(errs) != 1 || !strings.Contains(errs[0], "a bug in this worker") {
		t.Fatalf("reported: %v", errs)
	}
}

func TestAFencedHeartbeatCancelsTheBodyAndPostsNoComplete(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/workers/register": {registered}, "/steps/claim": {claimed(wireLease(nil))},
		"/started": {started}, "/heartbeat": {conflict("fenced", map[string]any{"attempt_status": "orphaned"})}})
	causes := make(chan error, 1)
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		<-ctx.Done()
		causes <- context.Cause(ctx)
		return "too late", nil
	}))
	if err := h.worker.RegisterDefinitions(context.Background()); err != nil {
		t.Fatal(err)
	}
	h.claim(t, 1)
	// The worker no longer waits for the body once its lease is fenced, so the
	// body's report arrives on its own goroutine.
	var cause error
	select {
	case cause = <-causes:
	case <-time.After(2 * time.Second):
	}
	var conflictErr *StepConflictError
	if !errors.As(cause, &conflictErr) || conflictErr.Conflict.Reason != ConflictReasonFenced {
		t.Fatalf("cause: %v", cause)
	}
	expectPaths(t, h.daemon.paths(), "POST /v1/api/workers/register", "POST /v1/api/steps/claim",
		"POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/heartbeat")
	if errs := h.reported(); len(errs) != 0 {
		t.Fatalf("reported: %v", errs)
	}
}

func TestAHeartbeatThatFailsForAnyOtherReasonIsReportedAndTheBodyGoesOn(t *testing.T) {
	// A 400 is not retried by the transport; a 5xx would be, and would land.
	h := newHarness(t, map[string][]reply{"/workers/register": {registered}, "/steps/claim": {claimed(wireLease(nil))},
		"/started": {started}, "/heartbeat": {{http.StatusBadRequest, map[string]any{"error": "bad_request", "message": "no"}}}, "/complete": {acked}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		time.Sleep(1150 * time.Millisecond)
		return "done", nil
	}))
	if err := h.worker.RegisterDefinitions(context.Background()); err != nil {
		t.Fatal(err)
	}
	h.claim(t, 1)
	paths := h.daemon.paths()
	if paths[len(paths)-1] != "POST /v1/api/steps/lease-1/complete" {
		t.Fatalf("paths: %v", paths)
	}
	found := false
	for _, e := range h.reported() {
		if strings.Contains(e, "heartbeat failed") {
			found = true
		}
	}
	if !found {
		t.Fatalf("reported: %v", h.reported())
	}
}

func TestServeRegistersClaimsUntilStoppedAndReturnsOnceStopped(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/workers/register": {registered}, "/steps/claim": {claimed()}}, WorkerConcurrency(2))
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) { return 1, nil }))
	served := make(chan error, 1)
	go func() { served <- h.worker.Serve(context.Background()) }()
	time.Sleep(50 * time.Millisecond)
	h.worker.Stop()
	select {
	case err := <-served:
		if err != nil {
			t.Fatalf("serve: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("serve did not return after stop")
	}
	paths := h.daemon.paths()
	if paths[0] != "POST /v1/api/workers/register" {
		t.Fatalf("paths: %v", paths)
	}
	equalJSON(t, h.daemon.call(0).body, map[string]any{"project": "test", "worker_id": "w-1", "definitions": []any{
		map[string]any{"id": "support", "kind": "workflow", "body": map[string]any{"steps": []any{map[string]any{"kind": "task", "id": "classify"}}}}}})
	if len(paths) < 3 {
		t.Fatalf("too few claims: %v", paths)
	}
	for _, p := range paths[1:] {
		if p != "POST /v1/api/steps/claim" {
			t.Fatalf("unexpected call %s", p)
		}
	}
	equalJSON(t, h.daemon.call(1).body, map[string]any{"project": "test", "worker_id": "w-1", "max": 2, "wait_ms": 0})
	if errs := h.reported(); len(errs) != 0 {
		t.Fatalf("reported: %v", errs)
	}
}

func TestServeReturnsWhenItsContextEnds(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/workers/register": {registered}, "/steps/claim": {claimed()}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) { return 1, nil }))
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	if err := h.worker.Serve(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("serve: %v", err)
	}
}

func TestAClaimThatFailedIsReportedAndTriedAgain(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/workers/register": {registered},
		"/steps/claim": {{http.StatusBadRequest, map[string]any{"error": "bad_request", "message": "no"}}, claimed()}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) { return 1, nil }))
	served := make(chan error, 1)
	go func() { served <- h.worker.Serve(context.Background()) }()
	time.Sleep(50 * time.Millisecond)
	h.worker.Stop()
	<-served
	errs := h.reported()
	if len(errs) == 0 || !strings.Contains(errs[0], "claim failed") {
		t.Fatalf("reported: %v", errs)
	}
}

func TestARunStartsWithTheFieldsTheDaemonExpects(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/v1/api/runs": {{http.StatusOK, map[string]any{
		"run_id": "run-1", "project": "test", "definition_id": "support", "definition_version": 2, "status": "running",
		"cursor": map[string]any{"index": 0, "construct": "task"}, "correlation": map[string]any{"project": "test"},
		"trace_id": leaseTrace, "steps_used": 0, "cost_usd": 0, "root_run_id": "run-1", "depth": 0,
		"created_at": "2026-09-01T10:00:00Z", "updated_at": "2026-09-01T10:00:00Z"}}}})
	session := "s-1"
	run, err := h.client.StartRun(context.Background(), StartRun{Definition: "support", Version: 2,
		Input: map[string]any{"ticket_id": "T-1"}, IdempotencyKey: "ticket:T-1", Correlation: &Correlation{SessionID: &session}})
	if err != nil {
		t.Fatal(err)
	}
	if run.RunID != "run-1" || run.Status != RunStatusRunning {
		t.Fatalf("run: %+v", run)
	}
	equalJSON(t, h.daemon.call(0).body, map[string]any{"project": "test", "definition_id": "support", "version": 2,
		"input": map[string]any{"ticket_id": "T-1"}, "idempotency_key": "ticket:T-1",
		"correlation": map[string]any{"project": "test", "session_id": "s-1"}})
}

func TestADefinitionIsStoredWithItsBodyUntouched(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/v1/api/definitions": {{http.StatusOK, map[string]any{"id": "support", "version": 1, "assigned": true}}}})
	doc, err := NewWorkflow("support").Step("classify", noop, StepTimeout(100*time.Millisecond)).Document()
	if err != nil {
		t.Fatal(err)
	}
	stored, err := h.client.PutDefinition(context.Background(), doc, "")
	if err != nil {
		t.Fatal(err)
	}
	if stored.ID != "support" || stored.Version != 1 || !stored.Assigned {
		t.Fatalf("stored: %+v", stored)
	}
	if c := h.daemon.call(0); c.query != "project=test" {
		t.Fatalf("query: %s", c.query)
	}
	equalJSON(t, h.daemon.call(0).body, map[string]any{"id": "support", "kind": "workflow",
		"body": map[string]any{"steps": []any{map[string]any{"kind": "task", "id": "classify", "step_timeout_ms": 100}}}})
}

func TestWatchRunWalksThePagesUntilTheRunEndsPassingTheLastSeqItSaw(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/events": {
		{http.StatusOK, map[string]any{"events": []map[string]any{
			{"run_id": "run-1", "seq": 0, "kind": "run_started", "payload": map[string]any{"definition_id": "support"}, "at": "2026-09-01T10:00:00Z"},
			{"run_id": "run-1", "seq": 1, "kind": "step_leased", "payload": map[string]any{"step_key": "classify"}, "at": "2026-09-01T10:00:01Z"},
		}, "has_more": true}},
		{http.StatusOK, map[string]any{"events": []map[string]any{}, "has_more": false}},
		{http.StatusOK, map[string]any{"events": []map[string]any{
			{"run_id": "run-1", "seq": 2, "kind": "run_ended", "payload": map[string]any{"status": "succeeded"}, "at": "2026-09-01T10:00:02Z"},
		}, "has_more": false}},
	}})
	var kinds []RunEventKind
	err := h.client.watchRun(context.Background(), "run-1", -1, time.Millisecond, func(e RunEvent) error {
		kinds = append(kinds, e.Kind)
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	equalJSON(t, kinds, []string{"run_started", "step_leased", "run_ended"})
	for i, want := range []string{"after=-1&limit=500", "after=1&limit=500", "after=1&limit=500"} {
		if got := h.daemon.call(i).query; got != want {
			t.Fatalf("call %d query: %s want %s", i, got, want)
		}
	}
}

func TestPublishAndSignalCarryTheIDsTheCallerSupplied(t *testing.T) {
	receipt := reply{http.StatusOK, map[string]any{"outcome": "delivered", "event_id": "evt-1"}}
	h := newHarness(t, map[string][]reply{"/v1/api/events": {receipt}, "/signal": {receipt}})
	ctx := context.Background()
	if _, err := h.client.PublishEvent(ctx, PublishEvent{Event: "approved", Key: "T-1", Payload: map[string]any{"by": "ops"}, EventID: "evt-1"}); err != nil {
		t.Fatal(err)
	}
	if _, err := h.client.SignalRun(ctx, "run-1", "approved", map[string]any{"by": "ops"}, "evt-2"); err != nil {
		t.Fatal(err)
	}
	equalJSON(t, h.daemon.call(0).body, map[string]any{"project": "test", "event": "approved", "key": "T-1", "payload": map[string]any{"by": "ops"}, "event_id": "evt-1"})
	if p := h.daemon.call(1).path; p != "/v1/api/runs/run-1/signal" {
		t.Fatalf("path: %s", p)
	}
	equalJSON(t, h.daemon.call(1).body, map[string]any{"event": "approved", "payload": map[string]any{"by": "ops"}, "event_id": "evt-2"})
}

func TestA409FromAWorkerCallIsATypedConflictAndAnyOther409IsItself(t *testing.T) {
	h := newHarness(t, map[string][]reply{
		"/complete":           {conflict("fenced", map[string]any{"attempt_status": "orphaned"})},
		"/v1/api/definitions": {{http.StatusConflict, map[string]any{"error": "version_taken", "message": "support@1 exists with a different body"}}},
	})
	_, err := h.client.CompleteStep(context.Background(), "lease-1", StepResult{})
	var typed *StepConflictError
	if !errors.As(err, &typed) || typed.Conflict.Reason != ConflictReasonFenced || typed.Conflict.AttemptStatus == nil || *typed.Conflict.AttemptStatus != AttemptStatusOrphaned {
		t.Fatalf("err: %v", err)
	}
	var api *APIError
	if !errors.As(err, &api) || api.Status != http.StatusConflict {
		t.Fatalf("the api error should unwrap: %v", err)
	}
	doc, _ := NewWorkflow("support", WithVersion(1)).Document()
	_, err = h.client.PutDefinition(context.Background(), doc, "")
	if errors.As(err, &typed) {
		t.Fatalf("a version conflict is not a step conflict: %v", err)
	}
	if !errors.As(err, &api) || api.Code != "version_taken" {
		t.Fatalf("err: %v", err)
	}
}

func TestABodyThatRunsPastTheLeaseDeadlineIsCancelledAndFailedRetryable(t *testing.T) {
	deadline := uint64(150)
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(map[string]any{"deadline_ms": deadline}))}, "/started": {started}, "/fail": {acked}})
	causes := make(chan error, 1)
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		<-ctx.Done()
		causes <- context.Cause(ctx)
		return "too late", nil
	}))
	h.claim(t, 1)
	// The worker no longer waits for the body once its lease is given up, so
	// the body's report arrives on its own goroutine.
	var cause error
	select {
	case cause = <-causes:
	case <-time.After(2 * time.Second):
	}
	if cause == nil || cause.Error() != "step deadline of 150ms passed" {
		t.Fatalf("cause: %v", cause)
	}
	expectPaths(t, h.daemon.paths(), "POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/fail")
	equalJSON(t, h.daemon.call(2).body, map[string]any{"error": map[string]any{"class": "retryable", "message": "step deadline of 150ms passed"}})
	if errs := h.reported(); len(errs) != 0 {
		t.Fatalf("reported: %v", errs)
	}
}

func TestTheSlotIsFreedAtTheDeadlineEvenWhenTheBodyIgnoresItsContext(t *testing.T) {
	deadline := uint64(100)
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(map[string]any{"deadline_ms": deadline}))}, "/started": {started}, "/fail": {acked}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		// A body that never looks at ctx.
		time.Sleep(3 * time.Second)
		return "too late", nil
	}))
	began := time.Now()
	h.claim(t, 1)
	if took := time.Since(began); took > time.Second {
		t.Fatalf("the slot was held for %v, past the deadline", took)
	}
	if inflight := h.worker.Stats().Inflight; inflight != 0 {
		t.Fatalf("inflight: %d", inflight)
	}
	expectPaths(t, h.daemon.paths(), "POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/fail")
}

func TestInputThatDoesNotFitFailsTheStepInvalidInput(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started}, "/fail": {acked}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		var input struct {
			TicketID int `json:"ticket_id"`
		}
		if err := step.Input(&input); err != nil {
			return nil, err
		}
		return input, nil
	}))
	h.claim(t, 1)
	body := h.daemon.call(2).body["error"].(map[string]any)
	if body["class"] != "invalid_input" || !strings.Contains(body["message"].(string), "the step's input could not be read") {
		t.Fatalf("fail body: %v", body)
	}
}

func TestWaitRunReturnsTheOutputOrATypedFailure(t *testing.T) {
	run := func(status string, extra map[string]any) map[string]any {
		r := map[string]any{
			"run_id": "run-1", "project": "test", "definition_id": "support", "definition_version": 1, "status": status,
			"cursor": map[string]any{"index": 2, "construct": "end"}, "correlation": map[string]any{"project": "test"},
			"trace_id": leaseTrace, "steps_used": 2, "cost_usd": 0.25, "root_run_id": "run-1", "depth": 0,
			"created_at": "2026-09-01T10:00:00Z", "updated_at": "2026-09-01T10:00:05Z",
		}
		for k, v := range extra {
			r[k] = v
		}
		return r
	}
	ended := reply{http.StatusOK, map[string]any{"events": []map[string]any{{"run_id": "run-1", "seq": 0, "kind": "run_ended", "at": "2026-09-01T10:00:05Z"}}, "has_more": false}}
	h := newHarness(t, map[string][]reply{
		"/events":            {ended},
		"/v1/api/runs/run-1": {{http.StatusOK, map[string]any{"run": run("succeeded", map[string]any{"output": map[string]any{"answer": 42}}), "attempts": []any{}, "timers": []any{}, "waits": []any{}}}},
	})
	result, err := h.client.waitRun(context.Background(), "run-1", -1, time.Millisecond)
	if err != nil {
		t.Fatal(err)
	}
	var out struct {
		Answer int `json:"answer"`
	}
	if err := result.OutputAs(&out); err != nil || out.Answer != 42 || result.CostUSD != 0.25 || result.TraceID != leaseTrace {
		t.Fatalf("result: %+v %v", result, err)
	}

	h2 := newHarness(t, map[string][]reply{
		"/events":            {ended},
		"/v1/api/runs/run-1": {{http.StatusOK, map[string]any{"run": run("failed", map[string]any{"error": "send: relay down"}), "attempts": []any{}, "timers": []any{}, "waits": []any{}}}},
	})
	_, err = h2.client.waitRun(context.Background(), "run-1", -1, time.Millisecond)
	var failed *RunFailedError
	if !errors.As(err, &failed) || failed.Run.Status != RunStatusFailed || err.Error() != "mcpd: run run-1 failed: send: relay down" {
		t.Fatalf("err: %v", err)
	}

	h3 := newHarness(t, map[string][]reply{"/events": {{http.StatusOK, map[string]any{"events": []any{}, "has_more": false}}}})
	ctx, cancel := context.WithTimeout(context.Background(), 40*time.Millisecond)
	defer cancel()
	_, err = h3.client.waitRun(ctx, "run-1", -1, 5*time.Millisecond)
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("err: %v", err)
	}
}
