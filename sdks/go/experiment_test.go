package mcpd

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// experimentServer plays the daemon's experiment endpoints and remembers every
// request, so a test can say exactly what reached the wire.
type experimentServer struct {
	mu       sync.Mutex
	requests []experimentRequest
	items    []DatasetItem
	// itemsStatus, when set, is the answer to the items read.
	itemsStatus int
}

type experimentRequest struct {
	method string
	path   string
	query  url.Values
	body   map[string]any
}

func (s *experimentServer) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	raw, _ := io.ReadAll(r.Body)
	var body map[string]any
	if len(raw) > 0 {
		json.Unmarshal(raw, &body)
	}
	s.mu.Lock()
	s.requests = append(s.requests, experimentRequest{r.Method, r.URL.EscapedPath(), r.URL.Query(), body})
	s.mu.Unlock()

	w.Header().Set("Content-Type", "application/json")
	path := r.URL.EscapedPath()
	switch {
	case r.Method == http.MethodPost && path == "/v1/api/experiments":
		// A copy, so the recorded body stays what the client sent.
		answer := map[string]any{}
		for k, v := range body {
			answer[k] = v
		}
		if answer["experiment_id"] == "" {
			answer["experiment_id"] = "exp-1"
		}
		json.NewEncoder(w).Encode(answer)
	case strings.HasPrefix(path, "/v1/api/datasets/"):
		if s.itemsStatus != 0 {
			w.WriteHeader(s.itemsStatus)
			w.Write([]byte(`{"error":{"code":"forbidden","message":"this key cannot read datasets"}}`))
			return
		}
		json.NewEncoder(w).Encode(s.items)
	case strings.HasSuffix(path, "/score"):
		w.Write([]byte(`{"traces_scored":3,"scores_written":3,"skipped":[],"scores":[]}`))
	case strings.HasSuffix(path, "/results"):
		w.Write([]byte(`{"experiment_id":"exp-1","runs":3,"failed":1,` +
			`"items":[{"trace_id":"t1","item_id":"c1","status":"ok","scores":{"exact_match":1}}],` +
			`"scorers":[{"scorer_id":"exact_match","n":3,"mean":0.5,"min":0,"max":1,"direction":"higher_is_better"}]}`))
	case strings.HasSuffix(path, "/trajectory"):
		w.Write([]byte(`{"trace_id":"t1","steps":[],"tool_sequence":["search"],"step_count":2,` +
			`"tool_count":1,"error_count":0,"goal_reached":true}`))
	default:
		w.WriteHeader(http.StatusNotFound)
	}
}

func (s *experimentServer) recorded() []experimentRequest {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]experimentRequest(nil), s.requests...)
}

// experimentPosts is every experiment the runner posted, in order.
func (s *experimentServer) experimentPosts() []map[string]any {
	var out []map[string]any
	for _, r := range s.recorded() {
		if r.method == http.MethodPost && r.path == "/v1/api/experiments" {
			out = append(out, r.body)
		}
	}
	return out
}

func cases(n int) []DatasetItem {
	items := make([]DatasetItem, n)
	for i := range items {
		items[i] = DatasetItem{
			ItemID:    fmt.Sprintf("c%d", i+1),
			DatasetID: "questions",
			Input:     json.RawMessage(fmt.Sprintf(`{"question":"q%d"}`, i+1)),
		}
	}
	return items
}

func newExperimentClient(t *testing.T, server *experimentServer, opts ...Option) (*Client, *RecordingSink) {
	t.Helper()
	daemon := httptest.NewServer(server)
	t.Cleanup(daemon.Close)
	sink := &RecordingSink{}
	base := []Option{
		WithEndpoint(daemon.URL), WithProject("support"), WithSink(sink),
		WithFlushInterval(time.Hour), WithLogger(nil),
	}
	client := New(append(base, opts...)...)
	t.Cleanup(func() { client.Shutdown(context.Background()) })
	return client, sink
}

// finalSpans keeps the last record of each span, since a span is exported on
// start and again on end.
func finalSpans(sink *RecordingSink) map[string]Span {
	out := map[string]Span{}
	for _, s := range sink.Spans() {
		out[s.SpanID] = s
	}
	return out
}

func TestExperimentReadsAndWritesUseTheirPaths(t *testing.T) {
	server := &experimentServer{items: cases(2)}
	client, _ := newExperimentClient(t, server)
	ctx := context.Background()

	trajectory, err := client.GetTrajectory(ctx, "t/1")
	if err != nil || trajectory.StepCount != 2 || trajectory.ToolSequence[0] != "search" {
		t.Fatalf("trajectory %+v, %v", trajectory, err)
	}
	items, err := client.ListDatasetItems(ctx, "support qs", 50)
	if err != nil || len(items) != 2 || string(items[0].Input) != `{"question":"q1"}` {
		t.Fatalf("items %+v, %v", items, err)
	}
	created, err := client.CreateExperiment(ctx, Experiment{
		Name: "v2", DatasetID: "support qs", Status: ExperimentStatusRunning,
		Metadata: json.RawMessage(`{"promptVersion":4,"Camel_Key":true}`),
	})
	if err != nil || created.ExperimentID != "exp-1" {
		t.Fatalf("created %+v, %v", created, err)
	}
	if _, err := client.ScoreExperiment(ctx, "exp/1", []string{"exact_match"}, &ScorerConfig{
		JSONSchema: json.RawMessage(`{"type":"object","minLength":1}`),
	}); err != nil {
		t.Fatal(err)
	}
	if _, err := client.ScoreExperiment(ctx, "exp-1", nil, nil); err != nil {
		t.Fatal(err)
	}
	results, err := client.ExperimentResults(ctx, "exp/1")
	if err != nil || results.Scorers[0].ScorerID != "exact_match" ||
		string(results.Items[0].Scores) != `{"exact_match":1}` {
		t.Fatalf("results %+v, %v", results, err)
	}

	got := server.recorded()
	project := url.Values{"project": {"support"}}
	want := []struct {
		method, path string
		query        url.Values
	}{
		{"GET", "/v1/api/traces/t%2F1/trajectory", project},
		{"GET", "/v1/api/datasets/support%20qs/items", url.Values{"limit": {"50"}}},
		{"POST", "/v1/api/experiments", url.Values{}},
		{"POST", "/v1/api/experiments/exp%2F1/score", project},
		{"POST", "/v1/api/experiments/exp-1/score", project},
		{"GET", "/v1/api/experiments/exp%2F1/results", project},
	}
	if len(got) != len(want) {
		t.Fatalf("%d requests, want %d", len(got), len(want))
	}
	for i, w := range want {
		if got[i].method != w.method || got[i].path != w.path || !reflect.DeepEqual(got[i].query, w.query) {
			t.Errorf("request %d: %s %s %v, want %s %s %v", i, got[i].method, got[i].path, got[i].query,
				w.method, w.path, w.query)
		}
	}
	posted := got[2].body
	if posted["correlation"].(map[string]any)["project"] != "support" {
		t.Errorf("the experiment was posted without the client's project: %v", posted)
	}
	if !reflect.DeepEqual(posted["metadata"], map[string]any{"promptVersion": float64(4), "Camel_Key": true}) {
		t.Errorf("metadata changed on the wire: %v", posted["metadata"])
	}
	score := got[3].body
	if !reflect.DeepEqual(score["scorers"], []any{"exact_match"}) {
		t.Errorf("score body %v", score)
	}
	config := score["config"].(map[string]any)
	if !reflect.DeepEqual(config["json_schema"], map[string]any{"type": "object", "minLength": float64(1)}) ||
		!reflect.DeepEqual(config["forbidden"], []any{}) {
		t.Errorf("scorer config on the wire %v", config)
	}
	if !reflect.DeepEqual(got[4].body, map[string]any{"scorers": []any{}}) {
		t.Errorf("a bare score body %v, want only an empty scorers list", got[4].body)
	}
}

func TestRunExperimentRecordsEveryCaseAndScoresThem(t *testing.T) {
	server := &experimentServer{items: cases(3)}
	// A sample rate of zero would drop every case if the runner sampled them.
	client, sink := newExperimentClient(t, server, WithSampleRate(0))
	ctx := context.Background()

	var calls []string
	var callsMu sync.Mutex
	target := func(ctx context.Context, input json.RawMessage) (any, error) {
		var in struct{ Question string }
		json.Unmarshal(input, &in)
		callsMu.Lock()
		calls = append(calls, in.Question)
		callsMu.Unlock()
		_, child := client.Start(ctx, "lookup", WithSpanKind(SpanKindToolCall))
		child.End()
		if in.Question == "q2" {
			return nil, errors.New("the model refused")
		}
		return map[string]string{"answer": "a-" + in.Question}, nil
	}
	results, err := client.RunExperiment(ctx, "questions", target,
		ExperimentName("answers-v2"), ExperimentScorers("exact_match"),
		ExperimentConcurrency(1), ExperimentMetadata(map[string]any{"prompt": "answer@4"}))
	if err != nil {
		t.Fatal(err)
	}
	if results.ExperimentID != "exp-1" || results.Scorers[0].Mean != 0.5 {
		t.Errorf("results %+v", results)
	}
	if !reflect.DeepEqual(calls, []string{"q1", "q2", "q3"}) {
		t.Errorf("target called with %v; the case after the failing one must still run", calls)
	}

	roots := map[string]Span{}
	var children []Span
	for _, s := range finalSpans(sink) {
		if s.IsRoot {
			roots[attrsOf(t, s)[DatasetItemAttribute].(string)] = s
		} else {
			children = append(children, s)
		}
	}
	if len(roots) != 3 || len(children) != 3 {
		t.Fatalf("%d case spans and %d children, want 3 and 3", len(roots), len(children))
	}
	for id, root := range roots {
		if root.Name != "answers-v2" || root.Kind != SpanKindAgentRun || root.Correlation.ExperimentID == nil ||
			*root.Correlation.ExperimentID != "exp-1" {
			t.Errorf("case %s span %+v", id, root)
		}
	}
	if string(roots["c1"].Input) != `{"question":"q1"}` || string(roots["c1"].Output) != `{"answer":"a-q1"}` ||
		roots["c1"].Status != SpanStatusOk {
		t.Errorf("case c1: input %s output %s status %s", roots["c1"].Input, roots["c1"].Output, roots["c1"].Status)
	}
	if failed := roots["c2"]; failed.Status != SpanStatusError || failed.Error == nil ||
		failed.Error.Message != "the model refused" {
		t.Errorf("the failing case: status %s error %+v", failed.Status, failed.Error)
	}
	if roots["c3"].Status != SpanStatusOk {
		t.Errorf("the case after the failure ended %s", roots["c3"].Status)
	}
	parents := map[string]bool{}
	for _, child := range children {
		if child.ParentSpanID != nil {
			parents[*child.ParentSpanID] = true
		}
	}
	for id, root := range roots {
		if !parents[root.SpanID] {
			t.Errorf("the target's span is not a child of case %s", id)
		}
	}

	posts := server.experimentPosts()
	if len(posts) != 2 {
		t.Fatalf("%d experiment posts, want running then succeeded", len(posts))
	}
	first, last := posts[0], posts[1]
	if first["status"] != "running" || first["experiment_id"] != "" || first["name"] != "answers-v2" ||
		first["dataset_id"] != "questions" || first["started_at"] == nil ||
		!reflect.DeepEqual(first["metadata"], map[string]any{"prompt": "answer@4"}) {
		t.Errorf("first post %v", first)
	}
	if last["status"] != "succeeded" || last["experiment_id"] != "exp-1" || last["item_count"] != float64(3) ||
		last["finished_at"] == nil || !reflect.DeepEqual(last["summary"], map[string]any{"exact_match": 0.5}) ||
		last["started_at"] != first["started_at"] {
		t.Errorf("last post %v", last)
	}
	var score map[string]any
	for _, r := range server.recorded() {
		if strings.HasSuffix(r.path, "/score") {
			score = r.body
		}
	}
	if !reflect.DeepEqual(score, map[string]any{"scorers": []any{"exact_match"}}) {
		t.Errorf("score body %v", score)
	}
}

func TestRunExperimentKeepsAtMostConcurrencyTargetsInFlight(t *testing.T) {
	server := &experimentServer{items: cases(12)}
	client, sink := newExperimentClient(t, server)
	const concurrency = 3

	var inFlight, peak, calls atomic.Int32
	target := func(ctx context.Context, input json.RawMessage) (any, error) {
		calls.Add(1)
		now := inFlight.Add(1)
		for {
			seen := peak.Load()
			if now <= seen || peak.CompareAndSwap(seen, now) {
				break
			}
		}
		time.Sleep(20 * time.Millisecond)
		inFlight.Add(-1)
		return "ok", nil
	}
	if _, err := client.RunExperiment(context.Background(), "questions", target,
		ExperimentConcurrency(concurrency)); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 12 || peak.Load() != concurrency {
		t.Errorf("%d calls with %d in flight at most, want 12 with %d", calls.Load(), peak.Load(), concurrency)
	}
	if n := len(finalSpans(sink)); n != 12 {
		t.Errorf("%d case spans, want 12", n)
	}
}

func TestRunExperimentPostsFailedWhenTheCasesCannotBeRead(t *testing.T) {
	server := &experimentServer{itemsStatus: http.StatusForbidden}
	client, _ := newExperimentClient(t, server)

	called := false
	_, err := client.RunExperiment(context.Background(), "questions",
		func(context.Context, json.RawMessage) (any, error) { called = true; return nil, nil })
	var apiErr *APIError
	if !errors.As(err, &apiErr) || apiErr.Status != http.StatusForbidden {
		t.Fatalf("error %v, want the 403", err)
	}
	if called {
		t.Error("the target ran with no cases read")
	}
	posts := server.experimentPosts()
	if len(posts) != 2 {
		t.Fatalf("%d experiment posts, want running then failed", len(posts))
	}
	failed := posts[1]
	if failed["status"] != "failed" || failed["experiment_id"] != "exp-1" || failed["finished_at"] == nil ||
		failed["error"] != err.Error() {
		t.Errorf("failed post %v, error %q", failed, err)
	}
	for _, r := range server.recorded() {
		if strings.HasSuffix(r.path, "/score") {
			t.Error("a run that read no cases was scored")
		}
	}
}

func TestRunExperimentRefusesBeforeAnyRequest(t *testing.T) {
	target := func(context.Context, json.RawMessage) (any, error) { return nil, nil }
	tests := []struct {
		name string
		opts []Option
		run  []ExperimentOption
		want string
	}{
		{"concurrency 0", nil, []ExperimentOption{ExperimentConcurrency(0)}, "at least 1, got 0"},
		{"disabled client", []Option{WithEnabled(false)}, nil, "needs an enabled client"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			server := &experimentServer{items: cases(1)}
			client, _ := newExperimentClient(t, server, tc.opts...)
			_, err := client.RunExperiment(context.Background(), "questions", target, tc.run...)
			if err == nil || !strings.Contains(err.Error(), tc.want) {
				t.Errorf("error %v, want one containing %q", err, tc.want)
			}
			if n := len(server.recorded()); n != 0 {
				t.Errorf("%d requests were sent", n)
			}
		})
	}
}
