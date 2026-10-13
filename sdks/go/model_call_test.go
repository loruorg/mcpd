package mcpd

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"regexp"
	"strings"
	"sync"
	"testing"
	"time"
)

// The tests in this file cover sections 2.1.1 and 3.1 of sdks/SDK-CONTRACT.md:
// redaction in the process, and recording a model call from its response.

const openaiChatJSON = `{
  "id": "chatcmpl-1",
  "object": "chat.completion",
  "model": "gpt-4o-2024-08-06",
  "service_tier": "flex",
  "choices": [{"index": 0, "finish_reason": "stop", "message": {"role": "assistant", "content": "hi"}}],
  "usage": {
    "prompt_tokens": 1000,
    "completion_tokens": 200,
    "total_tokens": 1200,
    "prompt_tokens_details": {"cached_tokens": 600, "audio_tokens": 0},
    "completion_tokens_details": {"reasoning_tokens": 50}
  }
}`

var anthropicMessage = map[string]any{
	"id":          "msg_1",
	"type":        "message",
	"model":       "claude-sonnet-4-5",
	"stop_reason": "end_turn",
	"usage": map[string]any{
		"input_tokens":                100,
		"cache_read_input_tokens":     900,
		"cache_creation_input_tokens": 50,
		"cache_creation": map[string]any{
			"ephemeral_5m_input_tokens": 20,
			"ephemeral_1h_input_tokens": 30,
		},
		"output_tokens": 40,
		"service_tier":  "batch",
	},
}

func newSinkClient(t *testing.T, opts ...Option) (*Client, *RecordingSink) {
	t.Helper()
	sink := &RecordingSink{}
	base := []Option{
		WithProject("test"),
		WithEnabled(true),
		WithFlushInterval(time.Hour),
		WithLogger(nil),
		WithSink(sink),
	}
	client := New(append(base, opts...)...)
	t.Cleanup(func() { client.Shutdown(context.Background()) })
	return client, sink
}

func usageJSON(t *testing.T, u *TokenUsage) string {
	t.Helper()
	raw, err := json.Marshal(u)
	if err != nil {
		t.Fatal(err)
	}
	return string(raw)
}

func attrsOf(t *testing.T, s Span) map[string]any {
	t.Helper()
	var out map[string]any
	if len(s.Attributes) > 0 {
		if err := json.Unmarshal(s.Attributes, &out); err != nil {
			t.Fatal(err)
		}
	}
	return out
}

func TestOpenAIUsageSplitsTheGrossPromptIntoTextAndCache(t *testing.T) {
	want := `{"input_tokens":1000,"output_tokens":200,"total_tokens":1200,` +
		`"input_details":{"text":400,"cache_read":600,"audio":0},"output_details":{"reasoning":50}}`
	if got := usageJSON(t, UsageFromOpenAI(json.RawMessage(openaiChatJSON))); got != want {
		t.Fatalf("chat completions:\n got %s\nwant %s", got, want)
	}

	// A typed struct, as a provider SDK would hand over, is read by its JSON
	// field names.
	type details struct {
		CachedTokens int `json:"cached_tokens"`
	}
	type usage struct {
		InputTokens        int     `json:"input_tokens"`
		OutputTokens       int     `json:"output_tokens"`
		InputTokensDetails details `json:"input_tokens_details"`
	}
	responses := struct {
		Object string `json:"object"`
		Usage  usage  `json:"usage"`
	}{"response", usage{10, 5, details{4}}}
	want = `{"input_tokens":10,"output_tokens":5,"total_tokens":15,"input_details":{"text":6,"cache_read":4}}`
	if got := usageJSON(t, UsageFromOpenAI(&responses)); got != want {
		t.Fatalf("responses API:\n got %s\nwant %s", got, want)
	}
}

func TestAnthropicUsageIsNetOfTheCache(t *testing.T) {
	want := `{"input_tokens":1050,"output_tokens":40,"total_tokens":1090,"input_details":` +
		`{"text":100,"cache_read":900,"cache_write":50,"cache_write_5m":20,"cache_write_1h":30}}`
	if got := usageJSON(t, UsageFromAnthropic(anthropicMessage)); got != want {
		t.Fatalf("\n got %s\nwant %s", got, want)
	}
}

type panickyResponse struct{}

func (panickyResponse) MarshalJSON() ([]byte, error) { panic("provider object broke") }

func TestAnUnknownShapeReadsNothingAndNeverPanics(t *testing.T) {
	if UsageFromOpenAI(nil) != nil {
		t.Fatal("nil response gave usage")
	}
	if UsageFromAnthropic(map[string]any{"usage": map[string]any{"input_tokens": "many"}}) != nil {
		t.Fatal("a string token count was read")
	}
	if UsageFromOpenAI(panickyResponse{}) != nil || UsageFromOpenAI([]byte("not json")) != nil {
		t.Fatal("an unreadable response gave usage")
	}
	client, sink := newSinkClient(t)
	_, span := client.Start(context.Background(), "run")
	span.RecordResponse(42, "")
	span.RecordResponse(map[string]any{"usage": "x"}, "")
	span.RecordResponse(panickyResponse{}, "")
	var nilSpan *ActiveSpan
	nilSpan.RecordResponse(anthropicMessage, "")
	nilSpan.MarkFirstToken()
	nilSpan.SetGuardrail(true, "x")
	span.End()
	if err := client.Flush(context.Background()); err != nil {
		t.Fatal(err)
	}
	rec := onlySpan(t, sink.Spans(), "run")
	if rec.Usage != nil || rec.Model != nil || len(rec.Attributes) != 0 {
		t.Fatalf("an unknown shape recorded something: %+v", rec)
	}
}

func TestRecordResponseSetsModelUsageAndAttributesAndNeverTheText(t *testing.T) {
	client, sink := newSinkClient(t)
	ctx := context.Background()
	_, a := client.Start(ctx, "a", WithSpanKind(SpanKindModelGeneration), WithAttributes(map[string]any{"mine": 1}))
	a.RecordResponse(json.RawMessage(openaiChatJSON), "")
	a.End()
	_, b := client.Start(ctx, "b", WithSpanKind(SpanKindModelGeneration))
	b.RecordResponse(anthropicMessage, "aws.bedrock")
	b.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	ra, rb := onlySpan(t, sink.Spans(), "a"), onlySpan(t, sink.Spans(), "b")
	if ra.Model == nil || *ra.Model != "gpt-4o-2024-08-06" || ra.Provider == nil || *ra.Provider != "openai" {
		t.Fatalf("model and provider: %v %v", ra.Model, ra.Provider)
	}
	if ra.Output != nil || ra.Input != nil {
		t.Fatalf("the text was recorded: %s %s", ra.Input, ra.Output)
	}
	want := map[string]any{
		"mine":                           float64(1),
		"gen_ai.response.id":             "chatcmpl-1",
		"gen_ai.response.finish_reasons": []any{"stop"},
		"gen_ai.response.service_tier":   "flex",
	}
	if got := attrsOf(t, ra); !reflect.DeepEqual(got, want) {
		t.Fatalf("attributes:\n got %v\nwant %v", got, want)
	}
	if ra.Usage == nil || *ra.Usage.InputTokens != 1000 {
		t.Fatalf("usage: %+v", ra.Usage)
	}
	if rb.Provider == nil || *rb.Provider != "aws.bedrock" {
		t.Fatalf("provider override: %v", rb.Provider)
	}
	if got := attrsOf(t, rb)["gen_ai.response.service_tier"]; got != "batch" {
		t.Fatalf("anthropic tier: %v", got)
	}
	if got := attrsOf(t, rb)["gen_ai.response.finish_reasons"]; !reflect.DeepEqual(got, []any{"end_turn"}) {
		t.Fatalf("anthropic stop reason: %v", got)
	}
}

func TestTheFirstTokenCountsOnceAndAGuardrailRecordsItsOutcome(t *testing.T) {
	client, sink := newSinkClient(t)
	_, s := client.Start(context.Background(), "stream")
	s.MarkFirstToken()
	time.Sleep(5 * time.Millisecond)
	s.MarkFirstToken()
	s.SetGuardrail(true, "pii in output")
	s.End()
	if err := client.Flush(context.Background()); err != nil {
		t.Fatal(err)
	}
	attrs := attrsOf(t, onlySpan(t, sink.Spans(), "stream"))
	ms, ok := attrs["mcpd.time_to_first_token_ms"].(float64)
	if !ok || ms >= 5 {
		t.Fatalf("time to first token: %v", attrs["mcpd.time_to_first_token_ms"])
	}
	if attrs["mcpd.guardrail.tripped"] != true || attrs["mcpd.guardrail.reason"] != "pii in output" {
		t.Fatalf("guardrail: %v", attrs)
	}
}

func TestRedactRewritesPayloadsAndFailsClosed(t *testing.T) {
	card := regexp.MustCompile(`\d{4}-\d{4}`)
	client, sink := newSinkClient(t, WithRedact(func(v any) (any, error) {
		if s, ok := v.(string); ok {
			switch {
			case strings.Contains(s, "boom"):
				return nil, errors.New("redactor broke")
			case strings.Contains(s, "panic"):
				panic("redactor panicked")
			}
		}
		raw, _ := json.Marshal(v)
		var out any
		_ = json.Unmarshal([]byte(card.ReplaceAllString(string(raw), "****")), &out)
		return out, nil
	}))
	ctx := context.Background()
	_, span := client.Start(ctx, "run", WithInput(map[string]any{"card": "1234-5678"}))
	span.SetOutput("boom")
	span.End()
	_, other := client.Start(ctx, "other", WithInput("panic"))
	other.End()
	client.Log(ctx, LogLevelInfo, "paid", map[string]any{"card": "1234-5678"})
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	rec := onlySpan(t, sink.Spans(), "run")
	if string(rec.Input) != `{"card":"****"}` {
		t.Fatalf("input: %s", rec.Input)
	}
	if rec.Output != nil {
		t.Fatalf("a field the redactor failed on was sent: %s", rec.Output)
	}
	if in := onlySpan(t, sink.Spans(), "other").Input; in != nil {
		t.Fatalf("a field the redactor panicked on was sent: %s", in)
	}
	logs := sink.Logs()
	if len(logs) != 1 || string(logs[0].Fields) != `{"card":"****"}` {
		t.Fatalf("log fields: %+v", logs)
	}
	if got := client.Stats().RedactionFailures; got != 2 {
		t.Fatalf("redaction failures: %d", got)
	}
	// The caller's own span keeps what it was given.
	if !strings.Contains(string(span.snapshot().Input), "1234-5678") {
		t.Fatalf("the live span was rewritten: %s", span.snapshot().Input)
	}
}

func TestRedactMustKeepObjectFieldsObjects(t *testing.T) {
	client, sink := newSinkClient(t, WithRedact(func(v any) (any, error) {
		if m, ok := v.(map[string]any); ok {
			switch {
			case m["drop"] != nil:
				return nil, nil
			case m["flatten"] != nil:
				return "flattened", nil
			}
		}
		return v, nil
	}))
	ctx := context.Background()
	_, span := client.Start(ctx, "run",
		WithInput(map[string]any{"flatten": 1}),
		WithAttributes(map[string]any{"flatten": 1}),
		WithMetadata(map[string]any{"drop": 1}))
	span.End()
	client.Log(ctx, LogLevelInfo, "line", map[string]any{"flatten": 1})
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	rec := onlySpan(t, sink.Spans(), "run")
	if string(rec.Input) != `"flattened"` {
		t.Fatalf("input may be any value: %s", rec.Input)
	}
	if rec.Attributes != nil || rec.Metadata != nil || sink.Logs()[0].Fields != nil {
		t.Fatalf("an object field survived: %+v", rec)
	}
	// Attributes and log fields failed; metadata was dropped on purpose.
	if got := client.Stats().RedactionFailures; got != 2 {
		t.Fatalf("redaction failures: %d", got)
	}
}

func TestRedactNeverSeesWhatCaptureStripped(t *testing.T) {
	var mu sync.Mutex
	var seen []any
	client, sink := newSinkClient(t, WithCapturePayloads(false), WithRedact(func(v any) (any, error) {
		mu.Lock()
		defer mu.Unlock()
		seen = append(seen, v)
		return v, nil
	}))
	_, span := client.Start(context.Background(), "run", WithInput("secret"), WithMetadata(map[string]any{"k": "v"}))
	span.End()
	if err := client.Flush(context.Background()); err != nil {
		t.Fatal(err)
	}
	if rec := onlySpan(t, sink.Spans(), "run"); rec.Input != nil || string(rec.Metadata) != `{"k":"v"}` {
		t.Fatalf("record: %+v", rec)
	}
	mu.Lock()
	defer mu.Unlock()
	if len(seen) != 1 || reflect.DeepEqual(seen[0], "secret") {
		t.Fatalf("the redactor saw %v", seen)
	}
}
