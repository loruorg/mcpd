package mcpd

import (
	"bytes"
	"encoding/json"
	"math"
	"time"
)

// Model calls: what an OpenAI- or Anthropic-shaped response says about the
// call that produced it, mapped onto mcpd's usage record (SDK contract 3.1).
//
// Nothing here imports a provider SDK. The response is read as generic JSON,
// so a provider SDK's own struct, a parsed body, raw bytes and a hand-built
// map all work, and a field that is missing or of the wrong type is left
// absent rather than reported: instrumentation never fails the call it
// describes (B1).

// The attribute names this file writes. They are part of the wire contract,
// so every SDK spells them the same way.
const (
	attrResponseID       = "gen_ai.response.id"
	attrFinishReasons    = "gen_ai.response.finish_reasons"
	attrServiceTier      = "gen_ai.response.service_tier"
	attrTimeToFirstToken = "mcpd.time_to_first_token_ms"
	attrGuardrailTripped = "mcpd.guardrail.tripped"
	attrGuardrailReason  = "mcpd.guardrail.reason"
	providerOpenAI       = "openai"
	providerAnthropic    = "anthropic"
)

// asObject reads a response as a JSON object. A map is used as it is; bytes
// are parsed; anything else goes through encoding/json, which is how a
// provider SDK's typed struct is read by its JSON field names without this
// package importing it. A value that is not an object, or that panics while
// marshalling, gives nil.
func asObject(response any) (out map[string]any) {
	defer func() {
		if recover() != nil {
			out = nil
		}
	}()
	switch v := response.(type) {
	case nil:
		return nil
	case map[string]any:
		return v
	case json.RawMessage:
		return parseObject(v)
	case []byte:
		return parseObject(v)
	case string:
		return parseObject([]byte(v))
	}
	raw, err := json.Marshal(response)
	if err != nil {
		return nil
	}
	return parseObject(raw)
}

// parseObject decodes with UseNumber so a token count above 2^53 survives
// exactly rather than through a float.
func parseObject(raw []byte) map[string]any {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var out map[string]any
	if dec.Decode(&out) != nil {
		return nil
	}
	return out
}

func objectAt(m map[string]any, key string) map[string]any {
	if m == nil {
		return nil
	}
	v, _ := m[key].(map[string]any)
	return v
}

// count reads a non-negative token count. A float is floored, as the other
// SDKs do; a negative, non-finite or non-numeric value is absent.
func count(m map[string]any, key string) *uint64 {
	if m == nil {
		return nil
	}
	var f float64
	switch v := m[key].(type) {
	case json.Number:
		if n, err := v.Int64(); err == nil {
			if n < 0 {
				return nil
			}
			u := uint64(n)
			return &u
		}
		parsed, err := v.Float64()
		if err != nil {
			return nil
		}
		f = parsed
	case float64:
		f = v
	case float32:
		f = float64(v)
	case int:
		f = float64(v)
	case int32:
		f = float64(v)
	case int64:
		if v < 0 {
			return nil
		}
		u := uint64(v)
		return &u
	case uint:
		u := uint64(v)
		return &u
	case uint32:
		u := uint64(v)
		return &u
	case uint64:
		u := v
		return &u
	default:
		return nil
	}
	if math.IsNaN(f) || math.IsInf(f, 0) || f < 0 || f >= math.MaxUint64 {
		return nil
	}
	u := uint64(math.Floor(f))
	return &u
}

func text(m map[string]any, key string) string {
	if m == nil {
		return ""
	}
	s, _ := m[key].(string)
	return s
}

func has(m map[string]any, key string) bool {
	if m == nil {
		return false
	}
	_, ok := m[key]
	return ok
}

func firstCount(m map[string]any, keys ...string) *uint64 {
	for _, key := range keys {
		if v := count(m, key); v != nil {
			return v
		}
	}
	return nil
}

func firstObject(m map[string]any, keys ...string) map[string]any {
	for _, key := range keys {
		if v := objectAt(m, key); v != nil {
			return v
		}
	}
	return nil
}

func u64(v uint64) *uint64 { return &v }

// UsageFromOpenAI reads token usage from an OpenAI chat-completions or
// responses-API response. It returns nil when the response carries no usage
// it can read.
//
// The prompt total is gross, so the plain text share is the total less the
// cached and audio tokens.
func UsageFromOpenAI(response any) *TokenUsage {
	return usageFromOpenAI(asObject(response))
}

func usageFromOpenAI(r map[string]any) *TokenUsage {
	usage := objectAt(r, "usage")
	if usage == nil {
		return nil
	}
	input := firstCount(usage, "prompt_tokens", "input_tokens")
	output := firstCount(usage, "completion_tokens", "output_tokens")
	inDetail := firstObject(usage, "prompt_tokens_details", "input_tokens_details")
	outDetail := firstObject(usage, "completion_tokens_details", "output_tokens_details")
	cached := count(inDetail, "cached_tokens")
	audioIn := count(inDetail, "audio_tokens")

	out := &TokenUsage{InputTokens: input, OutputTokens: output}
	out.TotalTokens = count(usage, "total_tokens")
	if out.TotalTokens == nil && input != nil && output != nil {
		out.TotalTokens = u64(*input + *output)
	}
	in := InputTokenDetails{CacheRead: cached, Audio: audioIn}
	if input != nil && (cached != nil || audioIn != nil) {
		var less uint64
		if cached != nil {
			less += *cached
		}
		if audioIn != nil {
			less += *audioIn
		}
		if less > *input {
			less = *input
		}
		in.Text = u64(*input - less)
	}
	if in != (InputTokenDetails{}) {
		out.InputDetails = &in
	}
	outD := OutputTokenDetails{Reasoning: count(outDetail, "reasoning_tokens"), Audio: count(outDetail, "audio_tokens")}
	if outD != (OutputTokenDetails{}) {
		out.OutputDetails = &outD
	}
	if *out == (TokenUsage{}) {
		return nil
	}
	return out
}

// UsageFromAnthropic reads token usage from an Anthropic messages response. It
// returns nil when the response carries no usage it can read.
//
// input_tokens there is net of the cache, so it is the text share, and the
// prompt's size is the sum of it and both cache meters.
func UsageFromAnthropic(response any) *TokenUsage {
	return usageFromAnthropic(asObject(response))
}

func usageFromAnthropic(r map[string]any) *TokenUsage {
	usage := objectAt(r, "usage")
	if usage == nil {
		return nil
	}
	textTokens := count(usage, "input_tokens")
	read := count(usage, "cache_read_input_tokens")
	write := count(usage, "cache_creation_input_tokens")
	split := objectAt(usage, "cache_creation")
	output := count(usage, "output_tokens")

	out := &TokenUsage{OutputTokens: output}
	if textTokens != nil || read != nil || write != nil {
		var sum uint64
		for _, p := range []*uint64{textTokens, read, write} {
			if p != nil {
				sum += *p
			}
		}
		out.InputTokens = &sum
	}
	if out.InputTokens != nil && output != nil {
		out.TotalTokens = u64(*out.InputTokens + *output)
	}
	in := InputTokenDetails{
		Text:         textTokens,
		CacheRead:    read,
		CacheWrite:   write,
		CacheWrite5m: count(split, "ephemeral_5m_input_tokens"),
		CacheWrite1h: count(split, "ephemeral_1h_input_tokens"),
	}
	if in != (InputTokenDetails{}) {
		out.InputDetails = &in
	}
	if *out == (TokenUsage{}) {
		return nil
	}
	return out
}

// ResponseShape names the provider whose shape the response has: "openai",
// "anthropic", or "" when neither.
func ResponseShape(response any) string {
	return responseShape(asObject(response))
}

func responseShape(r map[string]any) string {
	if r == nil {
		return ""
	}
	usage := objectAt(r, "usage")
	if text(r, "type") == "message" || has(usage, "cache_read_input_tokens") {
		return providerAnthropic
	}
	if has(usage, "prompt_tokens") || has(usage, "input_tokens_details") {
		return providerOpenAI
	}
	if _, ok := r["choices"].([]any); ok || text(r, "object") == "response" {
		return providerOpenAI
	}
	if has(usage, "input_tokens") {
		return providerAnthropic
	}
	return ""
}

// ResponseFacts is what RecordResponse sets on a span.
type ResponseFacts struct {
	// Model is the response's model, or "".
	Model string
	// Provider is the one given, or the one inferred from the shape, or "".
	Provider string
	// Usage is nil when the response carries none that could be read.
	Usage *TokenUsage
	// Attributes holds the gen_ai.response.* keys that were present. Never
	// nil.
	Attributes map[string]any
}

// FactsFromResponse reads everything a response says about its call: the
// model, the provider, the usage, and the id, finish reasons and service tier
// as gen_ai.response.* attributes. Never the text: that stays the caller's to
// record or not, under payload capture. An empty provider means infer it from
// the shape.
func FactsFromResponse(response any, provider string) ResponseFacts {
	r := asObject(response)
	shape := responseShape(r)
	facts := ResponseFacts{Model: text(r, "model"), Provider: provider, Attributes: map[string]any{}}
	if facts.Provider == "" {
		facts.Provider = shape
	}
	if id := text(r, "id"); id != "" {
		facts.Attributes[attrResponseID] = id
	}
	var reasons []string
	if choices, ok := r["choices"].([]any); ok {
		for _, c := range choices {
			choice, _ := c.(map[string]any)
			if reason := text(choice, "finish_reason"); reason != "" {
				reasons = append(reasons, reason)
			}
		}
	}
	if stop := text(r, "stop_reason"); stop != "" {
		reasons = append(reasons, stop)
	}
	if len(reasons) == 0 && text(r, "object") == "response" {
		if status := text(r, "status"); status != "" {
			reasons = append(reasons, status)
		}
	}
	if len(reasons) > 0 {
		facts.Attributes[attrFinishReasons] = reasons
	}
	tier := text(r, "service_tier")
	if tier == "" {
		tier = text(objectAt(r, "usage"), "service_tier")
	}
	if tier != "" {
		facts.Attributes[attrServiceTier] = tier
	}
	switch shape {
	case providerAnthropic:
		facts.Usage = usageFromAnthropic(r)
	case providerOpenAI:
		facts.Usage = usageFromOpenAI(r)
	}
	return facts
}

// RecordResponse sets the model, provider, usage and gen_ai.response.*
// attributes from a provider's response (contract 3.1). An empty provider
// means infer "openai" or "anthropic" from the shape; pass one to name a
// gateway, such as "aws.bedrock". It never sets input or output, and an
// unknown shape records what it can and nothing else.
func (s *ActiveSpan) RecordResponse(response any, provider string) {
	if s == nil || !s.recording {
		return
	}
	facts := FactsFromResponse(response, provider)
	s.mutate(func(d *Span) {
		if facts.Model != "" {
			d.Model = strPtr(facts.Model)
		}
		if facts.Provider != "" {
			d.Provider = strPtr(facts.Provider)
		}
		if facts.Usage != nil {
			d.Usage = facts.Usage
		}
		if len(facts.Attributes) > 0 {
			d.Attributes = mergeAttributes(d.Attributes, facts.Attributes)
		}
	})
}

// MarkFirstToken records the time to first token, from this span's start to
// now, as mcpd.time_to_first_token_ms. Only the first call counts, so it can
// sit inside a streaming loop.
func (s *ActiveSpan) MarkFirstToken() {
	if s == nil || !s.recording {
		return
	}
	now := time.Now()
	s.mutate(func(d *Span) {
		if attributeSet(d.Attributes, attrTimeToFirstToken) {
			return
		}
		ms := float64(now.Sub(d.StartTime).Nanoseconds()) / 1e6
		if ms < 0 {
			ms = 0
		}
		d.Attributes = mergeAttributes(d.Attributes, map[string]any{attrTimeToFirstToken: ms})
	})
}

// SetGuardrail records a guardrail's outcome as mcpd.guardrail.tripped and,
// when reason is not empty, mcpd.guardrail.reason. Tripping is not an error by
// itself; call RecordError as well if it should fail the span.
func (s *ActiveSpan) SetGuardrail(tripped bool, reason string) {
	if s == nil || !s.recording {
		return
	}
	add := map[string]any{attrGuardrailTripped: tripped}
	if reason != "" {
		add[attrGuardrailReason] = reason
	}
	s.mutate(func(d *Span) { d.Attributes = mergeAttributes(d.Attributes, add) })
}

// mergeAttributes adds keys to an encoded attribute object. Existing
// attributes the caller set are kept unless a key is written again; an
// existing value that is not an object is replaced, since there is nothing to
// merge into. It never fails: a value that cannot be encoded leaves the
// attributes as they were.
func mergeAttributes(existing json.RawMessage, add map[string]any) json.RawMessage {
	merged := parseObject(existing)
	if merged == nil {
		merged = make(map[string]any, len(add))
	}
	for k, v := range add {
		merged[k] = v
	}
	raw, err := json.Marshal(merged)
	if err != nil {
		return existing
	}
	return raw
}

func attributeSet(existing json.RawMessage, key string) bool {
	return has(parseObject(existing), key)
}
