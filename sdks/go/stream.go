package mcpd

import (
	"encoding/json"
	"math"
	"sort"
	"strings"
	"sync"
)

// Streamed model calls (SDK contract 3.2).
//
// A streamed response spreads the facts RecordResponse reads across its
// chunks: the id and model in the first, the text in the middle, the finish
// reason and the usage at the end. The accumulator here collects them one
// chunk at a time and builds the response the provider would have sent
// without streaming, so the span records exactly what it would have recorded
// for the same call made the other way.
//
// Nothing here fails on a chunk it does not understand, and nothing grows
// without bound: text and tool-call arguments share one budget, and tool calls
// are capped by count.

const (
	// StreamTextBudget is how many Unicode code points of text and tool-call
	// arguments a recorded stream keeps, together.
	StreamTextBudget = 32768
	// StreamMaxToolCalls is how many tool calls a recorded stream keeps.
	// Later ones are ignored.
	StreamMaxToolCalls = 64

	attrStreamTruncated = "mcpd.stream.truncated"
)

// ChunkStream is a provider's stream of chunks. It is the shape of the
// ssestream.Stream[T] that the official OpenAI and Anthropic Go SDKs return,
// so either can be passed to RecordStream as it is.
type ChunkStream[T any] interface {
	Next() bool
	Current() T
	Err() error
	Close() error
}

// RecordedStream is a ChunkStream that records what passes through it on a
// span. Read it in place of the provider's stream: every chunk comes through
// unchanged and in order.
type RecordedStream[T any] struct {
	span     *ActiveSpan
	inner    ChunkStream[T]
	provider string
	record   bool

	mu       sync.Mutex
	acc      streamAccumulator
	marked   bool
	finished bool
}

// RecordStream wraps a provider's stream so the span records the call when
// the stream is done: the model, the provider, the usage, the
// gen_ai.response.* attributes, the time to first token and the output, as
// RecordResponse and SetOutput would for the same call made without
// streaming. An empty provider means infer it from the chunks' shape.
//
// The span ends exactly once: when Next returns false, with the stream's
// error recorded if Err is not nil, or when you call Close before the end. A
// Go method cannot take a type parameter, which is why this is a function
// rather than a method on ActiveSpan.
//
//	stream := client.Chat.Completions.NewStreaming(ctx, params)
//	recorded := mcpd.RecordStream(span, stream, "")
//	defer recorded.Close()
//	for recorded.Next() {
//		chunk := recorded.Current()
//		...
//	}
func RecordStream[T any](span *ActiveSpan, stream ChunkStream[T], provider string) *RecordedStream[T] {
	r := &RecordedStream[T]{span: span, inner: stream, provider: provider}
	// A span that records nothing has no use for the chunks, so they are not
	// decoded at all.
	r.record = span != nil && span.recording
	r.acc.budget = StreamTextBudget
	return r
}

// Next advances to the next chunk. When the stream is over it ends the span
// and returns false.
func (r *RecordedStream[T]) Next() bool {
	if r == nil {
		return false
	}
	if r.inner == nil {
		r.finish(nil)
		return false
	}
	if !r.inner.Next() {
		r.finish(r.inner.Err())
		return false
	}
	if r.record {
		r.observe(r.inner.Current())
	}
	return true
}

// Current returns the chunk Next moved to, unchanged.
func (r *RecordedStream[T]) Current() T {
	if r == nil || r.inner == nil {
		var zero T
		return zero
	}
	return r.inner.Current()
}

// Err returns the provider's error, unchanged.
func (r *RecordedStream[T]) Err() error {
	if r == nil || r.inner == nil {
		return nil
	}
	return r.inner.Err()
}

// Close ends the span if the stream was not read to the end, then closes the
// provider's stream. Stopping early is not an error, so the span keeps its
// status.
func (r *RecordedStream[T]) Close() error {
	if r == nil {
		return nil
	}
	r.finish(nil)
	if r.inner == nil {
		return nil
	}
	return r.inner.Close()
}

func (r *RecordedStream[T]) observe(chunk T) {
	first := false
	r.guard("a chunk", func() {
		r.mu.Lock()
		defer r.mu.Unlock()
		if r.finished {
			return
		}
		if r.acc.add(chunkObject(chunk)) && !r.marked {
			r.marked, first = true, true
		}
	})
	if first {
		r.span.MarkFirstToken()
	}
}

func (r *RecordedStream[T]) finish(err error) {
	r.mu.Lock()
	if r.finished {
		r.mu.Unlock()
		return
	}
	r.finished = true
	var (
		response  map[string]any
		output    any
		truncated bool
	)
	if r.record {
		// Read under the lock: a Close from another goroutine can land while
		// the reading goroutine is still folding in its last chunk.
		r.guard("the end of the stream", func() {
			response, output, truncated = r.acc.response(), r.acc.output(), r.acc.truncated
		})
	}
	r.mu.Unlock()

	r.guard("the end of the stream", func() {
		if response != nil {
			r.span.RecordResponse(response, r.provider)
		}
		if output != nil {
			r.span.SetOutput(output)
		}
		if truncated {
			r.span.mutate(func(d *Span) {
				d.Attributes = mergeAttributes(d.Attributes, map[string]any{attrStreamTruncated: true})
			})
		}
	})
	if err != nil {
		r.span.RecordError(err)
	}
	r.span.End()
}

// guard keeps a fault in the accumulator out of the caller's read loop (B1).
func (r *RecordedStream[T]) guard(what string, f func()) {
	defer func() {
		if p := recover(); p != nil && r.span != nil {
			r.span.client.logf("RecordStream could not read %s: %v", what, p)
		}
	}()
	f()
}

// chunkObject reads a chunk as a JSON object. A hand-built map may hold typed
// slices and maps, such as []map[string]any, which the accumulator would not
// recognise, so it goes through JSON like everything else.
func chunkObject(chunk any) map[string]any {
	if m, ok := chunk.(map[string]any); ok {
		raw, err := json.Marshal(m)
		if err != nil {
			return nil
		}
		return parseObject(raw)
	}
	return asObject(chunk)
}

type streamToolCall struct {
	id        string
	name      string
	arguments strings.Builder
}

// streamAccumulator is what one stream has said so far.
type streamAccumulator struct {
	shape     string
	truncated bool
	id        string
	model     string
	tier      string
	text      strings.Builder
	budget    int
	tools     map[int]*streamToolCall
	finish    map[int]string
	stop      string
	usage     map[string]any
}

// add folds one chunk in and reports whether it carried text or a tool-call
// delta.
func (a *streamAccumulator) add(c map[string]any) bool {
	if c == nil {
		return false
	}
	if choices, ok := c["choices"].([]any); ok {
		return a.openai(c, choices)
	}
	// A Go struct encodes an empty slice it never allocated as null, and the
	// chunk that carries the usage is the one with no choices.
	if raw, ok := c["choices"]; ok && raw == nil {
		return a.openai(c, nil)
	}
	switch text(c, "type") {
	case "message_start", "content_block_start", "content_block_delta", "message_delta":
		return a.anthropic(c)
	}
	return false
}

func (a *streamAccumulator) openai(c map[string]any, choices []any) bool {
	if a.shape == "" {
		a.shape = providerOpenAI
	}
	setOnce(&a.id, text(c, "id"))
	setOnce(&a.model, text(c, "model"))
	setOnce(&a.tier, text(c, "service_tier"))
	if usage := objectAt(c, "usage"); usage != nil {
		a.usage = usage
	}
	content := false
	for position, raw := range choices {
		choice, _ := raw.(map[string]any)
		if choice == nil {
			continue
		}
		at := streamIndex(choice["index"], position)
		if reason := text(choice, "finish_reason"); reason != "" {
			if a.finish == nil {
				a.finish = map[int]string{}
			}
			a.finish[at] = reason
		}
		if at != 0 {
			continue
		}
		delta := objectAt(choice, "delta")
		if delta == nil {
			continue
		}
		if t := text(delta, "content"); t != "" {
			content = true
			a.text.WriteString(a.take(t))
		}
		calls, _ := delta["tool_calls"].([]any)
		for i, rawCall := range calls {
			call, _ := rawCall.(map[string]any)
			if call == nil {
				continue
			}
			content = true
			tool := a.tool(streamIndex(call["index"], i))
			if tool == nil {
				continue
			}
			fn := objectAt(call, "function")
			setOnce(&tool.id, text(call, "id"))
			setOnce(&tool.name, text(fn, "name"))
			if args := text(fn, "arguments"); args != "" {
				tool.arguments.WriteString(a.take(args))
			}
		}
	}
	return content
}

func (a *streamAccumulator) anthropic(c map[string]any) bool {
	if a.shape == "" {
		a.shape = providerAnthropic
	}
	switch text(c, "type") {
	case "message_start":
		message := objectAt(c, "message")
		setOnce(&a.id, text(message, "id"))
		setOnce(&a.model, text(message, "model"))
		if usage := objectAt(message, "usage"); usage != nil {
			a.usage = copyObject(usage)
		}
		return false
	case "content_block_start":
		block := objectAt(c, "content_block")
		if text(block, "type") == "tool_use" {
			if tool := a.tool(streamIndex(c["index"], len(a.tools))); tool != nil {
				setOnce(&tool.id, text(block, "id"))
				setOnce(&tool.name, text(block, "name"))
			}
			return true
		}
		t := text(block, "text")
		if t == "" {
			return false
		}
		a.text.WriteString(a.take(t))
		return true
	case "content_block_delta":
		delta := objectAt(c, "delta")
		switch text(delta, "type") {
		case "text_delta":
			t := text(delta, "text")
			if t == "" {
				return false
			}
			a.text.WriteString(a.take(t))
			return true
		case "input_json_delta":
			args := text(delta, "partial_json")
			if args == "" {
				return false
			}
			if tool := a.tools[streamIndex(c["index"], -1)]; tool != nil {
				tool.arguments.WriteString(a.take(args))
			}
			return true
		}
		return false
	case "message_delta":
		if stop := text(objectAt(c, "delta"), "stop_reason"); stop != "" {
			a.stop = stop
		}
		if usage := objectAt(c, "usage"); usage != nil {
			merged := copyObject(a.usage)
			for k, v := range usage {
				if v != nil {
					merged[k] = v
				}
			}
			a.usage = merged
		}
		return false
	}
	return false
}

// tool returns the tool call at the index, made on first sight, or nil past
// the cap.
func (a *streamAccumulator) tool(at int) *streamToolCall {
	if known := a.tools[at]; known != nil {
		return known
	}
	if len(a.tools) >= StreamMaxToolCalls {
		a.truncated = true
		return nil
	}
	if a.tools == nil {
		a.tools = map[int]*streamToolCall{}
	}
	tool := &streamToolCall{}
	a.tools[at] = tool
	return tool
}

// take returns as much of s as the budget still allows, counted in code
// points so a cut never splits a character.
func (a *streamAccumulator) take(s string) string {
	if a.budget <= 0 {
		a.truncated = true
		return ""
	}
	n := 0
	for at := range s {
		if n == a.budget {
			a.truncated = true
			a.budget = 0
			return s[:at]
		}
		n++
	}
	a.budget -= n
	return s
}

// response is the response the provider would have sent, or nil when no
// chunk was recognised.
func (a *streamAccumulator) response() map[string]any {
	put := func(m map[string]any, key, value string) {
		if value != "" {
			m[key] = value
		}
	}
	switch a.shape {
	case providerOpenAI:
		out := map[string]any{"object": "chat.completion"}
		put(out, "id", a.id)
		put(out, "model", a.model)
		put(out, "service_tier", a.tier)
		choices := make([]any, 0, len(a.finish))
		for _, i := range sortedKeys(a.finish) {
			choices = append(choices, map[string]any{"index": i, "finish_reason": a.finish[i]})
		}
		out["choices"] = choices
		if a.usage != nil {
			out["usage"] = a.usage
		}
		return out
	case providerAnthropic:
		out := map[string]any{"type": "message"}
		put(out, "id", a.id)
		put(out, "model", a.model)
		put(out, "stop_reason", a.stop)
		if a.usage != nil {
			out["usage"] = a.usage
		}
		return out
	}
	return nil
}

// output is the text, or the text and the tool calls, or nil when neither
// arrived.
func (a *streamAccumulator) output() any {
	if len(a.tools) == 0 {
		if a.text.Len() == 0 {
			return nil
		}
		return a.text.String()
	}
	calls := make([]map[string]any, 0, len(a.tools))
	for _, i := range sortedKeys(a.tools) {
		tool := a.tools[i]
		call := map[string]any{"arguments": tool.arguments.String()}
		if tool.id != "" {
			call["id"] = tool.id
		}
		if tool.name != "" {
			call["name"] = tool.name
		}
		calls = append(calls, call)
	}
	return map[string]any{"content": a.text.String(), "tool_calls": calls}
}

func setOnce(field *string, value string) {
	if *field == "" {
		*field = value
	}
}

func copyObject(m map[string]any) map[string]any {
	out := make(map[string]any, len(m))
	for k, v := range m {
		out[k] = v
	}
	return out
}

func sortedKeys[V any](m map[int]V) []int {
	keys := make([]int, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Ints(keys)
	return keys
}

// streamIndex reads a chunk's index: a whole number, zero or more. Anything
// else gives the fallback.
func streamIndex(v any, fallback int) int {
	var f float64
	switch n := v.(type) {
	case json.Number:
		if i, err := n.Int64(); err == nil {
			if i >= 0 && i <= math.MaxInt32 {
				return int(i)
			}
			return fallback
		}
		parsed, err := n.Float64()
		if err != nil {
			return fallback
		}
		f = parsed
	case float64:
		f = n
	case int:
		f = float64(n)
	default:
		return fallback
	}
	if f < 0 || f > math.MaxInt32 || f != math.Trunc(f) {
		return fallback
	}
	return int(f)
}
