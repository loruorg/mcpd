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
	"strings"
	"time"

	mcpd "github.com/mcpd/mcpd/sdks/go"
	"github.com/mcpd/mcpd/sdks/go/mcpdtest"
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

	// A streamed call, and the unit test that checks what it records.
	fmt.Println("streamed:", streamAnswer(ctx, m, "Is A-1042 on its way?"))
	if err := checkStreamAnswer(); err != nil {
		log.Fatalf("check the streamed call: %v", err)
	}

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
		mcpd.WithInput(map[string]any{
			"messages": []map[string]string{
				{"role": "system", "content": "You are a billing support agent."},
				{"role": "user", "content": question},
			},
		}),
	)
	defer span.End()

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

// chatChunk is the part of an OpenAI streamed chat-completion chunk this
// example reads.
type chatChunk struct {
	ID      string         `json:"id"`
	Model   string         `json:"model"`
	Choices []chatChoice   `json:"choices"`
	Usage   map[string]int `json:"usage,omitempty"`
}

type chatChoice struct {
	Index        int       `json:"index"`
	Delta        chatDelta `json:"delta"`
	FinishReason string    `json:"finish_reason,omitempty"`
}

type chatDelta struct {
	Content string `json:"content,omitempty"`
}

// fakeChatStream stands in for a provider's stream. It has the four methods of
// the stream the official OpenAI Go SDK returns.
type fakeChatStream struct {
	chunks []chatChunk
	at     int
}

func newFakeChatStream(text string) *fakeChatStream {
	s := &fakeChatStream{at: -1}
	chunk := func(choice chatChoice) chatChunk {
		return chatChunk{ID: "chatcmpl-01", Model: "gpt-4o-mini", Choices: []chatChoice{choice}}
	}
	for _, word := range strings.SplitAfter(text, " ") {
		s.chunks = append(s.chunks, chunk(chatChoice{Delta: chatDelta{Content: word}}))
	}
	s.chunks = append(s.chunks, chunk(chatChoice{FinishReason: "stop"}))
	usage := map[string]int{"prompt_tokens": 24, "completion_tokens": 6, "total_tokens": 30}
	s.chunks = append(s.chunks, chatChunk{ID: "chatcmpl-01", Model: "gpt-4o-mini", Usage: usage})
	return s
}

func (s *fakeChatStream) Next() bool {
	time.Sleep(5 * time.Millisecond)
	s.at++
	return s.at < len(s.chunks)
}
func (s *fakeChatStream) Current() chatChunk { return s.chunks[s.at] }
func (s *fakeChatStream) Err() error         { return nil }
func (s *fakeChatStream) Close() error       { return nil }

// #region stream
// streamAnswer is a streamed model call. Read the recorded stream in place of
// the provider's: every chunk comes through unchanged, and when the stream
// ends the span gets the time to first token, the text, the usage and the
// finish reason.
func streamAnswer(ctx context.Context, m *mcpd.Client, question string) string {
	_, span := m.Start(ctx, "stream_answer", mcpd.WithSpanKind(mcpd.SpanKindModelGeneration))
	span.SetInput([]map[string]string{{"role": "user", "content": question}})

	// Where client.Chat.Completions.NewStreaming(ctx, params) goes.
	provider := newFakeChatStream("Your order is in transit.")
	stream := mcpd.RecordStream(span, provider, "")
	// Close ends the span if the loop stops early. After a full read it
	// only closes the provider's stream.
	defer stream.Close()

	var answer strings.Builder
	for stream.Next() {
		if choices := stream.Current().Choices; len(choices) > 0 {
			answer.WriteString(choices[0].Delta.Content)
		}
	}
	if err := stream.Err(); err != nil {
		log.Printf("stream: %v", err)
	}
	return answer.String()
}

// #endregion stream

// #region testing
// checkStreamAnswer is a unit test. Pass the recorder's Client where your code
// takes an *mcpd.Client, then read back what it recorded: no daemon, no port,
// no flush. In a _test.go file, rec.MustSpan(t, name) fails the test for you.
func checkStreamAnswer() error {
	ctx := context.Background()
	rec := mcpdtest.New()
	defer rec.Client.Shutdown(ctx)
	answer := streamAnswer(ctx, rec.Client, "Where is my order A-1042?")

	call, err := rec.Span("stream_answer")
	if err != nil {
		return err
	}
	var output string
	_ = json.Unmarshal(call.Output, &output)
	if output != answer || call.Model == nil || *call.Model != "gpt-4o-mini" {
		return fmt.Errorf("recorded %q from %v", output, call.Model)
	}
	if call.Usage == nil || call.Usage.OutputTokens == nil || *call.Usage.OutputTokens != 6 {
		return fmt.Errorf("recorded usage %+v", call.Usage)
	}

	if trees := rec.Trees(); len(trees) != 1 || trees[0].Span.Name != "stream_answer" {
		return fmt.Errorf("expected one run named stream_answer, got %d", len(trees))
	}
	return nil
}

// #endregion testing

func duration(ns *uint64) time.Duration {
	if ns == nil {
		return 0
	}
	return time.Duration(*ns)
}
