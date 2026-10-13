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
	"encoding/json"
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

	// Store the prompt the agent reads and label it production. Posting the
	// same body again stores nothing, so this can run on every start.
	if _, err := m.CreatePrompt(ctx, mcpd.CreatePromptRequest{
		Name: "support/answer",
		Messages: json.RawMessage(`[
			{"role": "system", "content": "You answer billing questions for {{company}}."},
			{"role": "user", "content": "{{question}}"}
		]`),
		Labels: []string{"production"},
	}); err != nil {
		log.Printf("store the prompt: %v", err)
	}

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
	// #region prompt
	// The version labelled production, cached for a minute. If the daemon is
	// down before the first fetch, the fallback keeps the agent working.
	prompt, err := m.GetPrompt(ctx, "support/answer", mcpd.PromptOptions{
		Label: "production",
		FallbackMessages: []map[string]any{
			{"role": "system", "content": "You answer billing questions for {{company}}."},
			{"role": "user", "content": "{{question}}"},
		},
	})
	if err != nil {
		log.Fatalf("read the prompt: %v", err)
	}
	rendered, err := mcpd.RenderPrompt(prompt, map[string]string{
		"company": "Acme", "question": question,
	})
	if err != nil {
		log.Fatalf("render the prompt: %v", err)
	}

	ctx, span := m.Start(ctx, "draft_answer",
		mcpd.WithSpanKind(mcpd.SpanKindModelGeneration),
		mcpd.WithInput(map[string]any{"messages": rendered.Messages}),
	)
	defer span.End()
	// The run now shows which prompt version wrote the answer.
	span.SetPrompt(prompt)
	// #endregion prompt

	// Where a real provider call goes. This is the shape Anthropic answers in.
	time.Sleep(40 * time.Millisecond)
	span.MarkFirstToken()
	answer := "You were charged twice on 3 March. The duplicate is refunded today."
	response := map[string]any{
		"id":          "msg_01",
		"type":        "message",
		"model":       "claude-sonnet-4-5",
		"stop_reason": "end_turn",
		"content":     []map[string]any{{"type": "text", "text": answer}},
		"usage": map[string]any{
			"input_tokens": 172, "cache_read_input_tokens": 640, "output_tokens": 96,
		},
	}

	span.SetOutput(answer)
	// The model, the token counts mcpd prices, the response id and the
	// finish reason, read from the response in one call.
	span.RecordResponse(response, "")
	m.Metric(ctx, "support.answer_tokens", 96, mcpd.MetricKindHistogram,
		mcpd.MetricUnit("{token}"),
		mcpd.MetricLabels(map[string]any{"model": "claude-sonnet-4-5"}),
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
