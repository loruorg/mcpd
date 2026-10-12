// The canonical run, in Go. Prints its trace id on the last line.
package main

import (
	"context"
	"errors"
	"fmt"
	"os"

	mcpd "github.com/mcpd/mcpd/sdks/go"
)

func main() {
	ctx := context.Background()
	m := mcpd.New(
		mcpd.WithEndpoint(os.Getenv("MCPD_ENDPOINT")),
		mcpd.WithProject(os.Getenv("MCPD_PROJECT")),
	)

	ctx, run := m.Start(ctx, "support_agent",
		mcpd.WithSpanKind(mcpd.SpanKindAgentRun),
		mcpd.WithInput("where is my order 41?"),
		mcpd.WithSession("sess-conformance"),
		mcpd.WithThread("thread-7"),
		mcpd.WithUser("user-41"),
		mcpd.WithRequestID("req-9001"),
		mcpd.WithRelease("2026.08.1"),
	)

	_, gen := m.Start(ctx, "plan_answer", mcpd.WithSpanKind(mcpd.SpanKindModelGeneration))
	gen.SetModel("gpt-4o", "openai")
	gen.SetTokens(1180, 96)
	gen.End()

	_, tool := m.Start(ctx, "lookup_order", mcpd.WithSpanKind(mcpd.SpanKindToolCall))
	tool.End()

	_, failing := m.Start(ctx, "estimate_delivery", mcpd.WithSpanKind(mcpd.SpanKindToolCall))
	failing.RecordError(errors.New("shipping service returned 503"))
	failing.End()

	// Both carry the run's context, so the log lands on the trace and the
	// metric point keeps an exemplar back to it.
	m.Log(ctx, mcpd.LogLevelWarn, "estimate_delivery failed, answering from the cached estimate",
		map[string]any{"step": "estimate_delivery"})
	m.Metric(ctx, "conformance.run_steps", 3, mcpd.MetricKindCounter,
		mcpd.MetricUnit("{step}"),
		mcpd.MetricLabels(map[string]any{"outcome": "degraded", "lang": "go"}))

	traceID := run.TraceID()
	run.End()

	m.Score(ctx, mcpd.TraceTarget(traceID), "answer_relevance", 0.86, 0, 1,
		mcpd.ScoreFrom(mcpd.ScoreSourceLive))
	// No source: the suite compares what each SDK defaults to.
	m.Score(ctx, mcpd.TraceTarget(traceID), "scale_check", 3, 1, 5)
	m.Feedback(ctx, mcpd.TraceTarget(traceID), mcpd.FeedbackKindThumbs, "up")
	_ = m.Shutdown(ctx)
	fmt.Println(traceID)
}
