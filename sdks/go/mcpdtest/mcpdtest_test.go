package mcpdtest_test

import (
	"context"
	"encoding/json"
	"reflect"
	"regexp"
	"strings"
	"testing"

	mcpd "github.com/mcpd/mcpd/sdks/go"
	"github.com/mcpd/mcpd/sdks/go/mcpdtest"
)

// The tests in this file cover section 2.5 of sdks/SDK-CONTRACT.md: the test
// client.

func newRecorder(t *testing.T, opts ...mcpd.Option) *mcpdtest.Recorder {
	t.Helper()
	rec := mcpdtest.New(append([]mcpd.Option{mcpd.WithLogger(nil)}, opts...)...)
	t.Cleanup(func() { rec.Client.Shutdown(context.Background()) })
	return rec
}

func answer(ctx context.Context, m *mcpd.Client) {
	ctx, run := m.Start(ctx, "answer")
	defer run.End()
	_, lookup := m.Start(ctx, "lookup", mcpd.WithSpanKind(mcpd.SpanKindToolCall))
	lookup.End()
	_, gen := m.Start(ctx, "generate", mcpd.WithSpanKind(mcpd.SpanKindModelGeneration))
	gen.SetOutput("it ships tomorrow")
	gen.End()
	m.Log(ctx, mcpd.LogLevelInfo, "answered", map[string]any{"order": "A-1"})
	m.Metric(ctx, "answers", 1, mcpd.MetricKindCounter)
	m.Score(ctx, mcpd.Target{}, "relevance", 0.9, 0, 1)
	m.Feedback(ctx, mcpd.Target{}, mcpd.FeedbackKindThumbs, 1)
	run.SetOutput("done")
}

func names(spans []mcpd.Span) []string {
	out := []string{}
	for _, s := range spans {
		out = append(out, s.Name)
	}
	return out
}

func TestReadsBackEverySignalWithNoFlush(t *testing.T) {
	rec := newRecorder(t)
	answer(context.Background(), rec.Client)

	if got, want := names(rec.Spans()), []string{"answer", "lookup", "generate"}; !reflect.DeepEqual(got, want) {
		t.Errorf("spans = %v, want %v", got, want)
	}
	if logs := rec.Logs(); len(logs) != 1 || logs[0].Message != "answered" {
		t.Errorf("logs = %+v", logs)
	}
	if metrics := rec.Metrics(); len(metrics) != 1 || metrics[0].Name != "answers" {
		t.Errorf("metrics = %+v", metrics)
	}
	if scores := rec.Scores(); len(scores) != 1 || scores[0].ScorerID != "relevance" {
		t.Errorf("scores = %+v", scores)
	}
	if fb := rec.RecordedFeedback(); len(fb) != 1 || fb[0].Kind != mcpd.FeedbackKindThumbs {
		t.Errorf("feedback = %+v", fb)
	}
}

func TestOneRecordPerSpanIDFinishedOnly(t *testing.T) {
	rec := newRecorder(t)
	ctx := context.Background()
	_, open := rec.Client.Start(ctx, "still_running")
	answer(ctx, rec.Client)

	spans := rec.Spans()
	if raw := rec.Sink.Spans(); len(raw) <= len(spans) {
		t.Errorf("the sink saw %d records for %d spans; expected a start record as well", len(raw), len(spans))
	}
	ids := map[string]bool{}
	for _, s := range spans {
		if ids[s.SpanID] {
			t.Errorf("span id %s appears twice", s.SpanID)
		}
		ids[s.SpanID] = true
		if s.Name == "still_running" {
			t.Error("a span that has not ended was returned")
		}
	}
	open.End()
	got := names(rec.Spans())
	if len(got) != 4 || got[0] != "still_running" {
		t.Errorf("after End, spans = %v; want still_running first, in arrival order", got)
	}
}

func TestFiltersByNameAndKind(t *testing.T) {
	rec := newRecorder(t)
	answer(context.Background(), rec.Client)
	if got := names(rec.Spans(mcpdtest.SpanFilter{Kind: mcpd.SpanKindToolCall})); !reflect.DeepEqual(got, []string{"lookup"}) {
		t.Errorf("by kind = %v", got)
	}
	gen := rec.Spans(mcpdtest.SpanFilter{Name: "generate"})
	if len(gen) != 1 || string(gen[0].Output) != `"it ships tomorrow"` {
		t.Errorf("by name = %+v", gen)
	}
	if got := rec.Spans(mcpdtest.SpanFilter{Name: "generate", Kind: mcpd.SpanKindToolCall}); len(got) != 0 {
		t.Errorf("name and kind together = %v", names(got))
	}
}

func TestSpanFailsOnNoneAndOnTwo(t *testing.T) {
	rec := newRecorder(t)
	ctx := context.Background()
	answer(ctx, rec.Client)
	if _, err := rec.Span("lookup"); err != nil {
		t.Fatalf("one lookup: %v", err)
	}
	_, err := rec.Span("missing")
	if err == nil || !strings.Contains(err.Error(), "found 0") ||
		!strings.Contains(err.Error(), "answer, lookup, generate") {
		t.Errorf("none: err = %v", err)
	}
	answer(ctx, rec.Client)
	if _, err := rec.Span("lookup"); err == nil || !regexp.MustCompile(`found 2\b`).MatchString(err.Error()) {
		t.Errorf("two: err = %v", err)
	}
}

func TestMustSpanFailsTheTest(t *testing.T) {
	rec := newRecorder(t)
	answer(context.Background(), rec.Client)
	if s := rec.MustSpan(t, "generate"); s.Name != "generate" {
		t.Errorf("MustSpan = %q", s.Name)
	}
	fake := &fakeTB{TB: t}
	func() {
		defer func() { _ = recover() }()
		rec.MustSpan(fake, "missing")
	}()
	if !fake.fatal {
		t.Error("MustSpan did not fail the test on a missing span")
	}
}

// fakeTB records a Fatal instead of stopping the real test.
type fakeTB struct {
	testing.TB
	fatal bool
}

func (f *fakeTB) Helper() {}
func (f *fakeTB) Fatal(args ...any) {
	f.fatal = true
	panic("fatal")
}

func TestTrees(t *testing.T) {
	rec := newRecorder(t)
	answer(context.Background(), rec.Client)
	trees := rec.Trees()
	if len(trees) != 1 {
		t.Fatalf("roots = %d, want 1", len(trees))
	}
	root := trees[0]
	if root.Span.Name != "answer" {
		t.Errorf("root = %q", root.Span.Name)
	}
	var children []string
	for _, c := range root.Children {
		children = append(children, c.Span.Name)
	}
	if !reflect.DeepEqual(children, []string{"lookup", "generate"}) {
		t.Errorf("children = %v", children)
	}
}

func TestTreeTreatsAnOrphanAsARoot(t *testing.T) {
	rec := newRecorder(t)
	parent := mcpd.TraceContext{TraceID: strings.Repeat("a", 32), SpanID: strings.Repeat("b", 16), Sampled: true}
	_, orphan := rec.Client.Start(context.Background(), "orphan", mcpd.WithParent(parent))
	orphan.End()
	trees := rec.Trees()
	if len(trees) != 1 || trees[0].Span.Name != "orphan" {
		t.Errorf("trees = %+v", trees)
	}
}

func TestResetForgetsEverything(t *testing.T) {
	rec := newRecorder(t)
	answer(context.Background(), rec.Client)
	rec.Reset()
	if n := len(rec.Spans()) + len(rec.Logs()) + len(rec.Scores()) + len(rec.RecordedFeedback()); n != 0 {
		t.Errorf("%d signals survived Reset", n)
	}
}

func TestRedactApplied(t *testing.T) {
	rec := newRecorder(t, mcpd.WithRedact(func(v any) (any, error) {
		if s, ok := v.(string); ok {
			return strings.Replace(s, "tomorrow", "[date]", 1), nil
		}
		return v, nil
	}))
	answer(context.Background(), rec.Client)
	var out string
	if err := json.Unmarshal(rec.MustSpan(t, "generate").Output, &out); err != nil || out != "it ships [date]" {
		t.Errorf("output = %q (%v)", out, err)
	}
}

func TestCapturePayloadsOptionHonoured(t *testing.T) {
	rec := newRecorder(t, mcpd.WithCapturePayloads(false))
	answer(context.Background(), rec.Client)
	if out := rec.MustSpan(t, "generate").Output; len(out) != 0 {
		t.Errorf("output = %s, want none with payload capture off", out)
	}
}

func TestStaysOnWhenTheEnvironmentTurnsMcpdOff(t *testing.T) {
	t.Setenv("MCPD_ENABLED", "false")
	t.Setenv("MCPD_SAMPLE", "0")
	rec := newRecorder(t)
	answer(context.Background(), rec.Client)
	if got := len(rec.Spans()); got != 3 {
		t.Errorf("spans = %d, want 3", got)
	}
}

func TestExplicitOptionsStillWin(t *testing.T) {
	rec := newRecorder(t, mcpd.WithEnabled(false))
	answer(context.Background(), rec.Client)
	if got := len(rec.Spans()); got != 0 {
		t.Errorf("spans = %d with WithEnabled(false), want 0", got)
	}
}

func TestReplacesACallersSink(t *testing.T) {
	other := &mcpd.RecordingSink{}
	rec := newRecorder(t, mcpd.WithSink(other))
	answer(context.Background(), rec.Client)
	if len(rec.Spans()) != 3 || len(other.Spans()) != 0 {
		t.Error("a sink passed in should be replaced by the recorder's own")
	}
}
