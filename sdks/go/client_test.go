package mcpd

import (
	"compress/gzip"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"
)

// collector is a stand-in mcpd: it records every batch it is sent and can be
// told to fail a fixed number of times first.
type collector struct {
	mu        sync.Mutex
	batches   []IngestBatch
	encodings []string
	keys      []string
	failures  []string
	statuses  []int
	requests  int
}

func (c *collector) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	c.mu.Lock()
	c.requests++
	status := http.StatusOK
	if len(c.statuses) > 0 {
		status, c.statuses = c.statuses[0], c.statuses[1:]
	}
	c.mu.Unlock()

	body, err := readRequestBody(r)
	var batch IngestBatch
	if err == nil {
		err = json.Unmarshal(body, &batch)
	}

	c.mu.Lock()
	if err != nil {
		c.failures = append(c.failures, err.Error())
	} else {
		c.batches = append(c.batches, batch)
		c.encodings = append(c.encodings, r.Header.Get("Content-Encoding"))
		key := ""
		if batch.IngestKey != nil {
			key = *batch.IngestKey
		}
		c.keys = append(c.keys, key)
	}
	c.mu.Unlock()

	if status != http.StatusOK {
		w.WriteHeader(status)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(IngestResult{Accepted: uint32(len(batch.Spans))})
}

func readRequestBody(r *http.Request) ([]byte, error) {
	defer r.Body.Close()
	if r.Header.Get("Content-Encoding") == "gzip" {
		reader, err := gzip.NewReader(r.Body)
		if err != nil {
			return nil, err
		}
		defer reader.Close()
		return io.ReadAll(reader)
	}
	return io.ReadAll(r.Body)
}

func (c *collector) spans() []Span {
	c.mu.Lock()
	defer c.mu.Unlock()
	var out []Span
	for _, batch := range c.batches {
		out = append(out, batch.Spans...)
	}
	return out
}

func (c *collector) batchCount() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return len(c.batches)
}

func (c *collector) requestCount() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.requests
}

func (c *collector) ingestKeys() []string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return append([]string(nil), c.keys...)
}

func (c *collector) decodeFailures() []string {
	c.mu.Lock()
	defer c.mu.Unlock()
	return append([]string(nil), c.failures...)
}

// newTestClient returns a client pointed at a collector, with the timers turned
// off so a test controls exactly when an export happens.
func newTestClient(t *testing.T, opts ...Option) (*Client, *collector) {
	t.Helper()
	sink := &collector{}
	server := httptest.NewServer(sink)
	t.Cleanup(server.Close)

	base := []Option{
		WithEndpoint(server.URL),
		WithProject("test"),
		WithEnabled(true),
		WithFlushInterval(time.Hour),
		WithTimeout(2 * time.Second),
		WithLogger(nil),
	}
	client := New(append(base, opts...)...)
	t.Cleanup(func() { client.Shutdown(context.Background()) })
	return client, sink
}

func waitFor(t *testing.T, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if cond() {
			return
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", what)
}

func TestSpansAreBatchedIntoOneRequest(t *testing.T) {
	client, sink := newTestClient(t)

	ctx := context.Background()
	for i := 0; i < 5; i++ {
		_, span := client.Start(ctx, "step")
		span.End()
	}
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	if got := sink.batchCount(); got != 1 {
		t.Fatalf("expected one batch, got %d", got)
	}
	spans := sink.spans()
	if len(spans) != 5 {
		t.Fatalf("expected 5 spans in the batch, got %d", len(spans))
	}
	seen := map[string]bool{}
	for _, span := range spans {
		// A span that ended before the queue drained is sent once, finished.
		if span.EndTime == nil {
			t.Errorf("span %s was exported without an end time", span.SpanID)
		}
		if span.Status != SpanStatusOk {
			t.Errorf("span %s ended %q, want ok", span.SpanID, span.Status)
		}
		if seen[span.SpanID] {
			t.Errorf("span %s was exported twice", span.SpanID)
		}
		seen[span.SpanID] = true
	}
	if failures := sink.decodeFailures(); len(failures) > 0 {
		t.Fatalf("the server could not decode a batch: %v", failures)
	}
}

func TestUnfinishedSpansAreExportedOnStart(t *testing.T) {
	client, sink := newTestClient(t, WithBatchSize(3))

	ctx := context.Background()
	var open []*ActiveSpan
	for i := 0; i < 3; i++ {
		_, span := client.Start(ctx, "long_run")
		open = append(open, span)
	}
	// Reaching the batch size wakes the worker without waiting for the timer.
	waitFor(t, "the unfinished spans to be exported", func() bool { return sink.batchCount() >= 1 })

	for _, span := range sink.spans() {
		if span.EndTime != nil {
			t.Errorf("span %s was exported with an end time before it ended", span.SpanID)
		}
		if span.Status != SpanStatusUnset {
			t.Errorf("unfinished span %s has status %q, want unset", span.SpanID, span.Status)
		}
	}

	for _, span := range open {
		span.End()
	}
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	ended := 0
	for _, span := range sink.spans() {
		if span.EndTime != nil {
			ended++
		}
	}
	if ended != 3 {
		t.Fatalf("expected 3 finished spans in the update, got %d", ended)
	}
}

func TestModelParametersReachTheWire(t *testing.T) {
	client, sink := newTestClient(t)
	ctx := context.Background()

	_, span := client.Start(ctx, "generate", WithSpanKind(SpanKindModelGeneration))
	span.SetModelParameters(ModelParameters{
		Temperature:       ptrFloat64(0.2),
		TopP:              ptrFloat64(0.95),
		TopK:              ptrUint32(40),
		MaxTokens:         ptrUint64(1024),
		Seed:              ptrInt64(7),
		StopSequences:     []string{"\n\nHuman:"},
		HasResponseFormat: true,
	})
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	got := onlySpan(t, sink.spans(), "generate").ModelParameters
	if got == nil {
		t.Fatal("the model parameters never reached the wire")
	}
	if *got.Temperature != 0.2 || *got.TopP != 0.95 || *got.TopK != 40 {
		t.Errorf("sampling settings: %+v", got)
	}
	if *got.MaxTokens != 1024 || *got.Seed != 7 || !got.HasResponseFormat {
		t.Errorf("ceiling, seed and response format: %+v", got)
	}
	if len(got.StopSequences) != 1 || got.StopSequences[0] != "\n\nHuman:" {
		t.Errorf("stop sequences: %q", got.StopSequences)
	}
}

func TestAZeroSeedIsASeed(t *testing.T) {
	// The classic bug this guards: reading zero as "not given". A zero seed is
	// what a reproducible run is pinned to and a zero temperature is the
	// setting most worth recording, so neither may be dropped on the way out.
	// The collector decodes the JSON the client actually sent, so a field lost
	// to omitempty comes back as a nil pointer here.
	client, sink := newTestClient(t)
	ctx := context.Background()

	_, span := client.Start(ctx, "greedy")
	span.SetModelParameters(ModelParameters{
		Temperature: ptrFloat64(0),
		TopP:        ptrFloat64(0),
		Seed:        ptrInt64(0),
	})
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	got := onlySpan(t, sink.spans(), "greedy").ModelParameters
	if got == nil {
		t.Fatal("the model parameters never reached the wire")
	}
	if got.Seed == nil || *got.Seed != 0 {
		t.Errorf("seed: %v, want a pointer to 0", got.Seed)
	}
	if got.Temperature == nil || *got.Temperature != 0 {
		t.Errorf("temperature: %v, want a pointer to 0", got.Temperature)
	}
	if got.TopP == nil || *got.TopP != 0 {
		t.Errorf("top_p: %v, want a pointer to 0", got.TopP)
	}
	if got.HasResponseFormat {
		t.Error("has_response_format defaults to false")
	}
}

func TestChildInheritsTraceAndCorrelation(t *testing.T) {
	client, sink := newTestClient(t)

	ctx := context.Background()
	ctx, root := client.Start(ctx, "agent", WithSession("sess-1"), WithUser("u-1"))
	_, child := client.Start(ctx, "tool", WithSpanKind(SpanKindToolCall))
	child.End()
	root.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	spans := sink.spans()
	if len(spans) != 2 {
		t.Fatalf("expected 2 spans, got %d", len(spans))
	}
	byName := map[string]Span{}
	for _, span := range spans {
		byName[span.Name] = span
	}
	parent, ok := byName["agent"]
	if !ok {
		t.Fatal("the root span was not exported")
	}
	step, ok := byName["tool"]
	if !ok {
		t.Fatal("the child span was not exported")
	}
	if step.TraceID != parent.TraceID {
		t.Errorf("child trace %s != root trace %s", step.TraceID, parent.TraceID)
	}
	if step.ParentSpanID == nil || *step.ParentSpanID != parent.SpanID {
		t.Errorf("child parent id is %v, want %s", step.ParentSpanID, parent.SpanID)
	}
	if !parent.IsRoot || step.IsRoot {
		t.Errorf("root flags are wrong: root=%v child=%v", parent.IsRoot, step.IsRoot)
	}
	if parent.Kind != SpanKindAgentRun {
		t.Errorf("a root defaults to %q, want agent_run", parent.Kind)
	}
	if step.Correlation.SessionID == nil || *step.Correlation.SessionID != "sess-1" {
		t.Errorf("the child did not inherit the session: %v", step.Correlation.SessionID)
	}
	if step.Correlation.UserID == nil || *step.Correlation.UserID != "u-1" {
		t.Errorf("the child did not inherit the user: %v", step.Correlation.UserID)
	}
}

func TestRecordedErrorSetsStatusAndMessage(t *testing.T) {
	client, sink := newTestClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "tool")
	span.RecordError(errors.New("smtp: mailbox busy"), ErrorRetryable(true), ErrorDomain("email"))
	span.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	spans := sink.spans()
	if len(spans) != 1 {
		t.Fatalf("expected 1 span, got %d", len(spans))
	}
	got := spans[0]
	if got.Status != SpanStatusError {
		t.Errorf("status is %q, want error", got.Status)
	}
	if got.Error == nil {
		t.Fatal("the error detail was not exported")
	}
	if got.Error.Message != "smtp: mailbox busy" {
		t.Errorf("message is %q", got.Error.Message)
	}
	if got.Error.Retryable == nil || !*got.Error.Retryable {
		t.Error("retryable was not recorded")
	}
	if got.Error.Domain == nil || *got.Error.Domain != "email" {
		t.Error("domain was not recorded")
	}
}

type smtpFailure struct{ code int }

func (e *smtpFailure) Error() string { return "mailbox busy" }

func TestErrorKindIsTheTypeOnlyWhenItSaysSomething(t *testing.T) {
	client, sink := newTestClient(t)

	ctx := context.Background()
	_, plain := client.Start(ctx, "plain")
	plain.RecordError(errors.New("boom"))
	plain.End()

	_, typed := client.Start(ctx, "typed")
	typed.RecordError(&smtpFailure{code: 451})
	typed.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	for _, span := range sink.spans() {
		if span.Error == nil {
			t.Fatalf("span %s lost its error", span.Name)
		}
		switch span.Name {
		case "plain":
			// *errors.errorString is not an error class anyone can act on.
			if span.Error.Kind != nil {
				t.Errorf("recorded a useless kind %q", *span.Error.Kind)
			}
		case "typed":
			if span.Error.Kind == nil || *span.Error.Kind != "*mcpd.smtpFailure" {
				t.Errorf("kind is %v, want the concrete type", span.Error.Kind)
			}
		}
	}
}

func TestNilErrorIsIgnored(t *testing.T) {
	client, sink := newTestClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.RecordError(nil)
	span.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	spans := sink.spans()
	if len(spans) != 1 || spans[0].Status != SpanStatusOk || spans[0].Error != nil {
		t.Fatalf("a nil error changed the span: %+v", spans)
	}
}

func TestOtherSignalsAreCorrelatedAndSent(t *testing.T) {
	client, sink := newTestClient(t)

	ctx := context.Background()
	ctx, span := client.Start(ctx, "agent")
	client.Log(ctx, LogLevelInfo, "ticket received", map[string]any{"channel": "email"})
	client.Metric(ctx, "support.tickets", 1, MetricKindCounter, MetricUnit("{ticket}"))
	client.Score(ctx, Target{}, "helpfulness", 0.86, 0, 1, ScoreReason("answered"))
	client.Feedback(ctx, Target{}, FeedbackKindThumbs, "up", FeedbackComment("fast"))
	span.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	sink.mu.Lock()
	defer sink.mu.Unlock()
	var logs []LogRecord
	var metrics []MetricPoint
	var scores []Score
	var feedback []Feedback
	for _, batch := range sink.batches {
		logs = append(logs, batch.Logs...)
		metrics = append(metrics, batch.Metrics...)
		scores = append(scores, batch.Scores...)
		feedback = append(feedback, batch.Feedback...)
	}
	if len(logs) != 1 || logs[0].SpanID == nil || *logs[0].SpanID != span.SpanID() {
		t.Errorf("the log was not correlated to the span: %+v", logs)
	}
	if len(metrics) != 1 || metrics[0].TraceID == nil || *metrics[0].TraceID != span.TraceID() {
		t.Errorf("the metric was not correlated to the trace: %+v", metrics)
	}
	if len(scores) != 1 {
		t.Fatalf("expected 1 score, got %d", len(scores))
	}
	if scores[0].Min == nil || scores[0].Max == nil || *scores[0].Min != 0 || *scores[0].Max != 1 {
		t.Errorf("the score lost its scale: %+v", scores[0])
	}
	if scores[0].Source != ScoreSourceLive {
		t.Errorf("score source is %q, want live", scores[0].Source)
	}
	if len(feedback) != 1 || string(feedback[0].Value) != `"up"` {
		t.Errorf("feedback value is %q", string(feedback[0].Value))
	}
}

func TestScoreWithoutATargetIsDropped(t *testing.T) {
	client, sink := newTestClient(t)

	client.Score(context.Background(), Target{}, "helpfulness", 1, 0, 1)
	if err := client.Flush(context.Background()); err != nil {
		t.Fatalf("flush: %v", err)
	}
	if sink.requestCount() != 0 {
		t.Fatalf("a score with nothing to attach to was sent anyway")
	}
}

func TestDisabledClientRecordsNothingAndStillWorks(t *testing.T) {
	client, sink := newTestClient(t, WithEnabled(false))

	ctx := context.Background()
	ctx, span := client.Start(ctx, "agent", WithTags("a"))
	if span == nil {
		t.Fatal("a disabled client must still return a span")
	}
	if span.IsRecording() {
		t.Error("a disabled client must not record")
	}
	if !isTraceID(span.TraceID()) || !isSpanID(span.SpanID()) {
		t.Errorf("a disabled span still needs valid ids, got %q / %q", span.TraceID(), span.SpanID())
	}
	span.SetInput("prompt")
	span.RecordError(errors.New("boom"))
	span.Event("chunk")
	client.Log(ctx, LogLevelError, "nope", nil)
	client.Metric(ctx, "nope", 1, MetricKindCounter)
	client.Score(ctx, TraceTarget(span.TraceID()), "s", 1, 0, 1)
	client.Feedback(ctx, TraceTarget(span.TraceID()), FeedbackKindThumbs, "up")
	span.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush on a disabled client: %v", err)
	}
	if err := client.Shutdown(ctx); err != nil {
		t.Fatalf("shutdown on a disabled client: %v", err)
	}
	if sink.requestCount() != 0 {
		t.Fatalf("a disabled client sent %d requests", sink.requestCount())
	}
	if stats := client.Stats(); stats.Queued != 0 {
		t.Fatalf("a disabled client queued %d signals", stats.Queued)
	}
}

func TestSamplingIsDecidedAtTheRootAndInherited(t *testing.T) {
	client, sink := newTestClient(t, WithSampleRate(0))

	ctx := context.Background()
	ctx, root := client.Start(ctx, "agent")
	if root.IsRecording() {
		t.Fatal("a trace sampled out must not record")
	}
	_, child := client.Start(ctx, "step")
	if child.IsRecording() {
		t.Fatal("a child must inherit the root's decision")
	}
	child.End()
	root.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	if sink.requestCount() != 0 {
		t.Fatalf("an unsampled trace was exported")
	}

	kept, sink2 := newTestClient(t, WithSampleRate(1))
	ctx2, span := kept.Start(context.Background(), "agent")
	if !span.IsRecording() {
		t.Fatal("a fully sampled trace must record")
	}
	span.End()
	if err := kept.Flush(ctx2); err != nil {
		t.Fatalf("flush: %v", err)
	}
	if len(sink2.spans()) != 1 {
		t.Fatalf("expected the sampled span to arrive, got %d", len(sink2.spans()))
	}
}

func TestRemoteParentContinuesTheTrace(t *testing.T) {
	client, sink := newTestClient(t)

	header := http.Header{}
	header.Set(TraceparentHeader, "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
	ctx := Extract(context.Background(), header)

	_, span := client.Start(ctx, "handler")
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	spans := sink.spans()
	if len(spans) != 1 {
		t.Fatalf("expected 1 span, got %d", len(spans))
	}
	if spans[0].TraceID != "4bf92f3577b34da6a3ce929d0e0e4736" {
		t.Errorf("the trace was not continued: %s", spans[0].TraceID)
	}
	if spans[0].ParentSpanID == nil || *spans[0].ParentSpanID != "00f067aa0ba902b7" {
		t.Errorf("the upstream parent was lost: %v", spans[0].ParentSpanID)
	}
	if spans[0].IsRoot {
		t.Error("a span continuing an upstream trace is not a root")
	}
}

func TestADeadServerNeverRaises(t *testing.T) {
	// A server that is closed before anything is sent: every attempt fails at
	// the connection.
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {}))
	endpoint := server.URL
	server.Close()

	client := New(
		WithEndpoint(endpoint),
		WithEnabled(true),
		WithFlushInterval(time.Hour),
		WithTimeout(200*time.Millisecond),
		WithLogger(nil),
	)

	ctx := context.Background()
	for i := 0; i < 20; i++ {
		_, span := client.Start(ctx, "step")
		span.SetInput(map[string]any{"i": i})
		span.End()
	}
	client.Log(ctx, LogLevelWarn, "still fine", nil)

	// Flush reports the failure to the caller who asked to wait for it, and
	// nothing anywhere panics or blocks.
	if err := client.Flush(ctx); err == nil {
		t.Error("expected Flush to report the export failure")
	}
	_ = client.Shutdown(ctx)
	if stats := client.Stats(); stats.Failed == 0 {
		t.Error("failed signals were not counted")
	}
}

func TestShutdownIsIdempotent(t *testing.T) {
	client, _ := newTestClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.End()

	if err := client.Shutdown(ctx); err != nil {
		t.Fatalf("first shutdown: %v", err)
	}
	if err := client.Shutdown(ctx); err != nil {
		t.Fatalf("second shutdown: %v", err)
	}
	// Recording after shutdown must not panic or block either.
	_, after := client.Start(ctx, "after")
	after.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush after shutdown: %v", err)
	}
}

func TestNilClientIsSafe(t *testing.T) {
	var client *Client
	ctx := context.Background()

	ctx, span := client.Start(ctx, "step")
	if span == nil {
		t.Fatal("a nil client must still return a span")
	}
	if !isTraceID(span.TraceID()) {
		t.Errorf("a nil client must still return valid ids, got %q", span.TraceID())
	}
	span.SetInput("x")
	span.SetTokens(1, 2)
	span.RecordError(errors.New("boom"))
	span.Event("chunk")
	span.End()

	client.Log(ctx, LogLevelInfo, "message", nil)
	client.Metric(ctx, "name", 1, MetricKindCounter)
	client.Score(ctx, TraceTarget(span.TraceID()), "s", 1, 0, 1)
	client.Feedback(ctx, TraceTarget(span.TraceID()), FeedbackKindThumbs, "up")
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush on a nil client: %v", err)
	}
	if err := client.Shutdown(ctx); err != nil {
		t.Fatalf("shutdown on a nil client: %v", err)
	}
}

func TestEnvironmentConfiguresTheClientAndCodeWins(t *testing.T) {
	t.Setenv("MCPD_ENDPOINT", "http://127.0.0.1:9999")
	t.Setenv("MCPD_PROJECT", "from-env")
	t.Setenv("MCPD_ENABLED", "true")
	t.Setenv("MCPD_SAMPLE", "0.5")

	client := New()
	defer client.Shutdown(context.Background())
	if client.Endpoint() != "http://127.0.0.1:9999" {
		t.Errorf("endpoint is %q", client.Endpoint())
	}
	if client.Project() != "from-env" {
		t.Errorf("project is %q", client.Project())
	}
	if client.cfg.sample != 0.5 {
		t.Errorf("sample is %v", client.cfg.sample)
	}

	override := New(WithProject("from-code"), WithEnabled(false))
	defer override.Shutdown(context.Background())
	if override.Project() != "from-code" {
		t.Errorf("code did not win over the environment: %q", override.Project())
	}
	if override.Enabled() {
		t.Error("WithEnabled(false) did not win over MCPD_ENABLED=true")
	}
}

func TestBadEndpointDisablesRecordingInsteadOfFailing(t *testing.T) {
	client := New(WithEndpoint("not a url at all"), WithLogger(nil))
	defer client.Shutdown(context.Background())

	if client.Enabled() {
		t.Fatal("a client with an unusable endpoint must not try to record")
	}
	_, span := client.Start(context.Background(), "step")
	span.End()
}

// The five run-scoped fields are set on the run, and everything underneath
// carries them: the log, the metric, the score and the feedback emitted inside
// it, and a child opened after the run was tagged. Three SDKs once read
// correlation from the client at construction instead, so a session set on the
// run never reached anything under it.
func TestSignalsInsideARunCarryTheRunsCorrelation(t *testing.T) {
	client, sink := newTestClient(t)

	ctx := context.Background()
	ctx, run := client.Start(ctx, "agent", WithSession("sess-1"), WithUser("u-1"))
	client.Log(ctx, LogLevelInfo, "inside the run", nil)
	client.Metric(ctx, "steps", 1, MetricKindCounter)
	client.Score(ctx, Target{}, "helpfulness", 0.5, 0, 1)
	client.Feedback(ctx, Target{}, FeedbackKindThumbs, "up")

	// Set after the run started, so a child that inherits at creation time
	// from the run's current state sees it and one that copied the run's
	// options at start does not.
	run.SetThread("thread-7")
	_, late := client.Start(ctx, "late step")
	late.End()
	run.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	sink.mu.Lock()
	defer sink.mu.Unlock()
	want := func(what string, cor Correlation) {
		t.Helper()
		if cor.SessionID == nil || *cor.SessionID != "sess-1" {
			t.Errorf("%s did not carry the run's session: %v", what, cor.SessionID)
		}
		if cor.UserID == nil || *cor.UserID != "u-1" {
			t.Errorf("%s did not carry the run's user: %v", what, cor.UserID)
		}
	}
	var logs, metrics, scores, feedback int
	for _, batch := range sink.batches {
		for _, l := range batch.Logs {
			logs++
			want("the log", l.Correlation)
		}
		for _, m := range batch.Metrics {
			metrics++
			want("the metric", m.Correlation)
		}
		for _, s := range batch.Scores {
			scores++
			want("the score", s.Correlation)
		}
		for _, f := range batch.Feedback {
			feedback++
			want("the feedback", f.Correlation)
		}
		for _, span := range batch.Spans {
			if span.Name != "late step" {
				continue
			}
			want("the late child", span.Correlation)
			if span.Correlation.ThreadID == nil || *span.Correlation.ThreadID != "thread-7" {
				t.Errorf("a thread set on the run after it started did not reach a later child: %v",
					span.Correlation.ThreadID)
			}
		}
	}
	if logs != 1 || metrics != 1 || scores != 1 || feedback != 1 {
		t.Fatalf("expected one of each signal, got %d logs, %d metrics, %d scores, %d feedback",
			logs, metrics, scores, feedback)
	}
}
