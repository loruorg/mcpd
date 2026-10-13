package mcpd

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"
)

// promptServer answers the prompt endpoints from a script and remembers every
// request, so a test can say exactly what reached the wire.
type promptServer struct {
	mu       sync.Mutex
	requests []*url.URL
	methods  []string
	bodies   [][]byte
	// answer decides the reply to each request. Nil answers 200 with prompt.
	answer func(r *http.Request) (int, any)
	prompt Prompt
}

func (s *promptServer) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	body, _ := readRequestBody(r)
	s.mu.Lock()
	u := *r.URL
	s.requests = append(s.requests, &u)
	s.methods = append(s.methods, r.Method)
	s.bodies = append(s.bodies, body)
	answer, prompt := s.answer, s.prompt
	s.mu.Unlock()

	status, out := http.StatusOK, any(prompt)
	if answer != nil {
		status, out = answer(r)
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if status != http.StatusOK {
		out = map[string]string{"error": "not_found", "message": "no such prompt"}
	}
	json.NewEncoder(w).Encode(out)
}

func (s *promptServer) count() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.requests)
}

func (s *promptServer) request(i int) *url.URL {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.requests[i]
}

func (s *promptServer) setAnswer(answer func(r *http.Request) (int, any)) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.answer = answer
}

func newPromptClient(t *testing.T, opts ...Option) (*Client, *promptServer) {
	t.Helper()
	template := "Answer the customer in {{language}}."
	srv := &promptServer{prompt: Prompt{
		Project: "support", Name: "support/answer", Version: 3, Kind: PromptKindText,
		Template: &template, Variables: []string{"language"}, Labels: []string{"production"},
	}}
	server := httptest.NewServer(srv)
	t.Cleanup(server.Close)
	base := []Option{
		WithEndpoint(server.URL), WithProject("support"), WithLogger(nil),
		WithTimeout(2 * time.Second),
	}
	client := New(append(base, opts...)...)
	t.Cleanup(func() { client.Shutdown(context.Background()) })
	return client, srv
}

type lineLogger struct {
	mu    sync.Mutex
	lines []string
}

func (l *lineLogger) Printf(format string, args ...any) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.lines = append(l.lines, fmt.Sprintf(format, args...))
}

func (l *lineLogger) count() int {
	l.mu.Lock()
	defer l.mu.Unlock()
	return len(l.lines)
}

func durationPtr(d time.Duration) *time.Duration { return &d }

func TestGetPromptAsksForProductionAndFallsBackToTheLatestOnA404(t *testing.T) {
	client, srv := newPromptClient(t)
	srv.setAnswer(func(r *http.Request) (int, any) {
		if r.URL.Query().Has("label") {
			return http.StatusNotFound, nil
		}
		return http.StatusOK, srv.prompt
	})

	p, err := client.GetPrompt(context.Background(), "support/answer", PromptOptions{})
	if err != nil {
		t.Fatalf("get prompt: %v", err)
	}
	if p.Version != 3 {
		t.Errorf("version %d", p.Version)
	}
	if srv.count() != 2 {
		t.Fatalf("%d requests, want 2", srv.count())
	}
	first, second := srv.request(0), srv.request(1)
	if first.EscapedPath() != "/v1/api/prompts/support%2Fanswer" {
		t.Errorf("path %q: a name with / must stay one segment", first.EscapedPath())
	}
	if first.Query().Get("label") != "production" || first.Query().Get("project") != "support" {
		t.Errorf("first request %q", first.RawQuery)
	}
	if q := second.Query(); q.Has("label") || q.Has("version") || q.Get("project") != "support" {
		t.Errorf("second request %q should carry no selector", second.RawQuery)
	}
}

func TestGetPromptSendsTheSelectorItWasGiven(t *testing.T) {
	client, srv := newPromptClient(t)
	ctx := context.Background()
	if _, err := client.GetPrompt(ctx, "support/answer", PromptOptions{Label: "staging"}); err != nil {
		t.Fatal(err)
	}
	if _, err := client.GetPrompt(ctx, "support/answer", PromptOptions{Version: 2}); err != nil {
		t.Fatal(err)
	}
	if q := srv.request(0).Query(); q.Get("label") != "staging" || q.Has("version") {
		t.Errorf("label request %q", q.Encode())
	}
	if q := srv.request(1).Query(); q.Get("version") != "2" || q.Has("label") {
		t.Errorf("version request %q", q.Encode())
	}
}

func TestBothSelectorsAreRefusedBeforeTheWire(t *testing.T) {
	client, srv := newPromptClient(t)
	_, err := client.GetPrompt(context.Background(), "support/answer",
		PromptOptions{Label: "production", Version: 2})
	if err == nil || !strings.Contains(err.Error(), "not both") {
		t.Fatalf("error %v", err)
	}
	if srv.count() != 0 {
		t.Errorf("%d requests reached the wire", srv.count())
	}
}

func TestAFreshCopyIsServedWithNoRequest(t *testing.T) {
	client, srv := newPromptClient(t)
	ctx := context.Background()
	first, err := client.GetPrompt(ctx, "support/answer", PromptOptions{Label: "production"})
	if err != nil {
		t.Fatal(err)
	}
	first.Labels[0] = "edited by the caller"
	second, err := client.GetPrompt(ctx, "support/answer", PromptOptions{Label: "production"})
	if err != nil {
		t.Fatal(err)
	}
	if srv.count() != 1 {
		t.Errorf("%d requests, want 1", srv.count())
	}
	if second.Labels[0] != "production" {
		t.Errorf("the cache served a copy the caller had edited: %v", second.Labels)
	}

	// A different selector is a different entry.
	if _, err := client.GetPrompt(ctx, "support/answer", PromptOptions{Version: 3}); err != nil {
		t.Fatal(err)
	}
	if srv.count() != 2 {
		t.Errorf("%d requests, want 2", srv.count())
	}

	// A zero TTL fetches every time.
	if _, err := client.GetPrompt(ctx, "support/answer",
		PromptOptions{Label: "production", CacheTTL: durationPtr(0)}); err != nil {
		t.Fatal(err)
	}
	if srv.count() != 3 {
		t.Errorf("%d requests, want 3", srv.count())
	}
}

func TestAStaleCopyIsServedWhenTheRefetchFails(t *testing.T) {
	logger := &lineLogger{}
	client, srv := newPromptClient(t, WithLogger(logger), WithFlushInterval(time.Millisecond))
	ctx := context.Background()
	opts := PromptOptions{Label: "production", CacheTTL: durationPtr(0)}
	if _, err := client.GetPrompt(ctx, "support/answer", opts); err != nil {
		t.Fatal(err)
	}

	srv.setAnswer(func(*http.Request) (int, any) { return http.StatusInternalServerError, nil })
	for i := 0; i < 3; i++ {
		time.Sleep(2 * time.Millisecond)
		p, err := client.GetPrompt(ctx, "support/answer", opts)
		if err != nil {
			t.Fatalf("a failed refetch raised: %v", err)
		}
		if p.Version != 3 {
			t.Errorf("served version %d", p.Version)
		}
	}
	if srv.count() != 4 {
		t.Errorf("%d requests, want 4", srv.count())
	}
	if logger.count() != 1 {
		t.Errorf("the failure was reported %d times, want once: %v", logger.count(), logger.lines)
	}
}

func TestAFallbackIsReturnedWhenNothingWasFetched(t *testing.T) {
	client, srv := newPromptClient(t)
	srv.setAnswer(func(*http.Request) (int, any) { return http.StatusNotFound, nil })
	ctx := context.Background()

	p, err := client.GetPrompt(ctx, "greeting", PromptOptions{
		FallbackTemplate: "Hello {{ name }}, about {{topic}} and {{name}}.",
	})
	if err != nil {
		t.Fatalf("get prompt: %v", err)
	}
	if p.Name != "greeting" || p.Version != 0 || p.Kind != PromptKindText ||
		p.Project != "support" || len(p.Labels) != 0 {
		t.Errorf("fallback %+v", p)
	}
	if !reflect.DeepEqual(p.Variables, []string{"name", "topic"}) {
		t.Errorf("variables %v", p.Variables)
	}

	// Never cached: the next call asks again, and with no fallback it fails.
	before := srv.count()
	_, err = client.GetPrompt(ctx, "greeting", PromptOptions{})
	var apiErr *APIError
	if !errors.As(err, &apiErr) || apiErr.Status != http.StatusNotFound {
		t.Errorf("error %v, want the 404", err)
	}
	if srv.count() == before {
		t.Error("a fallback was cached")
	}

	chat, err := client.GetPrompt(ctx, "triage", PromptOptions{
		FallbackMessages: []map[string]any{
			{"role": "system", "content": "Sort tickets for {{team}}."},
			{"role": "user", "content": "{{ticket}}"},
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if chat.Kind != PromptKindChat || !reflect.DeepEqual(chat.Variables, []string{"team", "ticket"}) {
		t.Errorf("chat fallback %+v", chat)
	}
}

func TestADisabledClientStillFetchesPrompts(t *testing.T) {
	client, srv := newPromptClient(t, WithEnabled(false))
	if _, err := client.GetPrompt(context.Background(), "support/answer", PromptOptions{}); err != nil {
		t.Fatal(err)
	}
	if srv.count() != 1 {
		t.Errorf("%d requests, want 1", srv.count())
	}
}

func TestThePromptCacheIsBounded(t *testing.T) {
	client, _ := newPromptClient(t)
	ctx := context.Background()
	for i := 0; i < promptCacheSize+10; i++ {
		if _, err := client.GetPrompt(ctx, fmt.Sprintf("p%d", i), PromptOptions{Version: 1}); err != nil {
			t.Fatal(err)
		}
	}
	if n := client.prompts.len(); n != promptCacheSize {
		t.Errorf("%d entries, want %d", n, promptCacheSize)
	}
	if client.prompts.get(promptCacheKey("support", "p0", PromptOptions{Version: 1})) != nil {
		t.Error("the oldest entry was not evicted")
	}
}

func TestCreatePromptAndSetPromptLabel(t *testing.T) {
	client, srv := newPromptClient(t)
	ctx := context.Background()
	template := "Hi {{name}}"
	if _, err := client.CreatePrompt(ctx, CreatePromptRequest{
		Name: "support/answer", Template: &template, Labels: []string{"production"},
	}); err != nil {
		t.Fatal(err)
	}
	if _, err := client.SetPromptLabel(ctx, "support/answer", "production", 2); err != nil {
		t.Fatal(err)
	}
	srv.mu.Lock()
	defer srv.mu.Unlock()
	if srv.methods[0] != http.MethodPost || srv.requests[0].Path != "/v1/api/prompts" ||
		srv.requests[0].Query().Get("project") != "support" {
		t.Errorf("create went to %s %v", srv.methods[0], srv.requests[0])
	}
	var created CreatePromptRequest
	if err := json.Unmarshal(srv.bodies[0], &created); err != nil || created.Name != "support/answer" ||
		*created.Template != template || created.Labels[0] != "production" {
		t.Errorf("create body %s", srv.bodies[0])
	}
	if srv.methods[1] != http.MethodPut ||
		srv.requests[1].EscapedPath() != "/v1/api/prompts/support%2Fanswer/labels/production" ||
		srv.requests[1].Query().Get("project") != "support" {
		t.Errorf("label went to %s %v", srv.methods[1], srv.requests[1])
	}
	if string(srv.bodies[1]) != `{"version":2}` {
		t.Errorf("label body %s", srv.bodies[1])
	}
}

func TestRenderingFollowsTheDaemonsRule(t *testing.T) {
	text := func(template string) *Prompt {
		return &Prompt{Name: "t", Kind: PromptKindText, Template: &template}
	}
	vars := map[string]string{"city": "Leeds", "x": "{{city}}", "a.b": "dotted"}
	cases := []struct{ template, want string }{
		{"Weather in {{city}} and {{ city }}.", "Weather in Leeds and Leeds."},
		{"{{\tcity\n}}", "Leeds"},
		{"JSON: {{ 1abc }} {{a-b}} {{}} {{ }}", "JSON: {{ 1abc }} {{a-b}} {{}} {{ }}"},
		{"one pass: {{x}}", "one pass: {{city}}"},
		{"dotted {{a.b}}", "dotted dotted"},
		{"unclosed {{city", "unclosed {{city"},
		{"{{city}} then {{city", "Leeds then {{city"},
		{"no braces", "no braces"},
	}
	for _, tc := range cases {
		out, err := RenderPrompt(text(tc.template), vars)
		if err != nil {
			t.Errorf("%q: %v", tc.template, err)
			continue
		}
		if out.Kind != PromptKindText || out.Text != tc.want {
			t.Errorf("%q rendered %q, want %q", tc.template, out.Text, tc.want)
		}
	}
}

func TestAMissingVariableIsNamedInTheError(t *testing.T) {
	template := "{{b}} {{city}} {{a}} {{b}}"
	_, err := RenderPrompt(&Prompt{Name: "t", Kind: PromptKindText, Template: &template},
		map[string]string{"city": "Leeds", "unused": "ignored"})
	var renderErr *PromptRenderError
	if !errors.As(err, &renderErr) {
		t.Fatalf("error %v, want *PromptRenderError", err)
	}
	if !reflect.DeepEqual(renderErr.Missing, []string{"b", "a"}) {
		t.Errorf("missing %v, want [b a]", renderErr.Missing)
	}
	if !strings.Contains(err.Error(), "b, a") {
		t.Errorf("message %q", err.Error())
	}
}

func TestChatContentIsRenderedAndOtherFieldsKept(t *testing.T) {
	p := &Prompt{Name: "chat", Kind: PromptKindChat, Messages: json.RawMessage(`[
		{"role":"system","content":"Help {{ team }}.","name":"{{team}}","weight":1.50},
		{"role":"user","content":[{"type":"text","text":"{{team}}"}]}
	]`)}
	out, err := RenderPrompt(p, map[string]string{"team": "billing"})
	if err != nil {
		t.Fatal(err)
	}
	if out.Kind != PromptKindChat || len(out.Messages) != 2 {
		t.Fatalf("rendered %+v", out)
	}
	first := out.Messages[0]
	if first["content"] != "Help billing." || first["role"] != "system" || first["name"] != "{{team}}" {
		t.Errorf("first message %v", first)
	}
	if first["weight"] != json.Number("1.50") {
		t.Errorf("weight %v (%T)", first["weight"], first["weight"])
	}
	raw, _ := json.Marshal(out.Messages[1]["content"])
	if string(raw) != `[{"text":"{{team}}","type":"text"}]` {
		t.Errorf("non-string content changed: %s", raw)
	}
	if !strings.Contains(string(p.Messages), "{{ team }}") {
		t.Error("rendering changed the prompt itself")
	}

	_, err = RenderPrompt(p, nil)
	var renderErr *PromptRenderError
	if !errors.As(err, &renderErr) || !reflect.DeepEqual(renderErr.Missing, []string{"team"}) {
		t.Errorf("error %v", err)
	}
}

func TestSetPromptSetsBothAttributesAndNeverPanics(t *testing.T) {
	client, sink := newTestClient(t)
	ctx := context.Background()

	_, span := client.Start(ctx, "draft", WithAttributes(map[string]any{"tool": "search"}))
	span.SetPrompt(&Prompt{Name: "support/answer", Version: 3})
	span.End()

	_, fallback := client.Start(ctx, "fallback")
	fallback.SetPrompt(&Prompt{Name: "support/answer", Version: 3})
	fallback.SetPrompt(&Prompt{Name: "support/answer"})
	fallback.End()

	_, bad := client.Start(ctx, "bad")
	bad.SetPrompt(nil)
	bad.SetPrompt(&Prompt{})
	bad.End()

	var nilSpan *ActiveSpan
	nilSpan.SetPrompt(&Prompt{Name: "x", Version: 1})

	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	byName := map[string]map[string]any{}
	for _, s := range sink.spans() {
		attrs := map[string]any{}
		if len(s.Attributes) > 0 {
			json.Unmarshal(s.Attributes, &attrs)
		}
		byName[s.Name] = attrs
	}
	draft := byName["draft"]
	if draft["mcpd.prompt.name"] != "support/answer" || draft["mcpd.prompt.version"] != float64(3) ||
		draft["tool"] != "search" {
		t.Errorf("draft attributes %v", draft)
	}
	if fb := byName["fallback"]; fb["mcpd.prompt.name"] != "support/answer" || fb["mcpd.prompt.version"] != nil {
		t.Errorf("fallback attributes %v", fb)
	}
	if len(byName["bad"]) != 0 {
		t.Errorf("a bad prompt set %v", byName["bad"])
	}
}

func TestListFiltersSendTheDocumentedParametersOnly(t *testing.T) {
	var mu sync.Mutex
	var captured []url.Values
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		captured = append(captured, r.URL.Query())
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		w.Write([]byte(`{"traces":[],"spans":[],"page":{"has_more":false}}`))
	}))
	defer server.Close()
	client := New(WithEndpoint(server.URL), WithProject("support"), WithLogger(nil))
	defer client.Shutdown(context.Background())
	ctx := context.Background()

	no, yes := false, true
	if _, err := client.ListTraces(ctx, TraceQuery{
		MaxDurationMs: 5000, MaxCost: 0.25, MinTokens: 1000,
		HasScore: "relevance", ScoreAbove: floatPtr(0.8), HasFeedback: &no,
		ExperimentID: "exp-1", Preview: &yes,
	}); err != nil {
		t.Fatal(err)
	}
	if _, err := client.ListSpans(ctx, SpanQuery{Prompt: "support/answer", PromptVersion: 3}); err != nil {
		t.Fatal(err)
	}
	if _, err := client.ListTraces(ctx, TraceQuery{}); err != nil {
		t.Fatal(err)
	}

	mu.Lock()
	defer mu.Unlock()
	want := url.Values{
		"project": {"support"}, "max_duration_ms": {"5000"}, "max_cost": {"0.25"},
		"min_tokens": {"1000"}, "has_score": {"relevance"}, "score_above": {"0.8"},
		"has_feedback": {"false"}, "experiment_id": {"exp-1"}, "preview": {"true"},
	}
	if !reflect.DeepEqual(captured[0], want) {
		t.Errorf("trace query %v, want %v", captured[0], want)
	}
	want = url.Values{"project": {"support"}, "prompt": {"support/answer"}, "prompt_version": {"3"}}
	if !reflect.DeepEqual(captured[1], want) {
		t.Errorf("span query %v, want %v", captured[1], want)
	}
	if !reflect.DeepEqual(captured[2], url.Values{"project": {"support"}}) {
		t.Errorf("an empty trace query sent %v", captured[2])
	}
}
