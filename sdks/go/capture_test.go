package mcpd

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

// The tests in this file cover sections 2.1 to 2.3 and the record call of
// section 3 of sdks/SDK-CONTRACT.md: payload capture, sinks, stats, a span
// recorded after the fact, and the external parent.

func ptrU64(v uint64) *uint64 { return &v }

func onlySpan(t *testing.T, spans []Span, name string) Span {
	t.Helper()
	for _, span := range spans {
		if span.Name == name {
			return span
		}
	}
	t.Fatalf("no span named %q among %d", name, len(spans))
	return Span{}
}

func TestCaptureOffKeepsInputAndOutputInTheProcess(t *testing.T) {
	client, sink := newTestClient(t, WithCapturePayloads(false))
	ctx := context.Background()

	_, span := client.Start(ctx, "model_call",
		WithInput("the prompt"), WithOutput("the completion"), WithModel("claude-sonnet-4", "anthropic"))
	span.SetInput("a later prompt")
	span.SetOutput("a later completion")
	span.SetTokens(3, 4)
	span.SetModelParameters(ModelParameters{Temperature: ptrFloat64(0), Seed: ptrInt64(0)})
	span.Event("chunk", WithInput("chunk text"), WithOutput("chunk out"))
	span.RecordError(errors.New("boom"))
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}

	spans := sink.spans()
	if len(spans) != 2 {
		t.Fatalf("spans on the wire: %d", len(spans))
	}
	for _, s := range spans {
		if s.Input != nil || s.Output != nil {
			t.Fatalf("%s carried a payload: in=%s out=%s", s.Name, s.Input, s.Output)
		}
	}
	call := onlySpan(t, spans, "model_call")
	if call.Usage == nil || *call.Usage.InputTokens != 3 || call.Model == nil || *call.Model != "claude-sonnet-4" {
		t.Fatalf("the rest of the record did not survive: %+v", call)
	}
	if call.Status != SpanStatusError || call.Error == nil || call.Error.Message != "boom" {
		t.Fatalf("status and error did not survive: %v %+v", call.Status, call.Error)
	}
	// Sampling settings are metadata, not payload: no prompt is in them, and
	// they are what the run has to be reproduced from. Capture off must not
	// take them, and a zero seed must survive the trip.
	if call.ModelParameters == nil || call.ModelParameters.Seed == nil || *call.ModelParameters.Seed != 0 {
		t.Fatalf("the model parameters did not survive capture off: %+v", call.ModelParameters)
	}
	if call.ModelParameters.Temperature == nil || *call.ModelParameters.Temperature != 0 {
		t.Fatalf("a zero temperature did not survive: %+v", call.ModelParameters)
	}
	if client.CapturesPayloads() || span.CapturesPayloads() {
		t.Fatal("capture should read as off")
	}
}

func TestCaptureOffFromTheEnvironment(t *testing.T) {
	t.Setenv("MCPD_CAPTURE_PAYLOADS", "off")
	client, sink := newTestClient(t)
	if client.CapturesPayloads() {
		t.Fatal("MCPD_CAPTURE_PAYLOADS=off should turn capture off")
	}
	ctx := context.Background()
	_, span := client.Start(ctx, "call", WithInput("secret"))
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	if s := onlySpan(t, sink.spans(), "call"); s.Input != nil {
		t.Fatalf("input leaked: %s", s.Input)
	}
	t.Setenv("MCPD_CAPTURE_PAYLOADS", "true")
	if on, _ := newTestClient(t); !on.CapturesPayloads() {
		t.Fatal("MCPD_CAPTURE_PAYLOADS=true should leave capture on")
	}
}

func TestASpanOverridesTheClientForItselfInEitherDirection(t *testing.T) {
	ctx := context.Background()

	off, sinkOff := newTestClient(t, WithCapturePayloads(false))
	_, allowed := off.Start(ctx, "allowed", WithInput("kept"), WithSpanCapturePayloads(true))
	_, child := allowed.Child(ctx, "child", WithInput("dropped"))
	child.End()
	allowed.End()
	if err := off.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	if s := onlySpan(t, sinkOff.spans(), "allowed"); string(s.Input) != `"kept"` {
		t.Fatalf("the override did not let the payload through: %s", s.Input)
	}
	if s := onlySpan(t, sinkOff.spans(), "child"); s.Input != nil {
		t.Fatalf("the override reached the child: %s", s.Input)
	}

	on, sinkOn := newTestClient(t)
	_, private := on.Start(ctx, "private", WithInput("dropped"), WithSpanCapturePayloads(false))
	private.SetOutput("also dropped")
	_, sibling := on.Start(ctx, "sibling", WithInput("kept"))
	sibling.End()
	private.End()
	if err := on.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	if s := onlySpan(t, sinkOn.spans(), "private"); s.Input != nil || s.Output != nil {
		t.Fatalf("the private span carried a payload: %s %s", s.Input, s.Output)
	}
	if s := onlySpan(t, sinkOn.spans(), "sibling"); string(s.Input) != `"kept"` {
		t.Fatalf("the sibling lost its payload: %s", s.Input)
	}
}

func TestAnExternalParentReachesTheWireWithoutParentingTheSpan(t *testing.T) {
	client, sink := newTestClient(t)
	ctx := context.Background()
	_, span := client.Start(ctx, "under_otel", WithExternalParent("d4d4d4d4d4d4d4d4"))
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	s := onlySpan(t, sink.spans(), "under_otel")
	if s.ExternalParentSpanID == nil || *s.ExternalParentSpanID != "d4d4d4d4d4d4d4d4" {
		t.Fatalf("external parent: %v", s.ExternalParentSpanID)
	}
	if s.ParentSpanID != nil || !s.IsRoot {
		t.Fatalf("the external parent must not parent the span: %v root=%v", s.ParentSpanID, s.IsRoot)
	}
}

func finishedSpan() Span {
	start := time.Date(2026, 1, 2, 3, 4, 5, 6, time.UTC)
	end := start.Add(time.Second)
	parent := "c3c3c3c3c3c3c3c3"
	return Span{
		TraceID:      leaseTrace,
		SpanID:       leaseSpan,
		ParentSpanID: &parent,
		Name:         "imported",
		Kind:         SpanKindToolCall,
		Status:       SpanStatusOk,
		StartTime:    start,
		EndTime:      &end,
		Input:        json.RawMessage(`"in"`),
		Output:       json.RawMessage(`"out"`),
		Correlation:  Correlation{SessionID: strPtr("s-9")},
	}
}

func TestRecordQueuesAFinishedSpanAsGiven(t *testing.T) {
	// A sample rate of zero would drop a span started here; a recorded span is
	// never re-sampled.
	client, sink := newTestClient(t, WithSampleRate(0), WithEnvironment("prod"))
	ctx := context.Background()
	given := finishedSpan()
	client.Record(given)
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	spans := sink.spans()
	if len(spans) != 1 {
		t.Fatalf("spans on the wire: %d", len(spans))
	}
	got := spans[0]
	if got.TraceID != given.TraceID || got.SpanID != given.SpanID || *got.ParentSpanID != *given.ParentSpanID {
		t.Fatalf("ids were rewritten: %+v", got)
	}
	if !got.StartTime.Equal(given.StartTime) || got.EndTime == nil || !got.EndTime.Equal(*given.EndTime) {
		t.Fatalf("times were rewritten: %v %v", got.StartTime, got.EndTime)
	}
	if string(got.Input) != `"in"` || string(got.Output) != `"out"` || got.Kind != SpanKindToolCall {
		t.Fatalf("the record changed: %+v", got)
	}
	if got.Correlation.Project != "test" || got.Correlation.Environment == nil || *got.Correlation.Environment != "prod" {
		t.Fatalf("empty correlation fields should be filled from the client: %+v", got.Correlation)
	}
	if got.Correlation.SessionID == nil || *got.Correlation.SessionID != "s-9" {
		t.Fatalf("a set correlation field should be kept: %+v", got.Correlation)
	}
	if stats := client.Stats(); stats.Rejected != 0 || stats.Exported != 1 {
		t.Fatalf("stats: %+v", stats)
	}
}

func TestRecordRefusesAMalformedSpanByCountingNotPanicking(t *testing.T) {
	client, sink := newTestClient(t)
	ctx := context.Background()
	cases := map[string]func(*Span){
		"uppercase trace id": func(s *Span) { s.TraceID = strings.ToUpper(s.TraceID) },
		"short trace id":     func(s *Span) { s.TraceID = s.TraceID[:31] },
		"zero trace id":      func(s *Span) { s.TraceID = strings.Repeat("0", 32) },
		"short span id":      func(s *Span) { s.SpanID = "b2b2" },
		"zero span id":       func(s *Span) { s.SpanID = strings.Repeat("0", 16) },
		"bad parent":         func(s *Span) { s.ParentSpanID = strPtr("not-hex-at-all!") },
		"empty name":         func(s *Span) { s.Name = "" },
		"no start time":      func(s *Span) { s.StartTime = time.Time{} },
	}
	n := uint64(0)
	for name, breakIt := range cases {
		span := finishedSpan()
		breakIt(&span)
		client.Record(span)
		n++
		if got := client.Stats().Rejected; got != n {
			t.Fatalf("%s: rejected %d, want %d", name, got, n)
		}
	}
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	if got := len(sink.spans()); got != 0 {
		t.Fatalf("%d malformed spans reached the wire", got)
	}

	disabled := New(WithEnabled(false), WithLogger(nil))
	disabled.Record(finishedSpan())
	if stats := disabled.Stats(); stats.Rejected != 0 || stats.Queued != 0 {
		t.Fatalf("a disabled client should drop quietly: %+v", stats)
	}
	var none *Client
	none.Record(finishedSpan())
}

func TestCaptureOffStripsARecordedSpanToo(t *testing.T) {
	client, sink := newTestClient(t, WithCapturePayloads(false))
	ctx := context.Background()
	client.Record(finishedSpan())
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	got := onlySpan(t, sink.spans(), "imported")
	if got.Input != nil || got.Output != nil {
		t.Fatalf("payloads leaked from a recorded span: %s %s", got.Input, got.Output)
	}
}

func TestARecordingSinkSeesExactlyWhatWasRecorded(t *testing.T) {
	sink := &RecordingSink{}
	client := New(WithSink(sink), WithProject("test"), WithFlushInterval(time.Hour), WithLogger(nil))
	t.Cleanup(func() { _ = client.Shutdown(context.Background()) })
	ctx := context.Background()

	ctx, span := client.Start(ctx, "run")
	client.Log(ctx, LogLevelInfo, "hello", nil)
	client.Metric(ctx, "tickets", 1, MetricKindCounter)
	client.Score(ctx, Target{}, "helpfulness", 0.5, 0, 1)
	client.Feedback(ctx, Target{}, FeedbackKindThumbs, "up")
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}

	if got := sink.Spans(); len(got) != 1 || got[0].Name != "run" || got[0].EndTime == nil {
		t.Fatalf("spans: %+v", got)
	}
	if got := sink.Logs(); len(got) != 1 || got[0].Message != "hello" {
		t.Fatalf("logs: %+v", got)
	}
	if got := sink.Metrics(); len(got) != 1 || got[0].Name != "tickets" {
		t.Fatalf("metrics: %+v", got)
	}
	if got := sink.Scores(); len(got) != 1 || got[0].ScorerID != "helpfulness" {
		t.Fatalf("scores: %+v", got)
	}
	if got := sink.Feedback(); len(got) != 1 || got[0].Kind != FeedbackKindThumbs {
		t.Fatalf("feedback: %+v", got)
	}
	if sink.Batches() != 1 {
		t.Fatalf("batches: %d", sink.Batches())
	}
	stats := client.Stats()
	if stats.Exported != 5 || stats.Failed != 0 || stats.Queued != 0 || stats.Capacity != DefaultMaxQueue {
		t.Fatalf("stats: %+v", stats)
	}
	sink.Clear()
	if len(sink.Spans()) != 0 || len(sink.Logs()) != 0 || sink.Batches() != 0 {
		t.Fatal("Clear should forget everything")
	}
}

// slowSink takes delay over every batch and ignores the context it is given:
// a daemon that accepts the connection and answers late.
type slowSink struct {
	calls atomic.Int64
	delay time.Duration
}

func (s *slowSink) Export(context.Context, IngestBatch, string) error {
	s.calls.Add(1)
	time.Sleep(s.delay)
	return nil
}

func TestFlushAndShutdownReturnByTheirDeadlineWhenTheSinkNeverAnswers(t *testing.T) {
	// Against a black-holed daemon every batch costs three attempts of the full
	// request timeout. A bound on batches is no bound on time.
	defer func(d time.Duration) { flushDeadline = d }(flushDeadline)
	flushDeadline = 50 * time.Millisecond
	sink := &slowSink{delay: 400 * time.Millisecond}
	client := New(WithSink(sink), WithProject("test"), WithBatchSize(1), WithFlushInterval(time.Hour), WithLogger(nil))
	ctx := context.Background()
	for i := 0; i < 5; i++ {
		client.Log(ctx, LogLevelInfo, "line", nil)
	}

	started := time.Now()
	// A context with no deadline of its own gets the default.
	if err := client.Flush(ctx); err == nil {
		t.Fatal("a flush that ran out of time should say so")
	}
	if err := client.Shutdown(ctx); err == nil {
		t.Fatal("a shutdown that ran out of time should say so")
	}
	if took := time.Since(started); took > 300*time.Millisecond {
		t.Fatalf("flush and shutdown took %v", took)
	}

	// The batch in flight finishes and is counted; no further batch starts.
	time.Sleep(time.Second)
	if got := sink.calls.Load(); got != 1 {
		t.Fatalf("%d batches reached the sink, want 1", got)
	}
	if stats := client.Stats(); stats.Exported != 1 || stats.Queued != 4 {
		t.Fatalf("stats: %+v", stats)
	}
}

type refusingSink struct{ calls int }

func (s *refusingSink) Export(context.Context, IngestBatch, string) error {
	s.calls++
	return errors.New("refused")
}

type panickingSink struct{}

func (panickingSink) Export(context.Context, IngestBatch, string) error { panic("the sink broke") }

func TestASinkThatFailsIsCountedAndNeverReachesTheRecordingCall(t *testing.T) {
	for name, sink := range map[string]Sink{"refuses": &refusingSink{}, "panics": panickingSink{}} {
		t.Run(name, func(t *testing.T) {
			client := New(WithSink(sink), WithProject("test"), WithFlushInterval(time.Hour), WithLogger(nil))
			t.Cleanup(func() { _ = client.Shutdown(context.Background()) })
			ctx := context.Background()
			for i := 0; i < 3; i++ {
				_, span := client.Start(ctx, "step")
				span.End()
			}
			// Flush is the explicit call and reports the failure; Start and End
			// above returned nothing and panicked on nothing.
			if err := client.Flush(ctx); err == nil {
				t.Fatal("flush should report the sink's failure")
			}
			stats := client.Stats()
			if stats.Failed != 1 || stats.Exported != 0 || stats.Queued != 0 {
				t.Fatalf("stats: %+v", stats)
			}
			// The worker is still alive after the failure.
			_, span := client.Start(ctx, "again")
			span.End()
			_ = client.Flush(ctx)
			if got := client.Stats().Failed; got != 2 {
				t.Fatalf("a second batch should fail on its own: %d", got)
			}
		})
	}
}

func TestAConsoleSinkWritesOneWireShapedLinePerSignal(t *testing.T) {
	var out bytes.Buffer
	client := New(WithSink(&ConsoleSink{Writer: &out}), WithProject("test"), WithFlushInterval(time.Hour), WithLogger(nil))
	t.Cleanup(func() { _ = client.Shutdown(context.Background()) })
	ctx := context.Background()
	ctx, span := client.Start(ctx, "run")
	client.Log(ctx, LogLevelInfo, "hello", nil)
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 2 {
		t.Fatalf("lines: %q", out.String())
	}
	var first map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &first); err != nil {
		t.Fatalf("line 0 is not JSON: %v", err)
	}
	if first["signal"] != "span" || first["trace_id"] != span.TraceID() || first["name"] != "run" {
		t.Fatalf("line 0: %v", first)
	}
	var second map[string]any
	if err := json.Unmarshal([]byte(lines[1]), &second); err != nil {
		t.Fatalf("line 1 is not JSON: %v", err)
	}
	if second["signal"] != "log" || second["message"] != "hello" {
		t.Fatalf("line 1: %v", second)
	}
}

func TestTheIngestKeyReachesTheSinkAndTheWire(t *testing.T) {
	var keys []string
	client := New(WithSink(sinkFunc(func(_ context.Context, batch IngestBatch, key string) error {
		keys = append(keys, key)
		if batch.IngestKey != nil {
			return errors.New("the exporter must not set the key on the batch; the transport does")
		}
		return nil
	})), WithProject("test"), WithFlushInterval(time.Hour), WithLogger(nil))
	t.Cleanup(func() { _ = client.Shutdown(context.Background()) })
	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	if len(keys) != 1 || !isTraceID(keys[0]) {
		t.Fatalf("keys: %v", keys)
	}

	http, collector := newTestClient(t)
	_, span = http.Start(ctx, "step")
	span.End()
	if err := http.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	if got := collector.ingestKeys(); len(got) != 1 || !isTraceID(got[0]) {
		t.Fatalf("wire keys: %v", got)
	}
}

type sinkFunc func(context.Context, IngestBatch, string) error

func (f sinkFunc) Export(ctx context.Context, batch IngestBatch, key string) error {
	return f(ctx, batch, key)
}
