// Command quickstart instruments a small fake support agent with mcpd: one
// agent run, one model generation with token usage, one tool call, an error,
// a score and a piece of human feedback, then reads the run back.
//
// Start a daemon and run it:
//
//	mcpd serve --data ./mcpd.db
//	go run ./examples/go
package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"time"

	mcpd "github.com/mcpd/mcpd/sdks/go"
)

func main() {
	ctx := context.Background()

	// #region init
	m := mcpd.New(
		mcpd.WithEndpoint("http://localhost:4317"),
		mcpd.WithProject("support"),
		mcpd.WithEnvironment("development"),
		mcpd.WithService("support-agent", "1.4.2"),
	)
	// Shutdown flushes what is still queued and stops the background worker.
	// A short-lived process that skips it loses whatever had not been sent.
	defer m.Shutdown(ctx)
	// #endregion init

	traceID := handleTicket(ctx, m, "My invoice for March charged me twice.")

	// #region score
	// A machine evaluation of the run. min and max are required: 0.86 means
	// nothing until you know the scale it sits on.
	m.Score(ctx, mcpd.TraceTarget(traceID), "answer_relevance", 0.86, 0, 1,
		mcpd.ScoreReason("Answered the billing question and cited the invoice."),
		mcpd.ScoreVersion("2"),
	)

	// A person's judgement of the same run. Wire this to your product's
	// thumbs-up control and it lands on the exact trace that produced the
	// answer.
	m.Feedback(ctx, mcpd.TraceTarget(traceID), mcpd.FeedbackKindThumbs, "up",
		mcpd.FeedbackComment("Fixed it first time."),
		mcpd.FeedbackAuthor("u-317", "end_user"),
	)
	// #endregion score

	// #region query
	// Recording is asynchronous, so wait for the batch to land before reading
	// it back. Outside an example you would never need this.
	if err := m.Flush(ctx); err != nil {
		log.Printf("flush: %v", err)
	}

	detail, err := m.GetTrace(ctx, traceID, "scores", "feedback")
	if err != nil {
		log.Fatalf("read the run back: %v", err)
	}
	fmt.Printf("%s: %s in %s, %d spans\n",
		detail.Trace.TraceID, detail.Trace.Status, duration(detail.Trace.DurationNs), len(detail.Spans))
	for _, span := range detail.Spans {
		fmt.Printf("  %-18s %-18s %s\n", span.Kind, span.Name, span.Status)
	}

	summary, err := m.AnalyticsSummary(ctx, time.Now().Add(-time.Hour), time.Now(), "model")
	if err != nil {
		log.Fatalf("read the summary: %v", err)
	}
	fmt.Printf("last hour: %d runs, %.1f%% failed, %.4f %s\n",
		summary.TraceCount, summary.ErrorRate*100, summary.TotalCost, summary.Currency)
	// #endregion query
}

// #region trace
// handleTicket is the agent. It drafts an answer with a model, tries to send
// it with a tool, and fails on the send so the error path is recorded too.
func handleTicket(ctx context.Context, m *mcpd.Client, question string) string {
	ctx, run := m.Start(ctx, "support_agent",
		mcpd.WithSpanKind(mcpd.SpanKindAgentRun),
		mcpd.WithEntity(mcpd.EntityKindAgent, "support-agent", "Support agent"),
		mcpd.WithSession("sess-4821"),
		mcpd.WithUser("u-317"),
		mcpd.WithTags("support", "billing"),
		mcpd.WithInput(question),
	)
	defer run.End()

	m.Log(ctx, mcpd.LogLevelInfo, "ticket received", map[string]any{"channel": "email"})

	answer := draftAnswer(ctx, m, question)
	run.SetOutput(answer)

	if err := sendReply(ctx, m, answer); err != nil {
		// The span is now an error with a message, which is what makes it
		// worth reading. End below records the failed status.
		run.RecordError(err, mcpd.ErrorDomain("email"), mcpd.ErrorRetryable(true))
	}
	return run.TraceID()
}
// #endregion trace

// draftAnswer stands in for a model call.
func draftAnswer(ctx context.Context, m *mcpd.Client, question string) string {
	ctx, span := m.Start(ctx, "draft_answer",
		mcpd.WithSpanKind(mcpd.SpanKindModelGeneration),
		mcpd.WithModel("gpt-4o-mini", "openai"),
		mcpd.WithInput(map[string]any{
			"messages": []map[string]string{
				{"role": "system", "content": "You are a billing support agent."},
				{"role": "user", "content": question},
			},
		}),
	)
	defer span.End()

	answer := "You were charged twice on 3 March. The duplicate is refunded today."
	time.Sleep(40 * time.Millisecond) // the provider round trip

	span.SetOutput(answer)
	// Token counts are what mcpd prices the span from, at ingest.
	span.SetUsage(mcpd.TokenUsage{
		InputTokens:  ptr(uint64(812)),
		OutputTokens: ptr(uint64(96)),
		InputDetails: &mcpd.InputTokenDetails{CacheRead: ptr(uint64(640))},
	})
	m.Metric(ctx, "support.answer_tokens", 96, mcpd.MetricKindHistogram,
		mcpd.MetricUnit("{token}"),
		mcpd.MetricLabels(map[string]any{"model": "gpt-4o-mini"}),
	)
	return answer
}

// sendReply stands in for a tool the agent calls, and fails.
func sendReply(ctx context.Context, m *mcpd.Client, answer string) error {
	_, span := m.Start(ctx, "send_email",
		mcpd.WithSpanKind(mcpd.SpanKindToolCall),
		mcpd.WithEntity(mcpd.EntityKindTool, "send_email", "Send email"),
		mcpd.WithInput(map[string]any{"to": "customer@example.com", "body": answer}),
	)
	defer span.End()

	time.Sleep(15 * time.Millisecond)
	err := errors.New("smtp: 451 mailbox busy, try again later")
	span.RecordError(err, mcpd.ErrorKind("SMTPTemporaryFailure"), mcpd.ErrorRetryable(true))
	return err
}


func duration(ns *uint64) time.Duration {
	if ns == nil {
		return 0
	}
	return time.Duration(*ns)
}

func ptr[T any](v T) *T { return &v }
