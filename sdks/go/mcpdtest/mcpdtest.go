// Package mcpdtest is a client for your own unit tests (SDK contract 2.5).
//
//	rec := mcpdtest.New()
//	answerQuestion(ctx, rec.Client)
//	call := rec.MustSpan(t, "model_call")
//	if got := *call.Usage.OutputTokens; got != 37 {
//		t.Errorf("output tokens = %d", got)
//	}
//
// Its Client is a real *mcpd.Client whose sink keeps every batch in memory, so
// what you read back went through sampling, payload capture, redaction and the
// queue exactly as it would on the way to a daemon. Nothing leaves the
// process.
package mcpdtest

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"testing"

	mcpd "github.com/mcpd/mcpd/sdks/go"
)

// Recorder holds a client that records into memory. Pass its Client wherever
// your code takes one, then read back what it recorded. Every accessor flushes
// first, so a test never calls Flush itself.
type Recorder struct {
	// Client is the client to hand to the code under test.
	Client *mcpd.Client
	// Sink is the sink underneath, for a test that wants the raw batches.
	Sink *mcpd.RecordingSink
}

// SpanFilter narrows Spans. A zero field matches every span.
type SpanFilter struct {
	Name string
	Kind mcpd.SpanKind
}

// SpanTree is one finished span and the finished spans under it, ordered by
// start time.
type SpanTree struct {
	Span     mcpd.Span
	Children []*SpanTree
}

// New returns a recorder. It takes the options mcpd.New takes; a WithSink
// among them is replaced, because recording into memory is the point.
// Recording is on and every trace is kept unless you pass WithEnabled or
// WithSampleRate yourself, whatever MCPD_ENABLED and MCPD_SAMPLE say, so a CI
// job that turns mcpd off does not turn your tests off with it. Every other
// option takes the client's own default.
func New(options ...mcpd.Option) *Recorder {
	sink := &mcpd.RecordingSink{}
	opts := make([]mcpd.Option, 0, len(options)+3)
	opts = append(opts, mcpd.WithEnabled(true), mcpd.WithSampleRate(1))
	opts = append(opts, options...)
	opts = append(opts, mcpd.WithSink(sink))
	return &Recorder{Client: mcpd.New(opts...), Sink: sink}
}

// flush hands everything queued to the sink. The client's own default
// deadline bounds it, so a test cannot hang here.
func (r *Recorder) flush() {
	_ = r.Client.Flush(context.Background())
}

// Spans returns the finished spans, one per span id, in the order each id
// first arrived. A span is recorded when it starts and again when it ends, so
// the last record for each id is kept, and a span that has not ended is left
// out. A span must match every filter given.
func (r *Recorder) Spans(filters ...SpanFilter) []mcpd.Span {
	r.flush()
	out := []mcpd.Span{}
	for _, s := range finished(r.Sink.Spans()) {
		if matches(s, filters) {
			out = append(out, s)
		}
	}
	return out
}

func matches(s mcpd.Span, filters []SpanFilter) bool {
	for _, f := range filters {
		if (f.Name != "" && s.Name != f.Name) || (f.Kind != "" && s.Kind != f.Kind) {
			return false
		}
	}
	return true
}

// Span returns the one finished span called name. The error gives the count,
// and the names that were recorded, when there is none or more than one.
func (r *Recorder) Span(name string) (mcpd.Span, error) {
	all := r.Spans()
	var found []mcpd.Span
	var seen []string
	known := map[string]bool{}
	for _, s := range all {
		if s.Name == name {
			found = append(found, s)
		}
		if !known[s.Name] {
			known[s.Name] = true
			seen = append(seen, s.Name)
		}
	}
	if len(found) != 1 {
		names := "none"
		if len(seen) > 0 {
			names = strings.Join(seen, ", ")
		}
		return mcpd.Span{}, fmt.Errorf("expected one finished span named %q, found %d; spans recorded: %s",
			name, len(found), names)
	}
	return found[0], nil
}

// MustSpan is Span for a test: it fails the test at once when there is not
// exactly one finished span called name.
func (r *Recorder) MustSpan(t testing.TB, name string) mcpd.Span {
	t.Helper()
	s, err := r.Span(name)
	if err != nil {
		t.Fatal(err)
	}
	return s
}

// Logs returns every log record, in arrival order.
func (r *Recorder) Logs() []mcpd.LogRecord {
	r.flush()
	return r.Sink.Logs()
}

// Metrics returns every metric point, in arrival order.
func (r *Recorder) Metrics() []mcpd.MetricPoint {
	r.flush()
	return r.Sink.Metrics()
}

// Scores returns every score, in arrival order.
func (r *Recorder) Scores() []mcpd.Score {
	r.flush()
	return r.Sink.Scores()
}

// RecordedFeedback returns every piece of feedback, in arrival order. It is
// not called Feedback because the client already has a Feedback that records
// one.
func (r *Recorder) RecordedFeedback() []mcpd.Feedback {
	r.flush()
	return r.Sink.Feedback()
}

// Trees returns every trace as a tree of finished spans, roots and children
// each ordered by start time. A span whose parent was not recorded is a root.
func (r *Recorder) Trees() []*SpanTree {
	spans := r.Spans()
	nodes := make(map[string]*SpanTree, len(spans))
	order := make([]*SpanTree, 0, len(spans))
	for _, s := range spans {
		node := &SpanTree{Span: s}
		nodes[s.SpanID] = node
		order = append(order, node)
	}
	roots := []*SpanTree{}
	for _, node := range order {
		var parent *SpanTree
		if p := node.Span.ParentSpanID; p != nil {
			parent = nodes[*p]
		}
		if parent != nil && parent != node {
			parent.Children = append(parent.Children, node)
		} else {
			roots = append(roots, node)
		}
	}
	byStart(roots)
	return roots
}

func byStart(list []*SpanTree) {
	sort.SliceStable(list, func(i, j int) bool {
		return list[i].Span.StartTime.Before(list[j].Span.StartTime)
	})
	for _, n := range list {
		byStart(n.Children)
	}
}

// Reset flushes and forgets everything recorded so far.
func (r *Recorder) Reset() {
	r.flush()
	r.Sink.Clear()
}

// finished keeps the last record per span id, in first-arrival order, and
// only the spans that ended.
func finished(records []mcpd.Span) []mcpd.Span {
	last := make(map[string]int, len(records))
	var out []mcpd.Span
	for _, s := range records {
		if i, ok := last[s.SpanID]; ok {
			out[i] = s
			continue
		}
		last[s.SpanID] = len(out)
		out = append(out, s)
	}
	ended := out[:0]
	for _, s := range out {
		if s.EndTime != nil {
			ended = append(ended, s)
		}
	}
	return ended
}
