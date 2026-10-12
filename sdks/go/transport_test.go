package mcpd

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"testing"
	"time"
)

// recordingHandler answers with a scripted list of statuses and remembers every
// request it saw.
type recordingHandler struct {
	mu       sync.Mutex
	statuses []int
	requests []*http.Request
	bodies   [][]byte
}

func (h *recordingHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	body, _ := readRequestBody(r)

	h.mu.Lock()
	h.requests = append(h.requests, r.Clone(context.Background()))
	h.bodies = append(h.bodies, body)
	status := http.StatusOK
	if len(h.statuses) > 0 {
		status, h.statuses = h.statuses[0], h.statuses[1:]
	}
	h.mu.Unlock()

	if status != http.StatusOK {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		w.Write([]byte(`{"error":"unavailable","message":"storage is catching up"}`))
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(IngestResult{Accepted: 1})
}

func (h *recordingHandler) count() int {
	h.mu.Lock()
	defer h.mu.Unlock()
	return len(h.requests)
}

func (h *recordingHandler) keys() []string {
	h.mu.Lock()
	defer h.mu.Unlock()
	out := make([]string, 0, len(h.bodies))
	for _, body := range h.bodies {
		var batch IngestBatch
		if json.Unmarshal(body, &batch) == nil && batch.IngestKey != nil {
			out = append(out, *batch.IngestKey)
		}
	}
	return out
}

func TestRetriesARetryableStatusAndReusesTheIngestKey(t *testing.T) {
	handler := &recordingHandler{statuses: []int{
		http.StatusServiceUnavailable,
		http.StatusServiceUnavailable,
	}}
	server := httptest.NewServer(handler)
	defer server.Close()

	client := New(
		WithEndpoint(server.URL),
		WithEnabled(true),
		WithFlushInterval(time.Hour),
		WithTimeout(2*time.Second),
		WithLogger(nil),
	)
	defer client.Shutdown(context.Background())

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("the third attempt should have succeeded: %v", err)
	}

	if got := handler.count(); got != 3 {
		t.Fatalf("made %d attempts, want 3", got)
	}
	keys := handler.keys()
	if len(keys) != 3 {
		t.Fatalf("only %d of the attempts carried an ingest key", len(keys))
	}
	// Retrying with a fresh key would let mcpd count the same batch twice.
	if keys[0] == "" || keys[0] != keys[1] || keys[1] != keys[2] {
		t.Fatalf("the ingest key changed between attempts: %v", keys)
	}
}

func TestDoesNotRetryAMalformedRequest(t *testing.T) {
	handler := &recordingHandler{statuses: []int{
		http.StatusBadRequest,
		http.StatusBadRequest,
		http.StatusBadRequest,
	}}
	server := httptest.NewServer(handler)
	defer server.Close()

	client := New(
		WithEndpoint(server.URL),
		WithEnabled(true),
		WithFlushInterval(time.Hour),
		WithTimeout(2*time.Second),
		WithLogger(nil),
	)
	defer client.Shutdown(context.Background())

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.End()

	err := client.Flush(ctx)
	if err == nil {
		t.Fatal("a 400 should be reported to a caller that waited for it")
	}
	var apiErr *APIError
	if !errors.As(err, &apiErr) {
		t.Fatalf("error is %T, want *APIError", err)
	}
	if apiErr.Status != http.StatusBadRequest {
		t.Errorf("status is %d", apiErr.Status)
	}
	if apiErr.Message == "" {
		t.Error("the server's reason was thrown away")
	}
	// A malformed batch stays malformed, so one attempt is the right number.
	if got := handler.count(); got != 1 {
		t.Fatalf("made %d attempts at a 400, want 1", got)
	}
}

func TestLargeBatchesAreCompressed(t *testing.T) {
	client, sink := newTestClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.SetInput(strings.Repeat("prompt text ", 400)) // comfortably over 1 KiB
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	sink.mu.Lock()
	encodings := append([]string(nil), sink.encodings...)
	sink.mu.Unlock()
	if len(encodings) != 1 || encodings[0] != "gzip" {
		t.Fatalf("content encodings were %v, want [gzip]", encodings)
	}
	if failures := sink.decodeFailures(); len(failures) > 0 {
		t.Fatalf("the compressed body did not decode: %v", failures)
	}
	if spans := sink.spans(); len(spans) != 1 || len(spans[0].Input) < 1024 {
		t.Fatal("the input did not survive compression")
	}
}

func TestSmallBatchesAreSentUncompressed(t *testing.T) {
	client, sink := newTestClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	sink.mu.Lock()
	defer sink.mu.Unlock()
	if len(sink.encodings) != 1 || sink.encodings[0] != "" {
		t.Fatalf("content encodings were %v, want one empty entry", sink.encodings)
	}
}

func TestParseRetryAfterUnderstandsBothForms(t *testing.T) {
	if got := parseRetryAfter("2"); got != 2*time.Second {
		t.Errorf("seconds form gave %v", got)
	}
	if got := parseRetryAfter(""); got != 0 {
		t.Errorf("an absent header gave %v", got)
	}
	if got := parseRetryAfter("later please"); got != 0 {
		t.Errorf("a malformed header gave %v", got)
	}
	future := time.Now().Add(30 * time.Second).UTC().Format(http.TimeFormat)
	if got := parseRetryAfter(future); got <= 0 || got > 31*time.Second {
		t.Errorf("the date form gave %v", got)
	}
	past := time.Now().Add(-time.Minute).UTC().Format(http.TimeFormat)
	if got := parseRetryAfter(past); got != 0 {
		t.Errorf("a date in the past gave %v", got)
	}
}

func TestBackoffGrowsAndHonoursRetryAfter(t *testing.T) {
	first := backoff(1, 0)
	if first < baseBackoff/2 || first > baseBackoff {
		t.Errorf("the first backoff is %v, want between %v and %v", first, baseBackoff/2, baseBackoff)
	}
	second := backoff(2, 0)
	if second < baseBackoff || second > 2*baseBackoff {
		t.Errorf("the second backoff is %v", second)
	}
	if got := backoff(1, 3*time.Second); got != 3*time.Second {
		t.Errorf("Retry-After was ignored: %v", got)
	}
	if got := backoff(1, time.Hour); got != maxBackoff {
		t.Errorf("an absurd Retry-After was not capped: %v", got)
	}
}

func TestListTracesBuildsTheRequestAndDecodesTheResponse(t *testing.T) {
	var mu sync.Mutex
	var captured *url.URL
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		captured = r.URL
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(TraceListResponse{
			Traces: []Trace{{TraceID: "4bf92f3577b34da6a3ce929d0e0e4736", Name: "support_agent", Status: SpanStatusError}},
			Page:   Page{HasMore: false},
		})
	}))
	defer server.Close()

	client := New(WithEndpoint(server.URL), WithProject("support"), WithLogger(nil))
	defer client.Shutdown(context.Background())

	out, err := client.ListTraces(context.Background(), TraceQuery{
		Status: SpanStatusError,
		Tags:   []string{"billing", "urgent"},
		Limit:  25,
	})
	if err != nil {
		t.Fatalf("list traces: %v", err)
	}
	mu.Lock()
	defer mu.Unlock()
	if captured == nil || captured.Path != "/v1/api/traces" {
		t.Fatalf("requested %v", captured)
	}
	query := captured.Query()
	if query.Get("project") != "support" {
		t.Errorf("project is %q", query.Get("project"))
	}
	if query.Get("status") != "error" {
		t.Errorf("status is %q", query.Get("status"))
	}
	if query.Get("tags") != "billing,urgent" {
		t.Errorf("tags are %q", query.Get("tags"))
	}
	if query.Get("limit") != "25" {
		t.Errorf("limit is %q", query.Get("limit"))
	}
	if query.Has("cursor") {
		t.Error("an empty cursor should not be sent")
	}
	if len(out.Traces) != 1 || out.Traces[0].Name != "support_agent" {
		t.Fatalf("decoded %+v", out.Traces)
	}
}

func TestReadErrorsCarryTheServersReason(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusNotFound)
		w.Write([]byte(`{"error":"not_found","message":"trace abc not found"}`))
	}))
	defer server.Close()

	client := New(WithEndpoint(server.URL), WithLogger(nil))
	defer client.Shutdown(context.Background())

	_, err := client.GetTrace(context.Background(), "abc")
	if err == nil {
		t.Fatal("a 404 was not reported")
	}
	var apiErr *APIError
	if !errors.As(err, &apiErr) {
		t.Fatalf("error is %T, want *APIError", err)
	}
	if apiErr.Code != "not_found" || apiErr.Message != "trace abc not found" {
		t.Fatalf("decoded %+v", apiErr)
	}
}

func TestHealthIsReadable(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/healthz" {
			t.Errorf("health asked for %q", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(HealthStatus{Status: "ok", Version: "0.1.0", SpecVersion: "1.0.0"})
	}))
	defer server.Close()

	client := New(WithEndpoint(server.URL), WithLogger(nil))
	defer client.Shutdown(context.Background())

	health, err := client.Health(context.Background())
	if err != nil {
		t.Fatalf("health: %v", err)
	}
	if health.Status != "ok" || health.SpecVersion != "1.0.0" {
		t.Fatalf("decoded %+v", health)
	}
}

func TestAuthorizationHeaderIsSentWhenAKeyIsSet(t *testing.T) {
	var mu sync.Mutex
	var header string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		header = r.Header.Get("Authorization")
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(HealthStatus{Status: "ok"})
	}))
	defer server.Close()

	client := New(WithEndpoint(server.URL), WithAPIKey("secret-key"), WithLogger(nil))
	defer client.Shutdown(context.Background())

	if _, err := client.Health(context.Background()); err != nil {
		t.Fatalf("health: %v", err)
	}
	mu.Lock()
	defer mu.Unlock()
	if header != "Bearer secret-key" {
		t.Fatalf("authorization header was %q", header)
	}
}

// Each read method builds its own request and decodes its own response type.
// The bodies are built from the generated types and serialised, so the test
// breaks if the SDK and the server ever stop agreeing about a field name.
func TestEveryReadMethodBuildsItsRequestAndDecodesItsResponse(t *testing.T) {
	var mu sync.Mutex
	var captured *url.URL
	from := time.Date(2026, 8, 1, 11, 0, 0, 0, time.UTC)
	to := time.Date(2026, 8, 1, 13, 0, 0, 0, time.UTC)

	bodies := map[string]any{
		"/v1/api/spans": SpanListResponse{
			Spans: []Span{{TraceID: "4bf92f3577b34da6a3ce929d0e0e4736", SpanID: "00f067aa0ba902b7", Name: "step", Kind: SpanKindToolCall}},
			Page:  Page{HasMore: false},
		},
		"/v1/api/logs": LogListResponse{
			Logs: []LogRecord{{LogID: "l", Level: LogLevelWarn, Message: "slow"}},
			Page: Page{HasMore: false},
		},
		"/v1/api/metrics": MetricSeriesResponse{
			Name:   "orders.looked_up",
			Series: []MetricSeries{{Points: []MetricSeriesPoint{{Timestamp: from, Value: 1.5}}}},
		},
		"/v1/api/scores": ScoreListResponse{
			Scores: []Score{{ScoreID: "s", ScorerID: "relevance", Source: ScoreSourceLive, Value: 0.9}},
			Page:   Page{HasMore: false},
		},
		"/v1/api/feedback": FeedbackListResponse{
			Feedback: []Feedback{{FeedbackID: "f", Kind: FeedbackKindThumbs, Value: json.RawMessage(`"up"`)}},
			Page:     Page{HasMore: false},
		},
		"/v1/api/analytics/summary": AnalyticsSummary{From: from, To: to, TraceCount: 7, ErrorCount: 1, Currency: "USD"},
		"/v1/api/analytics/cost":    CostBreakdown{From: from, To: to, TotalCost: 0.5, Currency: "USD"},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		captured = r.URL
		mu.Unlock()
		body, ok := bodies[r.URL.Path]
		if !ok {
			t.Errorf("a read hit %q, which no method should request", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(body)
	}))
	defer server.Close()

	client := New(WithEndpoint(server.URL), WithProject("support"), WithLogger(nil))
	defer client.Shutdown(context.Background())
	ctx := context.Background()

	query := func(path string) url.Values {
		t.Helper()
		mu.Lock()
		defer mu.Unlock()
		if captured == nil || captured.Path != path {
			t.Fatalf("requested %v, want %s", captured, path)
		}
		if captured.Query().Get("project") != "support" {
			t.Errorf("%s was read without the client's project: %q", path, captured.RawQuery)
		}
		return captured.Query()
	}

	spans, err := client.ListSpans(ctx, SpanQuery{TraceID: "4bf92f3577b34da6a3ce929d0e0e4736", Kind: SpanKindToolCall, Limit: 10})
	if err != nil {
		t.Fatalf("list spans: %v", err)
	}
	if q := query("/v1/api/spans"); q.Get("trace_id") != "4bf92f3577b34da6a3ce929d0e0e4736" || q.Get("kind") != "tool_call" || q.Get("limit") != "10" {
		t.Errorf("span filters: %q", q.Encode())
	}
	if len(spans.Spans) != 1 || spans.Spans[0].Name != "step" || spans.Spans[0].Kind != SpanKindToolCall {
		t.Errorf("decoded %+v", spans.Spans)
	}

	logs, err := client.ListLogs(ctx, LogQuery{Level: LogLevelWarn, TraceID: "4bf92f3577b34da6a3ce929d0e0e4736"})
	if err != nil {
		t.Fatalf("list logs: %v", err)
	}
	if q := query("/v1/api/logs"); q.Get("level") != "warn" || q.Get("trace_id") == "" {
		t.Errorf("log filters: %q", q.Encode())
	}
	if len(logs.Logs) != 1 || logs.Logs[0].Message != "slow" || logs.Logs[0].Level != LogLevelWarn {
		t.Errorf("decoded %+v", logs.Logs)
	}

	metrics, err := client.QueryMetrics(ctx, MetricQuery{Name: "orders.looked_up", From: from, To: to, Bucket: "1h", Agg: "sum", GroupBy: []string{"outcome", "model"}, Filter: map[string]any{"status": "ok", "model": "gpt-4o"}})
	if err != nil {
		t.Fatalf("query metrics: %v", err)
	}
	if q := query("/v1/api/metrics"); q.Get("name") != "orders.looked_up" || q.Get("bucket") != "1h" || q.Get("agg") != "sum" || q.Get("group_by") != "outcome,model" || q.Get("from") != "2026-08-01T11:00:00Z" {
		t.Errorf("metric filters: %q", q.Encode())
	}
	// The daemon reads key:value pairs. JSON here matched no point at all.
	if q := query("/v1/api/metrics"); q.Get("filter") != "model:gpt-4o,status:ok" {
		t.Errorf("metric filter went out as %q", q.Get("filter"))
	}
	for _, bad := range []map[string]any{{"a:b": "x"}, {"a,b": "x"}, {"": "x"}, {"model": "a,b"}} {
		if _, err := client.QueryMetrics(ctx, MetricQuery{Name: "orders.looked_up", From: from, To: to, Filter: bad}); err == nil {
			t.Errorf("filter %v cannot be written as key:value pairs and should be refused", bad)
		}
	}
	if metrics.Name != "orders.looked_up" || len(metrics.Series) != 1 || metrics.Series[0].Points[0].Value != 1.5 {
		t.Errorf("decoded %+v", metrics)
	}
	if _, err := client.QueryMetrics(ctx, MetricQuery{From: from, To: to}); err == nil {
		t.Error("a metric query with no name should be refused before it is sent")
	}

	scores, err := client.ListScores(ctx, ScoreQuery{ScorerID: "relevance", Source: ScoreSourceLive, MinValue: floatPtr(0.5)})
	if err != nil {
		t.Fatalf("list scores: %v", err)
	}
	if q := query("/v1/api/scores"); q.Get("scorer_id") != "relevance" || q.Get("source") != "live" || q.Get("min_value") != "0.5" || q.Has("max_value") {
		t.Errorf("score filters: %q", q.Encode())
	}
	if len(scores.Scores) != 1 || scores.Scores[0].ScorerID != "relevance" || scores.Scores[0].Value != 0.9 {
		t.Errorf("decoded %+v", scores.Scores)
	}

	feedback, err := client.ListFeedback(ctx, FeedbackQuery{TraceID: "0af7651916cd43dd8448eb211c80319c", Limit: 5})
	if err != nil {
		t.Fatalf("list feedback: %v", err)
	}
	// The daemon answers 400 to any other key, so these three are all.
	if q := query("/v1/api/feedback"); q.Encode() != "limit=5&project=support&trace_id=0af7651916cd43dd8448eb211c80319c" {
		t.Errorf("feedback filters: %q", q.Encode())
	}
	if _, err := client.ListFeedback(ctx, FeedbackQuery{Limit: 5}); err == nil {
		t.Error("a feedback list with no trace id should be refused before it is sent")
	}
	if len(feedback.Feedback) != 1 || feedback.Feedback[0].Kind != FeedbackKindThumbs || string(feedback.Feedback[0].Value) != `"up"` {
		t.Errorf("decoded %+v", feedback.Feedback)
	}

	summary, err := client.AnalyticsSummary(ctx, from, to, "model")
	if err != nil {
		t.Fatalf("analytics summary: %v", err)
	}
	if q := query("/v1/api/analytics/summary"); q.Get("group_by") != "model" || q.Get("to") != "2026-08-01T13:00:00Z" {
		t.Errorf("summary filters: %q", q.Encode())
	}
	if summary.TraceCount != 7 || summary.ErrorCount != 1 {
		t.Errorf("decoded %+v", summary)
	}

	cost, err := client.AnalyticsCost(ctx, from, to, "model", "1h")
	if err != nil {
		t.Fatalf("analytics cost: %v", err)
	}
	if q := query("/v1/api/analytics/cost"); q.Get("group_by") != "model" || q.Get("bucket") != "1h" {
		t.Errorf("cost filters: %q", q.Encode())
	}
	if cost.TotalCost != 0.5 || cost.Currency != "USD" {
		t.Errorf("decoded %+v", cost)
	}
}

func floatPtr(v float64) *float64 { return &v }

// The contract names four statuses worth retrying. 503 has its own test above;
// this one walks the other three through the client, so the policy is proved
// for the batch a flush sends and not only for the transport in isolation, and
// then proves the fourth failure is the one that ends it.
func TestTheClientRetries429And502And504AndGivesUpAfterTheThirdFailure(t *testing.T) {
	for _, status := range []int{http.StatusTooManyRequests, http.StatusBadGateway, http.StatusGatewayTimeout} {
		t.Run(http.StatusText(status), func(t *testing.T) {
			handler := &recordingHandler{statuses: []int{status, status}}
			server := httptest.NewServer(handler)
			defer server.Close()

			client := New(WithEndpoint(server.URL), WithEnabled(true), WithFlushInterval(time.Hour),
				WithTimeout(2*time.Second), WithLogger(nil))
			defer client.Shutdown(context.Background())

			ctx := context.Background()
			_, span := client.Start(ctx, "step")
			span.End()
			if err := client.Flush(ctx); err != nil {
				t.Fatalf("the third attempt should have succeeded after two %d replies: %v", status, err)
			}
			if got := handler.count(); got != 3 {
				t.Fatalf("made %d attempts, want 3", got)
			}
			keys := handler.keys()
			if len(keys) != 3 || keys[0] != keys[1] || keys[1] != keys[2] {
				t.Fatalf("the ingest key changed between attempts: %v", keys)
			}
		})
	}

	t.Run("gives up after the third failure", func(t *testing.T) {
		handler := &recordingHandler{statuses: []int{
			http.StatusBadGateway, http.StatusBadGateway, http.StatusBadGateway, http.StatusBadGateway,
		}}
		server := httptest.NewServer(handler)
		defer server.Close()

		client := New(WithEndpoint(server.URL), WithEnabled(true), WithFlushInterval(time.Hour),
			WithTimeout(2*time.Second), WithLogger(nil))
		defer client.Shutdown(context.Background())

		ctx := context.Background()
		_, span := client.Start(ctx, "step")
		span.End()
		if err := client.Flush(ctx); err == nil {
			t.Fatal("a batch that never landed should be reported by flush")
		}
		// Three attempts, not four and not forever: the fourth 502 is never
		// asked for, and the batch is dropped rather than held.
		if got := handler.count(); got != 3 {
			t.Fatalf("made %d attempts, want 3", got)
		}
		if stats := client.Stats(); stats.Queued != 0 {
			t.Fatalf("a batch that cannot land is held in the queue: %d queued", stats.Queued)
		}
	})
}
