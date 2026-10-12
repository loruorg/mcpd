package mcpd

// End-to-end tests against a running mcpd.
//
// These are skipped unless an endpoint is configured, because they need a
// server. Start one and point them at it:
//
//	mcpd demo --data /tmp/mcpd-live.db --listen 127.0.0.1:4317
//	MCPD_TEST_ENDPOINT=http://127.0.0.1:4317 go test ./... -run Live
//
// What they prove that the fake collector in client_test.go cannot: that mcpd
// accepts the exact shapes this SDK sends, and that what comes back out is what
// went in. Every other test in this package talks to an httptest server that
// this repository also wrote, so the two can agree with each other and both be
// wrong about the daemon.
//
// MCPD_REQUIRE_LIVE=1 turns a missing endpoint into a failure rather than a
// skip. CI sets it in the job that starts a daemon, so if the variable is ever
// renamed or dropped the job goes red instead of quietly skipping. The Rust
// SDK's live suite spent a long time reporting three passes having contacted no
// server at all, and this is the guard against a repeat.

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"
)

// liveClient returns a client pointed at a real daemon, or skips the test.
//
// It is a helper rather than a TestMain so each test reports its own skip, and
// so a run with no server says which tests did not happen instead of silently
// reporting a green package.
func liveClient(t *testing.T, project string) *Client {
	t.Helper()

	endpoint := os.Getenv("MCPD_TEST_ENDPOINT")
	if endpoint == "" {
		if os.Getenv("MCPD_REQUIRE_LIVE") != "" {
			t.Fatal("MCPD_REQUIRE_LIVE is set but MCPD_TEST_ENDPOINT is not, so this " +
				"suite would have skipped without reaching a server. That is the exact " +
				"silence this variable exists to break.")
		}
		t.Skip("no MCPD_TEST_ENDPOINT; set it to a running daemon to run the live suite")
	}

	c := New(
		WithEndpoint(endpoint),
		WithProject(project),
		WithEnvironment("test"),
		WithService("mcpd-go-live", "0.1.0"),
		// Small, so a test that forgets to flush fails on its assertions rather
		// than on a timer that had not fired yet.
		WithBatchSize(1000),
		WithFlushInterval(time.Hour),
	)
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = c.Shutdown(ctx)
	})
	return c
}

func TestLiveWholeRunSurvivesTheRoundTrip(t *testing.T) {
	c := liveClient(t, "sdk-live-go")
	ctx := context.Background()

	health, err := c.Health(ctx)
	if err != nil {
		t.Fatalf("the server should be healthy: %v", err)
	}
	if health.Status != "ok" {
		t.Fatalf("health status: got %q, want %q", health.Status, "ok")
	}
	// A real compatibility check, not a presence check. The SDK now carries the
	// spec version it was generated from, so a daemon built from a different one
	// fails here, at connect, instead of somewhere downstream with a field that
	// silently is not there.
	if health.SpecVersion != SpecVersion {
		t.Errorf("spec version: the daemon reports %q, this SDK was generated from %q",
			health.SpecVersion, SpecVersion)
	}

	ctx, run := c.Trace(ctx, "support agent",
		WithInput("where is my order?"),
		WithTags("live-test"),
		WithSession("session-live-go"),
	)
	traceID := run.TraceID()

	_, tool := c.Start(ctx, "lookup_order", WithSpanKind(SpanKindToolCall))
	tool.SetOutput(map[string]any{"status": "shipped"})
	tool.End()

	_, call := c.Start(ctx, "answer", WithSpanKind(SpanKindModelGeneration))
	call.SetModel("claude-sonnet-4", "anthropic")
	call.SetTokens(1000, 500)
	call.End()

	_, failed := c.Start(ctx, "notify_customer", WithSpanKind(SpanKindToolCall))
	failed.RecordError(errShortSMSGateway)
	failed.End()

	run.SetOutput("It shipped on Tuesday.")
	run.End()

	c.Score(ctx, TraceTarget(traceID), "answer_relevance", 0.9, 0, 1)
	c.Feedback(ctx, TraceTarget(traceID), FeedbackKindThumbs, "up")
	c.Log(ctx, LogLevelInfo, "run finished", nil)
	c.Metric(ctx, "orders.looked_up", 1, MetricKindCounter)

	if err := c.Flush(ctx); err != nil {
		t.Fatalf("the server should have accepted the batch: %v", err)
	}

	detail, err := c.GetTrace(ctx, traceID, "spans", "logs", "scores", "feedback")
	if err != nil {
		t.Fatalf("the trace should be readable: %v", err)
	}

	if detail.Trace.Name != "support agent" {
		t.Errorf("trace name: got %q, want %q", detail.Trace.Name, "support agent")
	}
	if got := len(detail.Spans); got != 4 {
		t.Errorf("four spans, each written once and updated once: got %d", got)
	}
	if detail.Trace.ErrorCount != 1 {
		t.Errorf("error_count: got %d, want 1", detail.Trace.ErrorCount)
	}
	if detail.Trace.ToolCount != 2 {
		t.Errorf("tool_count: got %d, want 2", detail.Trace.ToolCount)
	}
	if detail.Trace.ModelCount != 1 {
		t.Errorf("model_count: got %d, want 1", detail.Trace.ModelCount)
	}
	if sid := detail.Trace.Correlation.SessionID; sid == nil || *sid != "session-live-go" {
		t.Errorf("session id did not survive the round trip: %v", sid)
	}

	// mcpd priced the model call from its own price book at ingest. The SDK
	// sent tokens and no cost, so a cost here can only have come from the
	// daemon, which is the point of asserting it.
	if detail.Trace.Cost == nil || detail.Trace.Cost.TotalCost == nil ||
		*detail.Trace.Cost.TotalCost <= 0 {
		t.Error("a priced model call should have produced a cost")
	}

	var errSpan *Span
	for i := range detail.Spans {
		if detail.Spans[i].Name == "notify_customer" {
			errSpan = &detail.Spans[i]
		}
	}
	if errSpan == nil {
		t.Fatal("the failed tool span should be stored")
	}
	if errSpan.Status != SpanStatusError {
		t.Errorf("a recorded error should make the span an error: got %q", errSpan.Status)
	}
	if errSpan.Error == nil || errSpan.Error.Message != errShortSMSGateway.Error() {
		t.Errorf("the error message did not survive: %+v", errSpan.Error)
	}

	if got := len(detail.Scores); got != 1 {
		t.Errorf("scores: got %d, want 1", got)
	}
	if got := len(detail.Feedback); got != 1 {
		t.Errorf("feedback: got %d, want 1", got)
	}
}

func TestLiveReadSurfaceAnswers(t *testing.T) {
	c := liveClient(t, "sdk-live-go-read")
	ctx := context.Background()

	_, run := c.Trace(ctx, "readable run", WithInput("q"))
	traceID := run.TraceID()
	run.SetOutput("a")
	run.End()

	if err := c.Flush(ctx); err != nil {
		t.Fatalf("the server should have accepted the batch: %v", err)
	}

	list, err := c.ListTraces(ctx, TraceQuery{Project: "sdk-live-go-read", Limit: 50})
	if err != nil {
		t.Fatalf("listing traces should work: %v", err)
	}
	var found bool
	for _, tr := range list.Traces {
		if tr.TraceID == traceID {
			found = true
		}
	}
	if !found {
		t.Errorf("the run just written was not in its own project's listing (%d traces)",
			len(list.Traces))
	}
}

func TestLiveTraceContinuesAcrossAServiceBoundary(t *testing.T) {
	c := liveClient(t, "sdk-live-go-ctx")
	ctx := context.Background()

	// The caller opens a run and hands the context over the wire.
	ctx, caller := c.Trace(ctx, "caller")
	traceID := caller.TraceID()
	header := caller.Traceparent()

	// The callee is a different process in real life. It joins the trace from
	// the header alone, which is the whole contract of traceparent.
	parent, ok := ParseTraceparent(header)
	if !ok {
		t.Fatalf("the SDK emitted a traceparent it cannot parse: %q", header)
	}
	// Start, not Trace. Trace forces a root and documents that it ignores what
	// is in the context, which is the right behaviour for a process running many
	// independent runs and the wrong call for joining someone else's.
	remote := ContextWithRemoteParent(context.Background(), parent)
	_, callee := c.Start(remote, "callee")
	callee.End()

	caller.End()

	if err := c.Flush(ctx); err != nil {
		t.Fatalf("the server should have accepted the batch: %v", err)
	}

	detail, err := c.GetTrace(ctx, traceID, "spans")
	if err != nil {
		t.Fatalf("the trace should be readable: %v", err)
	}
	if got := len(detail.Spans); got != 2 {
		t.Fatalf("both sides of the boundary should be in one trace: got %d spans", got)
	}
}

// A fixed error so the assertion and the recording cannot drift apart.
var errShortSMSGateway = liveError("sms gateway timed out after 5s")

type liveError string

func (e liveError) Error() string { return string(e) }

// Every read method, against the daemon that answers them. The unit test
// proves the request shape and the decoding; this proves the daemon accepts
// those shapes and answers with the fields the generated types expect.
func TestLiveEveryReadMethodAnswers(t *testing.T) {
	c := liveClient(t, "sdk-live-go-reads")
	ctx := context.Background()

	ctx, run := c.Trace(ctx, "read me", WithInput("q"))
	traceID := run.TraceID()
	_, step := c.Start(ctx, "step", WithSpanKind(SpanKindWorkflowStep))
	c.Log(ctx, LogLevelWarn, "a line to find", nil)
	c.Metric(ctx, "orders.looked_up", 1, MetricKindCounter)
	step.End()
	run.End()
	c.Score(ctx, TraceTarget(traceID), "answer_relevance", 0.5, 0, 1)
	c.Feedback(ctx, TraceTarget(traceID), FeedbackKindThumbs, "down")

	if err := c.Flush(ctx); err != nil {
		t.Fatalf("the server should have accepted the batch: %v", err)
	}

	spans, err := c.ListSpans(ctx, SpanQuery{TraceID: traceID})
	if err != nil {
		t.Fatalf("list spans: %v", err)
	}
	if got := len(spans.Spans); got != 2 {
		t.Errorf("list spans: got %d spans for the run, want 2", got)
	}

	logs, err := c.ListLogs(ctx, LogQuery{TraceID: traceID})
	if err != nil {
		t.Fatalf("list logs: %v", err)
	}
	if len(logs.Logs) != 1 || logs.Logs[0].Message != "a line to find" {
		t.Errorf("list logs: got %+v, want the one line written", logs.Logs)
	}

	from := time.Now().Add(-time.Hour)
	to := time.Now().Add(time.Minute)
	metrics, err := c.QueryMetrics(ctx, MetricQuery{Name: "orders.looked_up", From: from, To: to})
	if err != nil {
		t.Fatalf("query metrics: %v", err)
	}
	if metrics.Name != "orders.looked_up" {
		t.Errorf("query metrics answered for %q", metrics.Name)
	}

	scores, err := c.ListScores(ctx, ScoreQuery{TraceID: traceID})
	if err != nil {
		t.Fatalf("list scores: %v", err)
	}
	if len(scores.Scores) != 1 || scores.Scores[0].ScorerID != "answer_relevance" {
		t.Errorf("list scores: got %+v", scores.Scores)
	}

	feedback, err := c.ListFeedback(ctx, FeedbackQuery{TraceID: traceID})
	if err != nil {
		t.Fatalf("list feedback: %v", err)
	}
	if len(feedback.Feedback) != 1 || feedback.Feedback[0].Kind != FeedbackKindThumbs {
		t.Errorf("list feedback: got %+v", feedback.Feedback)
	}

	summary, err := c.AnalyticsSummary(ctx, from, to, "model")
	if err != nil {
		t.Fatalf("analytics summary: %v", err)
	}
	if summary.TraceCount < 1 {
		t.Errorf("analytics summary counted %d traces in a window with at least one", summary.TraceCount)
	}

	cost, err := c.AnalyticsCost(ctx, from, to, "model", "1h")
	if err != nil {
		t.Fatalf("analytics cost: %v", err)
	}
	if cost.Currency != "USD" {
		t.Errorf("analytics cost answered in %q", cost.Currency)
	}
}

func TestLiveAWorkflowIsServedAndTheRunAndItsTraceReadBack(t *testing.T) {
	c := liveClient(t, "sdk-live-go-runs-"+newTraceID()[:8])
	ctx := context.Background()

	w := NewWorkflow("live_support").
		Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
			var input struct {
				Ticket string `json:"ticket"`
			}
			if err := step.Input(&input); err != nil {
				return nil, err
			}
			kind := "bug"
			if strings.HasPrefix(input.Ticket, "Q") {
				kind = "question"
			}
			return map[string]any{"kind": kind}, nil
		}).
		Agent("draft", func(ctx context.Context, step *StepContext) (any, error) {
			var input struct {
				Kind string `json:"kind"`
			}
			if err := step.Input(&input); err != nil {
				return nil, err
			}
			step.SetTokens(120, 40)
			step.SetState(map[string]any{"drafted": true})
			_, span := c.Start(ctx, "live_model", WithModel("claude-sonnet-4", "anthropic"))
			span.SetTokens(120, 40)
			span.End()
			return "an answer to a " + input.Kind, nil
		}, Input(map[string]any{"kind": "${steps.classify.output.kind}"})).
		Output(map[string]any{"answer": "${steps.draft.output}"})

	worker := NewWorker(c, WorkerID("live-worker"), ClaimWait(time.Second), WorkerOnError(func(message string, err error) {
		t.Errorf("%s: %v", message, err)
	})).Register(w)
	if err := worker.RegisterDefinitions(ctx); err != nil {
		t.Fatalf("register: %v", err)
	}

	run, err := c.StartRun(ctx, StartRun{Definition: "live_support", Input: map[string]any{"ticket": "Q-42"}})
	if err != nil {
		t.Fatalf("start: %v", err)
	}
	if run.Status != RunStatusRunning || run.DefinitionID != "live_support" {
		t.Fatalf("run: %+v", run)
	}

	detail, err := c.GetRun(ctx, run.RunID)
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 10 && detail.Run.Status == RunStatusRunning; i++ {
		if _, err := worker.Claim(ctx, 4, run.RunID); err != nil {
			t.Fatalf("claim: %v", err)
		}
		worker.Drain()
		if detail, err = c.GetRun(ctx, run.RunID); err != nil {
			t.Fatal(err)
		}
	}
	if detail.Run.Status != RunStatusSucceeded {
		t.Fatalf("status: %s (%v)", detail.Run.Status, detail.Run.Error)
	}
	if string(detail.Run.Output) != `{"answer":"an answer to a question"}` {
		t.Fatalf("output: %s", detail.Run.Output)
	}
	if string(detail.Run.State) != `{"drafted":true}` {
		t.Fatalf("state: %s", detail.Run.State)
	}
	byKey := map[string]StepAttempt{}
	for _, a := range detail.Attempts {
		byKey[a.StepKey] = a
	}
	if byKey["classify"].Status != AttemptStatusSucceeded || byKey["draft"].Status != AttemptStatusSucceeded {
		t.Fatalf("attempts: %+v", detail.Attempts)
	}
	if byKey["draft"].Kind != StepKindAgent || string(byKey["draft"].Input) != `{"kind":"question"}` {
		t.Fatalf("draft attempt: %+v", byKey["draft"])
	}
	if byKey["draft"].Usage == nil || byKey["draft"].Usage.InputTokens == nil || *byKey["draft"].Usage.InputTokens != 120 {
		t.Fatalf("draft usage: %+v", byKey["draft"].Usage)
	}

	var kinds []RunEventKind
	if err := c.watchRun(ctx, run.RunID, -1, 50*time.Millisecond, func(e RunEvent) error { kinds = append(kinds, e.Kind); return nil }); err != nil {
		t.Fatal(err)
	}
	if kinds[0] != RunEventKindRunStarted || kinds[len(kinds)-1] != RunEventKindRunEnded {
		t.Fatalf("kinds: %v", kinds)
	}

	if err := c.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	// The daemon writes the run's own spans through an outbox drained on its
	// own clock, so give it a moment before reading the trace.
	var trace *TraceDetail
	for i := 0; i < 40; i++ {
		trace, err = c.GetTrace(ctx, run.TraceID)
		if err == nil && len(trace.Spans) >= 4 {
			found := false
			for _, s := range trace.Spans {
				if s.Name == "live_model" {
					found = true
				}
			}
			if found {
				break
			}
		}
		time.Sleep(250 * time.Millisecond)
	}
	if err != nil {
		t.Fatal(err)
	}
	var model *Span
	for i := range trace.Spans {
		if trace.Spans[i].Name == "live_model" {
			model = &trace.Spans[i]
		}
	}
	if model == nil {
		t.Fatalf("live_model span not in trace: %d spans", len(trace.Spans))
	}
	if model.ParentSpanID == nil || *model.ParentSpanID != byKey["draft"].SpanID {
		t.Fatalf("live_model parent: %v want %s", model.ParentSpanID, byKey["draft"].SpanID)
	}
}

func TestLiveAWaitResumesOnPublishAndAnotherRunEndsOnCancel(t *testing.T) {
	c := liveClient(t, "sdk-live-go-runs-"+newTraceID()[:8])
	ctx := context.Background()

	w := NewWorkflow("live_approval").
		WaitFor("approval", "approved", "${input.ticket}", time.Minute).
		Step("close", func(ctx context.Context, step *StepContext) (any, error) {
			var input struct {
				By string `json:"by"`
			}
			if err := step.Input(&input); err != nil {
				return nil, err
			}
			return map[string]any{"closed_by": input.By}, nil
		}, Input(map[string]any{"by": "${steps.approval.output.by}"})).
		Output("${steps.close.output}")
	worker := NewWorker(c, WorkerID("live-approver"), ClaimWait(500*time.Millisecond), WorkerOnError(func(message string, err error) {
		t.Errorf("%s: %v", message, err)
	})).Register(w)
	if err := worker.RegisterDefinitions(ctx); err != nil {
		t.Fatalf("register: %v", err)
	}

	waiting, err := c.StartRun(ctx, StartRun{Definition: "live_approval", Input: map[string]any{"ticket": "T-7"}})
	if err != nil {
		t.Fatal(err)
	}
	detail, err := c.GetRun(ctx, waiting.RunID)
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 10 && len(detail.Waits) == 0; i++ {
		time.Sleep(100 * time.Millisecond)
		if detail, err = c.GetRun(ctx, waiting.RunID); err != nil {
			t.Fatal(err)
		}
	}
	if len(detail.Waits) != 1 || detail.Waits[0].Event != "approved" || detail.Waits[0].Key != "T-7" {
		t.Fatalf("waits: %+v", detail.Waits)
	}

	receipt, err := c.PublishEvent(ctx, PublishEvent{Event: "approved", Key: "T-7", Payload: map[string]any{"by": "ops"}, EventID: "live-approval-go-1"})
	if err != nil {
		t.Fatal(err)
	}
	if receipt.Outcome != EventOutcomeDelivered {
		t.Fatalf("outcome: %s", receipt.Outcome)
	}
	again, err := c.PublishEvent(ctx, PublishEvent{Event: "approved", Key: "T-7", EventID: "live-approval-go-1"})
	if err != nil {
		t.Fatal(err)
	}
	if again.Outcome != EventOutcomeDuplicate {
		t.Fatalf("outcome: %s", again.Outcome)
	}

	for i := 0; i < 10 && detail.Run.Status == RunStatusRunning; i++ {
		if _, err := worker.Claim(ctx, 1, waiting.RunID); err != nil {
			t.Fatal(err)
		}
		worker.Drain()
		if detail, err = c.GetRun(ctx, waiting.RunID); err != nil {
			t.Fatal(err)
		}
	}
	if detail.Run.Status != RunStatusSucceeded || string(detail.Run.Output) != `{"closed_by":"ops"}` {
		t.Fatalf("run: %s %s", detail.Run.Status, detail.Run.Output)
	}

	doomed, err := c.StartRun(ctx, StartRun{Definition: "live_approval", Input: map[string]any{"ticket": "T-8"}})
	if err != nil {
		t.Fatal(err)
	}
	cancelled, err := c.CancelRun(ctx, doomed.RunID)
	if err != nil {
		t.Fatal(err)
	}
	if cancelled.Status != RunStatusCancelled {
		t.Fatalf("status: %s", cancelled.Status)
	}
	var last RunEventKind
	if err := c.watchRun(ctx, doomed.RunID, -1, 50*time.Millisecond, func(e RunEvent) error { last = e.Kind; return nil }); err != nil {
		t.Fatal(err)
	}
	if last != RunEventKindRunEnded {
		t.Fatalf("last event: %s", last)
	}

	listed, err := c.ListRuns(ctx, RunQuery{DefinitionID: "live_approval"})
	if err != nil {
		t.Fatal(err)
	}
	if len(listed.Runs) != 2 {
		t.Fatalf("listed %d runs", len(listed.Runs))
	}
}
