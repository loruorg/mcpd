package mcpd_test

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"
	"unicode/utf8"

	mcpd "github.com/mcpd/mcpd/sdks/go"
	"github.com/mcpd/mcpd/sdks/go/mcpdtest"
)

// The tests in this file cover section 3.2 of sdks/SDK-CONTRACT.md: streamed
// model calls. They live outside the package so they can read the result back
// through mcpdtest.

// sliceStream is a provider stream over a fixed list. failAt makes Next stop
// at that position and Err report err.
type sliceStream[T any] struct {
	items  []T
	at     int
	failAt int
	err    error
	closed int
}

func newStream[T any](items ...T) *sliceStream[T] {
	return &sliceStream[T]{items: items, at: -1, failAt: -1}
}

func (s *sliceStream[T]) Next() bool {
	if s.at+1 == s.failAt || s.at+1 >= len(s.items) {
		return false
	}
	s.at++
	return true
}
func (s *sliceStream[T]) Current() T { return s.items[s.at] }
func (s *sliceStream[T]) Err() error {
	if s.failAt >= 0 && s.at+1 == s.failAt {
		return s.err
	}
	return nil
}
func (s *sliceStream[T]) Close() error { s.closed++; return nil }

// chatChunk is a typed chunk, the way a provider SDK's struct arrives.
type chatChunk struct {
	ID          string       `json:"id"`
	Object      string       `json:"object"`
	Model       string       `json:"model"`
	ServiceTier string       `json:"service_tier,omitempty"`
	Choices     []chatChoice `json:"choices"`
	Usage       *chatUsage   `json:"usage,omitempty"`
}

type chatChoice struct {
	Index        int       `json:"index"`
	Delta        chatDelta `json:"delta"`
	FinishReason string    `json:"finish_reason,omitempty"`
}

type chatDelta struct {
	Role    string `json:"role,omitempty"`
	Content string `json:"content,omitempty"`
}

type chatUsage struct {
	PromptTokens        int `json:"prompt_tokens"`
	CompletionTokens    int `json:"completion_tokens"`
	TotalTokens         int `json:"total_tokens"`
	PromptTokensDetails struct {
		CachedTokens int `json:"cached_tokens"`
	} `json:"prompt_tokens_details"`
}

func openaiChunks() []any {
	chunk := func(d chatDelta, finish string) chatChunk {
		return chatChunk{ID: "chatcmpl-1", Object: "chat.completion.chunk", Model: "gpt-4o",
			Choices: []chatChoice{{Index: 0, Delta: d, FinishReason: finish}}}
	}
	usage := &chatUsage{PromptTokens: 10, CompletionTokens: 2, TotalTokens: 12}
	usage.PromptTokensDetails.CachedTokens = 4
	return []any{
		chunk(chatDelta{Role: "assistant"}, ""),
		chunk(chatDelta{Content: "Hel"}, ""),
		// The same delta as a raw body, which is how a hand-rolled SSE reader
		// hands it over.
		json.RawMessage(`{"id":"chatcmpl-1","model":"gpt-4o","choices":[{"index":0,"delta":{"content":"lo"}}]}`),
		chunk(chatDelta{}, "stop"),
		chatChunk{ID: "chatcmpl-1", Model: "gpt-4o", ServiceTier: "flex", Choices: []chatChoice{}, Usage: usage},
	}
}

var anthropicEvents = []any{
	map[string]any{"type": "message_start", "message": map[string]any{
		"id": "msg_1", "type": "message", "model": "claude-sonnet-4-5",
		"usage": map[string]any{"input_tokens": 20, "cache_read_input_tokens": 80, "output_tokens": 1},
	}},
	map[string]any{"type": "content_block_start", "index": 0, "content_block": map[string]any{"type": "text", "text": ""}},
	map[string]any{"type": "content_block_delta", "index": 0, "delta": map[string]any{"type": "text_delta", "text": "Checking"}},
	map[string]any{"type": "content_block_stop", "index": 0},
	map[string]any{"type": "content_block_start", "index": 1, "content_block": map[string]any{
		"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": map[string]any{},
	}},
	map[string]any{"type": "content_block_delta", "index": 1, "delta": map[string]any{"type": "input_json_delta", "partial_json": `{"id":`}},
	map[string]any{"type": "content_block_delta", "index": 1, "delta": map[string]any{"type": "input_json_delta", "partial_json": `"A-1"}`}},
	map[string]any{"type": "message_delta", "delta": map[string]any{"stop_reason": "tool_use"}, "usage": map[string]any{"output_tokens": 15}},
	map[string]any{"type": "message_stop"},
}

func recorder(t *testing.T, opts ...mcpd.Option) *mcpdtest.Recorder {
	t.Helper()
	rec := mcpdtest.New(append([]mcpd.Option{mcpd.WithLogger(nil)}, opts...)...)
	t.Cleanup(func() { rec.Client.Shutdown(context.Background()) })
	return rec
}

func drain[T any](s mcpd.ChunkStream[T]) []T {
	var out []T
	for s.Next() {
		out = append(out, s.Current())
	}
	return out
}

func decode(t *testing.T, raw json.RawMessage) any {
	t.Helper()
	if len(raw) == 0 {
		return nil
	}
	var v any
	if err := json.Unmarshal(raw, &v); err != nil {
		t.Fatal(err)
	}
	return v
}

func attributes(t *testing.T, s mcpd.Span) map[string]any {
	t.Helper()
	m, _ := decode(t, s.Attributes).(map[string]any)
	if m == nil {
		m = map[string]any{}
	}
	return m
}

func str(p *string) string {
	if p == nil {
		return "<nil>"
	}
	return *p
}

func usageOf(t *testing.T, u *mcpd.TokenUsage) string {
	t.Helper()
	raw, err := json.Marshal(u)
	if err != nil {
		t.Fatal(err)
	}
	return string(raw)
}

func TestRecordStreamOpenAI(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call", mcpd.WithSpanKind(mcpd.SpanKindModelGeneration))
	chunks := append(openaiChunks(), "not a chunk", 42, nil)
	seen := drain[any](mcpd.RecordStream[any](span, newStream(chunks...), ""))
	if !reflect.DeepEqual(seen, chunks) {
		t.Errorf("chunks changed on the way through:\n got %#v\nwant %#v", seen, chunks)
	}

	s := rec.MustSpan(t, "call")
	if s.Model == nil || *s.Model != "gpt-4o" || s.Provider == nil || *s.Provider != "openai" {
		t.Errorf("model %v provider %v", s.Model, s.Provider)
	}
	if out := decode(t, s.Output); out != "Hello" {
		t.Errorf("output = %#v", out)
	}
	want := `{"input_tokens":10,"output_tokens":2,"total_tokens":12,"input_details":{"text":6,"cache_read":4}}`
	if got := usageOf(t, s.Usage); got != want {
		t.Errorf("usage = %s\nwant    %s", got, want)
	}
	attrs := attributes(t, s)
	if attrs["gen_ai.response.id"] != "chatcmpl-1" || attrs["gen_ai.response.service_tier"] != "flex" {
		t.Errorf("attributes = %v", attrs)
	}
	if !reflect.DeepEqual(attrs["gen_ai.response.finish_reasons"], []any{"stop"}) {
		t.Errorf("finish reasons = %v", attrs["gen_ai.response.finish_reasons"])
	}
	if _, ok := attrs["mcpd.time_to_first_token_ms"].(float64); !ok {
		t.Errorf("time to first token not recorded: %v", attrs)
	}
	if _, ok := attrs["mcpd.stream.truncated"]; ok {
		t.Error("truncated set on a short stream")
	}
	if s.Status != mcpd.SpanStatusOk {
		t.Errorf("status = %s", s.Status)
	}
}

func TestRecordStreamMatchesRecordResponse(t *testing.T) {
	rec := recorder(t)
	ctx := context.Background()
	_, streamed := rec.Client.Start(ctx, "streamed")
	drain[any](mcpd.RecordStream[any](streamed, newStream(anthropicEvents...), ""))
	_, whole := rec.Client.Start(ctx, "whole")
	whole.RecordResponse(map[string]any{
		"id": "msg_1", "type": "message", "model": "claude-sonnet-4-5", "stop_reason": "tool_use",
		"usage": map[string]any{"input_tokens": 20, "cache_read_input_tokens": 80, "output_tokens": 15},
	}, "")
	whole.End()

	a, b := rec.MustSpan(t, "streamed"), rec.MustSpan(t, "whole")
	if usageOf(t, a.Usage) != usageOf(t, b.Usage) || str(a.Model) != str(b.Model) || str(a.Provider) != str(b.Provider) {
		t.Errorf("streamed %s %s %s, whole %s %s %s",
			usageOf(t, a.Usage), str(a.Model), str(a.Provider), usageOf(t, b.Usage), str(b.Model), str(b.Provider))
	}
	at, bt := attributes(t, a), attributes(t, b)
	for _, key := range []string{"gen_ai.response.id", "gen_ai.response.finish_reasons"} {
		if !reflect.DeepEqual(at[key], bt[key]) {
			t.Errorf("%s: streamed %v, whole %v", key, at[key], bt[key])
		}
	}
}

func TestRecordStreamAnthropicToolCall(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	drain[any](mcpd.RecordStream[any](span, newStream(anthropicEvents...), ""))
	s := rec.MustSpan(t, "call")
	if str(s.Model) != "claude-sonnet-4-5" || str(s.Provider) != "anthropic" {
		t.Errorf("model %s provider %s", str(s.Model), str(s.Provider))
	}
	want := map[string]any{
		"content":    "Checking",
		"tool_calls": []any{map[string]any{"id": "toolu_1", "name": "lookup", "arguments": `{"id":"A-1"}`}},
	}
	if out := decode(t, s.Output); !reflect.DeepEqual(out, want) {
		t.Errorf("output = %#v", out)
	}
	if want := `{"input_tokens":100,"output_tokens":15,"total_tokens":115,`; !strings.HasPrefix(usageOf(t, s.Usage), want) {
		t.Errorf("usage = %s", usageOf(t, s.Usage))
	}
	if r := attributes(t, s)["gen_ai.response.finish_reasons"]; !reflect.DeepEqual(r, []any{"tool_use"}) {
		t.Errorf("finish reasons = %v", r)
	}
}

func TestRecordStreamOpenAIToolCallsByIndex(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	call := func(d map[string]any) map[string]any {
		return map[string]any{"choices": []map[string]any{{"index": 0, "delta": map[string]any{"tool_calls": []any{d}}}}}
	}
	stream := newStream(
		call(map[string]any{"index": 0, "id": "call_1", "function": map[string]any{"name": "lookup", "arguments": `{"a"`}}),
		call(map[string]any{"index": 1, "id": "call_2", "function": map[string]any{"name": "other", "arguments": `{}`}}),
		call(map[string]any{"index": 0, "function": map[string]any{"arguments": ":1}"}}),
		map[string]any{"choices": []any{map[string]any{"index": 0, "delta": map[string]any{}, "finish_reason": "tool_calls"}}},
	)
	drain[map[string]any](mcpd.RecordStream[map[string]any](span, stream, ""))
	want := map[string]any{"content": "", "tool_calls": []any{
		map[string]any{"id": "call_1", "name": "lookup", "arguments": `{"a":1}`},
		map[string]any{"id": "call_2", "name": "other", "arguments": `{}`},
	}}
	if out := decode(t, rec.MustSpan(t, "call").Output); !reflect.DeepEqual(out, want) {
		t.Errorf("output = %#v", out)
	}
}

func TestRecordStreamMarksFirstTokenOnce(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	stream := mcpd.RecordStream[any](span, newStream(openaiChunks()...), "")
	stream.Next() // the role only: no content yet
	if got := len(rec.Spans()); got != 0 {
		t.Fatalf("span finished after one chunk")
	}
	if _, ok := attributes(t, rec.Sink.Spans()[0])["mcpd.time_to_first_token_ms"]; ok {
		t.Error("first token marked before any content arrived")
	}
	stream.Next()
	rec.Client.Flush(context.Background())
	drain[any](stream)
	attrs := attributes(t, rec.MustSpan(t, "call"))
	ttft, ok := attrs["mcpd.time_to_first_token_ms"].(float64)
	if !ok {
		t.Fatalf("time to first token missing: %v", attrs)
	}
	// A second mark would have moved it later; the record keeps the first.
	if ttft < 0 {
		t.Errorf("time to first token = %v", ttft)
	}
}

func TestRecordStreamErrorEndsSpanAndReachesCaller(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	reset := errors.New("connection reset")
	inner := newStream(openaiChunks()...)
	inner.failAt, inner.err = 3, reset
	stream := mcpd.RecordStream[any](span, inner, "")
	if got := len(drain[any](stream)); got != 3 {
		t.Errorf("chunks before the error = %d", got)
	}
	if stream.Err() != reset {
		t.Errorf("Err() = %v, want the provider's error unchanged", stream.Err())
	}
	s := rec.MustSpan(t, "call")
	if s.Status != mcpd.SpanStatusError || s.Error == nil || s.Error.Message != "connection reset" {
		t.Errorf("status %s error %+v", s.Status, s.Error)
	}
	if out := decode(t, s.Output); out != "Hello" {
		t.Errorf("output = %#v", out)
	}
}

func TestRecordStreamEarlyClose(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	inner := newStream(openaiChunks()...)
	stream := mcpd.RecordStream[any](span, inner, "")
	for stream.Next() {
		if c, ok := stream.Current().(chatChunk); ok && c.Choices[0].Delta.Content == "Hel" {
			break
		}
	}
	if err := stream.Close(); err != nil {
		t.Fatal(err)
	}
	stream.Close()
	if inner.closed != 2 {
		t.Errorf("provider stream closed %d times, want each Close passed through", inner.closed)
	}
	s := rec.MustSpan(t, "call")
	if out := decode(t, s.Output); out != "Hel" {
		t.Errorf("output = %#v", out)
	}
	if s.Status != mcpd.SpanStatusOk {
		t.Errorf("status = %s", s.Status)
	}
	if n := len(rec.Spans()); n != 1 {
		t.Errorf("finished spans = %d", n)
	}
}

func TestRecordStreamEndsOnceAfterExhaustionAndClose(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	stream := mcpd.RecordStream[any](span, newStream(openaiChunks()...), "")
	drain[any](stream)
	end := rec.MustSpan(t, "call").EndTime
	stream.Next()
	stream.Close()
	if again := rec.MustSpan(t, "call").EndTime; !again.Equal(*end) {
		t.Errorf("end time moved from %v to %v", end, again)
	}
}

func TestRecordStreamTruncatesPastTheBudget(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	big := strings.Repeat("😀", mcpd.StreamTextBudget-1) + "ab"
	chunk := map[string]any{"choices": []any{map[string]any{"index": 0, "delta": map[string]any{"content": big}}}}
	drain[any](mcpd.RecordStream[any](span, newStream[any](chunk, chunk), ""))
	s := rec.MustSpan(t, "call")
	out, _ := decode(t, s.Output).(string)
	if n := utf8.RuneCountInString(out); n != mcpd.StreamTextBudget {
		t.Errorf("kept %d code points, want %d", n, mcpd.StreamTextBudget)
	}
	if !strings.HasSuffix(out, "😀a") {
		t.Errorf("cut in the wrong place: ...%q", out[len(out)-8:])
	}
	if attributes(t, s)["mcpd.stream.truncated"] != true {
		t.Error("mcpd.stream.truncated not set")
	}
}

func TestRecordStreamCapsToolCalls(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	var chunks []any
	for i := 0; i < mcpd.StreamMaxToolCalls+3; i++ {
		chunks = append(chunks, map[string]any{"choices": []any{map[string]any{"index": 0, "delta": map[string]any{
			"tool_calls": []any{map[string]any{"index": i, "function": map[string]any{"name": "f", "arguments": "{}"}}},
		}}}})
	}
	drain[any](mcpd.RecordStream[any](span, newStream(chunks...), ""))
	s := rec.MustSpan(t, "call")
	out, _ := decode(t, s.Output).(map[string]any)
	if calls, _ := out["tool_calls"].([]any); len(calls) != mcpd.StreamMaxToolCalls {
		t.Errorf("kept %d tool calls", len(calls))
	}
	if attributes(t, s)["mcpd.stream.truncated"] != true {
		t.Error("mcpd.stream.truncated not set")
	}
}

func TestRecordStreamPayloadCaptureOff(t *testing.T) {
	rec := recorder(t, mcpd.WithCapturePayloads(false))
	_, span := rec.Client.Start(context.Background(), "call")
	drain[any](mcpd.RecordStream[any](span, newStream(openaiChunks()...), ""))
	s := rec.MustSpan(t, "call")
	if len(s.Output) != 0 {
		t.Errorf("output = %s", s.Output)
	}
	if s.Model == nil || *s.Model != "gpt-4o" {
		t.Errorf("model = %v", s.Model)
	}
}

func TestRecordStreamUsageChunkWithNilChoices(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	chunks := openaiChunks()
	last := chunks[len(chunks)-1].(chatChunk)
	last.Choices = nil // encodes as "choices": null
	chunks[len(chunks)-1] = last
	drain[any](mcpd.RecordStream[any](span, newStream(chunks...), ""))
	s := rec.MustSpan(t, "call")
	if s.Usage == nil || s.Usage.OutputTokens == nil || *s.Usage.OutputTokens != 2 {
		t.Errorf("usage = %s", usageOf(t, s.Usage))
	}
	if attributes(t, s)["gen_ai.response.service_tier"] != "flex" {
		t.Error("service tier from the usage chunk was not recorded")
	}
}

func TestRecordStreamProviderArgument(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	drain[any](mcpd.RecordStream[any](span, newStream(openaiChunks()...), "azure.openai"))
	if p := rec.MustSpan(t, "call").Provider; p == nil || *p != "azure.openai" {
		t.Errorf("provider = %v", p)
	}
}

// panicky is a chunk whose JSON encoding panics. Reading it must not reach
// the caller.
type panicky struct{}

func (panicky) MarshalJSON() ([]byte, error) { panic("boom") }

func TestRecordStreamNeverPanics(t *testing.T) {
	rec := recorder(t)
	_, span := rec.Client.Start(context.Background(), "call")
	seen := drain[any](mcpd.RecordStream[any](span, newStream[any](panicky{}, openaiChunks()[1]), ""))
	if len(seen) != 2 {
		t.Errorf("chunks = %d", len(seen))
	}
	if out := decode(t, rec.MustSpan(t, "call").Output); out != "Hel" {
		t.Errorf("output = %#v", out)
	}

	// A nil span and a nil stream are both safe.
	inner := newStream(openaiChunks()...)
	if got := len(drain[any](mcpd.RecordStream[any](nil, inner, ""))); got != 5 {
		t.Errorf("nil span passed %d chunks", got)
	}
	nilStream := mcpd.RecordStream[int](nil, nil, "")
	if nilStream.Next() || nilStream.Current() != 0 || nilStream.Err() != nil || nilStream.Close() != nil {
		t.Error("a nil stream should read as empty")
	}
	var nothing *mcpd.RecordedStream[int]
	if nothing.Next() || nothing.Close() != nil {
		t.Error("a nil *RecordedStream should read as empty")
	}
}
