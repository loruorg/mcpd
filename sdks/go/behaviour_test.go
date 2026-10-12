package mcpd

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"math/rand"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// The tests in this file cover behaviours B1 to B10 of sdks/SDK-CONTRACT.md
// that the rest of the Go suite does not reach: what happens when the daemon
// is hostile rather than merely absent, what happens when many goroutines
// record at once, and what happens at the edges of a span's life.

// ------------------------------------------------------------- test doubles ---

// rawSink keeps the exact bytes of every request as well as the decoded batch,
// because two of the behaviours here (nanosecond timestamps, UTC) are only
// observable on the wire and are erased by decoding into a time.Time.
type rawSink struct {
	mu      sync.Mutex
	bodies  [][]byte
	batches []IngestBatch
}

func (s *rawSink) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	body, err := readRequestBody(r)
	var batch IngestBatch
	if err == nil {
		err = json.Unmarshal(body, &batch)
	}
	if err == nil {
		s.mu.Lock()
		s.bodies = append(s.bodies, body)
		s.batches = append(s.batches, batch)
		s.mu.Unlock()
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(IngestResult{Accepted: uint32(len(batch.Spans))})
}

func (s *rawSink) allSpans() []Span {
	s.mu.Lock()
	defer s.mu.Unlock()
	var out []Span
	for _, b := range s.batches {
		out = append(out, b.Spans...)
	}
	return out
}

// firstBytes keeps a failure message readable when the body it is quoting is a
// whole ingest batch.
func firstBytes(s string) string {
	if len(s) <= 240 {
		return s
	}
	return s[:240] + "..."
}

func (s *rawSink) rawJSON() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	var b strings.Builder
	for _, body := range s.bodies {
		b.Write(body)
	}
	return b.String()
}

// newRawClient is newTestClient's sibling for the cases that need the raw
// bytes. The batch size is deliberately far above anything a test records, so
// the worker never wakes on its own and every export happens where the test
// asked for one.
func newRawClient(t *testing.T, opts ...Option) (*Client, *rawSink) {
	t.Helper()
	sink := &rawSink{}
	server := httptest.NewServer(sink)
	t.Cleanup(server.Close)

	base := []Option{
		WithEndpoint(server.URL),
		WithProject("behaviour"),
		WithEnabled(true),
		WithFlushInterval(time.Hour),
		WithBatchSize(4096),
		WithTimeout(2 * time.Second),
		WithLogger(nil),
	}
	client := New(append(base, opts...)...)
	t.Cleanup(func() { client.Shutdown(context.Background()) })
	return client, sink
}

// stopClient shuts a client down under a deadline. Several tests here point a
// client at a server that answers slowly or not at all, and Shutdown waits for
// the worker: without a deadline the test hangs rather than failing.
func stopClient(t *testing.T, client *Client) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_ = client.Shutdown(ctx)
}

// -------------------------------------------------- B3, bounded memory ---

// Recording after Shutdown is the one path where the queue fills with no worker
// to drain it, so it is where the bound and the eviction order can be observed
// end to end through the public API rather than on the ring in isolation.
func TestTheQueueStaysBoundedAndEvictsTheOldestWhenNothingDrainsIt(t *testing.T) {
	client, _ := newRawClient(t, WithBatchSize(4), WithMaxQueue(8))
	if err := client.Shutdown(context.Background()); err != nil {
		t.Fatalf("shutdown: %v", err)
	}

	ctx := context.Background()
	const recorded = 100
	for i := 0; i < recorded; i++ {
		// Left unended on purpose: an unended span occupies exactly one queue
		// entry, so the arithmetic below is about the bound and nothing else.
		client.Start(ctx, fmt.Sprintf("step-%02d", i))
	}

	stats := client.Stats()
	if stats.Queued != 8 {
		t.Fatalf("the queue holds %d signals, want the bound of 8", stats.Queued)
	}
	if stats.Dropped != recorded-8 {
		t.Fatalf("dropped %d, want %d", stats.Dropped, recorded-8)
	}

	held := client.q.drain(0)
	if len(held) != 8 {
		t.Fatalf("drained %d entries, want 8", len(held))
	}
	// B3 says the OLDEST go, which is the opposite of what a queue usually
	// does. The survivors are therefore the last eight recorded, in order.
	for i, s := range held {
		want := fmt.Sprintf("step-%02d", recorded-8+i)
		if got := s.span.snapshot().Name; got != want {
			t.Errorf("position %d holds %q, want %q", i, got, want)
		}
	}
}

// WithMaxQueue below the batch size would make the queue evict a batch it was
// about to send, so New raises it. Nothing else asserts that clamp, and it is
// what keeps the previous test's arithmetic meaningful.
func TestAMaxQueueSmallerThanTheBatchSizeIsRaisedToIt(t *testing.T) {
	client := New(WithEnabled(false), WithLogger(nil), WithBatchSize(64), WithMaxQueue(4))
	defer client.Shutdown(context.Background())
	if client.cfg.maxQueue != 64 {
		t.Fatalf("max queue is %d, want it raised to the batch size of 64", client.cfg.maxQueue)
	}
	if client.q.max != 64 {
		t.Fatalf("the ring was built with a bound of %d, want 64", client.q.max)
	}
}

// A span evicted from the queue is released, so ending it later enqueues the
// finished record instead of being swallowed. queue_test proves the flag on the
// ring; this proves the finished record actually reaches the server.
func TestASpanEvictedFromTheQueueIsStillExportedWhenItEnds(t *testing.T) {
	sink := &rawSink{}
	release := make(chan struct{})
	var releaseOnce sync.Once
	letGo := func() { releaseOnce.Do(func() { close(release) }) }

	var blockFirst sync.Once
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// The first request never answers, which parks the worker and leaves
		// the queue with nothing draining it.
		blockFirst.Do(func() { <-release })
		sink.ServeHTTP(w, r)
	}))
	t.Cleanup(server.Close)
	t.Cleanup(letGo)

	client := New(
		WithEndpoint(server.URL),
		WithEnabled(true),
		WithBatchSize(4),
		WithMaxQueue(8),
		WithFlushInterval(time.Hour),
		WithTimeout(10*time.Second),
		WithLogger(nil),
	)

	ctx := context.Background()
	// Park the worker inside a request that will not answer, so nothing drains
	// the queue while it is being overflowed below.
	for i := 0; i < 4; i++ {
		client.Start(ctx, "warmup")
	}
	waitFor(t, "the worker to take the first batch", func() bool { return client.q.len() == 0 })

	_, evicted := client.Start(ctx, "evicted")
	for i := 0; i < 8; i++ {
		client.Start(ctx, "filler")
	}
	if client.Stats().Dropped == 0 {
		t.Fatal("nothing was evicted, so this test proves nothing")
	}
	if evicted.pending.Load() {
		t.Fatal("the evicted span was left marked as queued")
	}

	evicted.End()
	letGo()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	stopClient(t, client)

	finished := 0
	for _, span := range sink.allSpans() {
		if span.Name == "evicted" && span.EndTime != nil {
			finished++
		}
	}
	if finished != 1 {
		t.Fatalf("the evicted span produced %d finished records, want exactly 1", finished)
	}
}

// -------------------------------------------------- B2, never blocking ---

// The contract's promise is that an agent's latency does not depend on mcpd
// being fast. A server that answers every request slowly must therefore cost
// the calling goroutine nothing.
func TestRecordingDoesNotWaitForASlowServer(t *testing.T) {
	const serverDelay = 300 * time.Millisecond
	stop := make(chan struct{})
	var stopOnce sync.Once
	letGo := func() { stopOnce.Do(func() { close(stop) }) }

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		select {
		case <-time.After(serverDelay):
		case <-stop:
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(IngestResult{Accepted: 1})
	}))

	// A batch size of one makes the worker try to export on every single span,
	// so the slow server is in the way of the whole run, not only its end.
	client := New(
		WithEndpoint(server.URL),
		WithEnabled(true),
		WithBatchSize(1),
		WithFlushInterval(time.Hour),
		WithTimeout(5*time.Second),
		WithLogger(nil),
	)
	// Registered last, so the server is let go first and the shutdown below
	// measures nothing.
	t.Cleanup(server.Close)
	t.Cleanup(func() { stopClient(t, client) })
	t.Cleanup(letGo)

	const spans = 200
	ctx := context.Background()
	started := time.Now()
	// One server round trip is the budget for all 200 spans. It is checked
	// inside the loop so a blocking implementation fails here rather than
	// running for a minute first.
	budget := started.Add(serverDelay)
	for i := 0; i < spans; i++ {
		_, span := client.Start(ctx, "step")
		span.SetInput(map[string]any{"i": i})
		span.End()
		client.Log(ctx, LogLevelInfo, "recorded", nil)
		if time.Now().After(budget) {
			t.Fatalf("recording blocked: %d of %d spans took longer than one server round trip of %v",
				i+1, spans, serverDelay)
		}
	}
	if elapsed := time.Since(started); elapsed > serverDelay {
		t.Fatalf("recording %d spans took %v, longer than one round trip of %v",
			spans, elapsed, serverDelay)
	}
}

// ------------------------------------------- B1, a hostile daemon ---

// hangingHandler blocks until the test releases it, which is how a daemon that
// accepted the connection and then stopped answering behaves.
type hangingHandler struct{ release chan struct{} }

func (h *hangingHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	<-h.release
}

// Every one of these is a real failure mode of a daemon that is up but not
// well. None of them may panic, block the caller, or lose the client's ability
// to keep recording afterwards.
func TestAHostileDaemonNeverPanicsAndNeverBlocksTheCaller(t *testing.T) {
	cases := []struct {
		name    string
		handler http.HandlerFunc
	}{
		{"an unhandled server error", func(w http.ResponseWriter, r *http.Request) {
			http.Error(w, "boom", http.StatusInternalServerError)
		}},
		{"a body that is not JSON", func(w http.ResponseWriter, r *http.Request) {
			w.Header().Set("Content-Type", "application/json")
			w.Write([]byte("<html>the proxy ate it</html>"))
		}},
		{"a JSON body cut off part way", func(w http.ResponseWriter, r *http.Request) {
			w.Header().Set("Content-Type", "application/json")
			w.Write([]byte(`{"accepted": 3, "rejec`))
		}},
		{"a connection closed mid-response", func(w http.ResponseWriter, r *http.Request) {
			hijacker, ok := w.(http.Hijacker)
			if !ok {
				t.Error("the test server does not support hijacking")
				return
			}
			conn, _, err := hijacker.Hijack()
			if err != nil {
				t.Errorf("hijack: %v", err)
				return
			}
			conn.Write([]byte("HTTP/1.1 200 OK\r\nContent-Length: 64\r\n\r\n{\"acce"))
			conn.Close()
		}},
		{"a redirect to nowhere", func(w http.ResponseWriter, r *http.Request) {
			http.Redirect(w, r, "http://127.0.0.1:1/gone", http.StatusFound)
		}},
		{"an empty 200 with no body at all", func(w http.ResponseWriter, r *http.Request) {
			w.WriteHeader(http.StatusOK)
		}},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			server := httptest.NewServer(tc.handler)
			defer server.Close()

			client := New(
				WithEndpoint(server.URL),
				WithEnabled(true),
				WithBatchSize(8),
				WithFlushInterval(20*time.Millisecond),
				WithTimeout(200*time.Millisecond),
				WithLogger(nil),
			)

			done := make(chan struct{})
			go func() {
				defer close(done)
				ctx := context.Background()
				for i := 0; i < 20; i++ {
					_, span := client.Start(ctx, "step")
					span.SetInput("prompt")
					span.RecordError(errors.New("upstream refused"))
					span.End()
					client.Metric(ctx, "steps", 1, MetricKindCounter)
				}
				// The error, when there is one, belongs to the caller who asked
				// to wait. It is never a reason to fail the run.
				bounded, cancel := context.WithTimeout(ctx, 5*time.Second)
				defer cancel()
				_ = client.Flush(bounded)
				_ = client.Shutdown(bounded)
			}()

			select {
			case <-done:
			case <-time.After(30 * time.Second):
				t.Fatal("recording against this server never returned")
			}
		})
	}
}

// A daemon that accepts the connection and then says nothing is worse than one
// that is down, because the socket looks healthy. The caller must still be
// unaffected, and Shutdown must still return once its context expires.
func TestADaemonThatAcceptsAndThenGoesSilentDoesNotTrapTheCaller(t *testing.T) {
	handler := &hangingHandler{release: make(chan struct{})}
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	t.Cleanup(func() { close(handler.release) })

	client := New(
		WithEndpoint(server.URL),
		WithEnabled(true),
		WithBatchSize(2),
		WithFlushInterval(10*time.Millisecond),
		WithTimeout(30*time.Second),
		WithLogger(nil),
	)

	ctx := context.Background()
	started := time.Now()
	for i := 0; i < 500; i++ {
		_, span := client.Start(ctx, "step")
		span.End()
	}
	if elapsed := time.Since(started); elapsed > 2*time.Second {
		t.Fatalf("500 spans took %v against a silent server", elapsed)
	}
	// The bound holds even though nothing has drained since the first batch.
	if queued := client.Stats().Queued; queued > client.cfg.maxQueue {
		t.Fatalf("the queue reached %d, past its bound of %d", queued, client.cfg.maxQueue)
	}

	// Shutdown waits for the worker, and the worker is stuck in a request that
	// will never answer. The caller's context is what gets it back.
	deadline, cancel := context.WithTimeout(ctx, 300*time.Millisecond)
	defer cancel()
	if err := client.Shutdown(deadline); err == nil {
		t.Error("shutdown against a silent server should report the expired context")
	}
}

// A batch that leaves the process and is never answered must not double count
// on the retry, so the key is minted once per batch. transport_test proves the
// key is stable across retries of one attempt; this proves two separate batches
// do not share one.
func TestEachBatchCarriesItsOwnIngestKey(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	for round := 0; round < 3; round++ {
		_, span := client.Start(ctx, "step")
		span.End()
		if err := client.Flush(ctx); err != nil {
			t.Fatalf("flush %d: %v", round, err)
		}
	}

	sink.mu.Lock()
	defer sink.mu.Unlock()
	if len(sink.batches) != 3 {
		t.Fatalf("expected 3 batches, got %d", len(sink.batches))
	}
	seen := map[string]bool{}
	for i, batch := range sink.batches {
		if batch.IngestKey == nil {
			t.Fatalf("batch %d carried no ingest key", i)
		}
		if seen[*batch.IngestKey] {
			t.Fatalf("batch %d reused the key %q", i, *batch.IngestKey)
		}
		seen[*batch.IngestKey] = true
	}
}

// A payload the user handed us that cannot become JSON is their problem to fix,
// and it must surface as a dropped field rather than as a failure in their
// agent (B1).
func TestAPayloadThatCannotBeEncodedIsDroppedNotRaised(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.SetInput(make(chan int))
	span.SetOutput(func() {})
	span.SetMetadata(map[string]any{"ratio": math.NaN()})
	span.SetAttributes(map[string]any{"self": make(chan struct{})})
	span.End()
	client.Log(ctx, LogLevelInfo, "line", map[string]any{"bad": make(chan int)})
	client.Feedback(ctx, TraceTarget(span.TraceID()), FeedbackKindThumbs, make(chan int))

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	spans := sink.allSpans()
	if len(spans) != 1 {
		t.Fatalf("expected the span to be exported anyway, got %d", len(spans))
	}
	got := spans[0]
	if got.Input != nil || got.Output != nil || got.Metadata != nil || got.Attributes != nil {
		t.Errorf("an unencodable payload reached the wire: %+v", got)
	}
	if got.Status != SpanStatusOk {
		t.Errorf("the span ended %q, want ok: a bad payload is not a failed span", got.Status)
	}
	sink.mu.Lock()
	defer sink.mu.Unlock()
	for _, batch := range sink.batches {
		if len(batch.Feedback) != 0 {
			t.Errorf("feedback with an unencodable value was sent: %+v", batch.Feedback)
		}
		for _, record := range batch.Logs {
			// The field map survives with the one value nothing can encode
			// named in place (contract section 4.1). It used to be dropped
			// whole, which cost every other field in the line.
			if string(record.Fields) != `{"bad":"[unencodable chan int]"}` {
				t.Errorf("log fields: %s", record.Fields)
			}
		}
	}
}

// NaN and the infinities have no JSON literal, and json.Marshal refuses the
// whole batch over one. The signal that carries one in a typed number is what
// cannot be sent, so it alone is lost (contract section 4.1).
func TestANumberJSONCannotCarryCostsItselfAndNeverTheBatch(t *testing.T) {
	client, sink := newRawClient(t)
	ctx := context.Background()

	ctx, span := client.Start(ctx, "run")
	temperature, topP := math.NaN(), 0.9
	span.SetModelParameters(ModelParameters{Temperature: &temperature, TopP: &topP})
	client.Metric(ctx, "ratio", math.NaN(), MetricKindGauge)
	client.Metric(ctx, "calls", 1, MetricKindCounter)
	client.Score(ctx, Target{}, "broken", math.Inf(1), 0, 1)
	client.Log(ctx, LogLevelInfo, "beside them", nil)
	span.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	if stats := client.Stats(); stats.Failed != 0 {
		t.Fatalf("a batch was lost: %+v", stats)
	}
	sink.mu.Lock()
	defer sink.mu.Unlock()
	var metrics []string
	var spans, scores, logs int
	for _, batch := range sink.batches {
		for _, m := range batch.Metrics {
			metrics = append(metrics, m.Name)
		}
		for _, s := range batch.Spans {
			spans++
			if s.ModelParameters == nil || s.ModelParameters.Temperature != nil || *s.ModelParameters.TopP != 0.9 {
				t.Errorf("model parameters: %+v", s.ModelParameters)
			}
		}
		scores += len(batch.Scores)
		logs += len(batch.Logs)
	}
	if len(metrics) != 1 || metrics[0] != "calls" || spans == 0 || scores != 0 || logs != 1 {
		t.Errorf("metrics %v, spans %d, scores %d, logs %d", metrics, spans, scores, logs)
	}
}

// A logger the user supplied that panics is still the user's code running
// inside ours. B1 does not get to make an exception for it.
type panickingLogger struct{ calls atomic.Int64 }

func (l *panickingLogger) Printf(format string, args ...any) {
	l.calls.Add(1)
	panic("the user's logger blew up")
}

// Client.logf once called the supplied Logger with no recover, so a Logger that
// panicked took the caller's goroutine with it. Observed by calling Score with
// an empty Target: the panic propagated out of Score. WithLogger is a public
// extension point, so the panic belongs to the SDK to contain.
func TestALoggerThatPanicsDoesNotTakeDownTheCaller(t *testing.T) {
	logger := &panickingLogger{}
	client := New(WithEndpoint("http://127.0.0.1:1"), WithEnabled(true),
		WithFlushInterval(time.Hour), WithTimeout(50*time.Millisecond),
		WithLogger(logger))
	defer client.Shutdown(context.Background())

	// A score with nothing to attach to is logged on the calling goroutine,
	// which is where the panic lands.
	client.Score(context.Background(), Target{}, "helpfulness", 1, 0, 1)
	if logger.calls.Load() == 0 {
		t.Fatal("the logger was never called, so this test proves nothing")
	}
}

// ------------------------------------------------------- concurrency ---

// Many goroutines recording under one root must produce one correct tree: every
// child on the root's trace, every child pointing at the root, and every span
// exported exactly once. Run with -race, this is also the data-race check.
func TestConcurrentRecordingProducesOneCorrectTree(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	ctx, root := client.Start(ctx, "agent", WithSession("sess-9"))

	const workers = 32
	const perWorker = 8
	var wg sync.WaitGroup
	for w := 0; w < workers; w++ {
		wg.Add(1)
		go func(w int) {
			defer wg.Done()
			for i := 0; i < perWorker; i++ {
				childCtx, child := client.Start(ctx, fmt.Sprintf("child-%d-%d", w, i))
				child.SetInput(map[string]any{"worker": w})
				child.SetTokens(uint64(w), uint64(i))
				client.Log(childCtx, LogLevelInfo, "step", nil)
				client.Metric(childCtx, "steps", 1, MetricKindCounter)
				child.End()
			}
		}(w)
	}
	wg.Wait()
	root.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	spans := sink.allSpans()
	if len(spans) != workers*perWorker+1 {
		t.Fatalf("exported %d spans, want %d", len(spans), workers*perWorker+1)
	}
	seen := map[string]bool{}
	children := 0
	for _, span := range spans {
		if seen[span.SpanID] {
			t.Fatalf("span %s (%s) was exported twice", span.SpanID, span.Name)
		}
		seen[span.SpanID] = true
		if span.TraceID != root.TraceID() {
			t.Errorf("span %s is on trace %s, want %s", span.Name, span.TraceID, root.TraceID())
		}
		if span.EndTime == nil {
			t.Errorf("span %s was exported unfinished after everything ended", span.Name)
		}
		if span.Name == "agent" {
			continue
		}
		children++
		if span.ParentSpanID == nil || *span.ParentSpanID != root.SpanID() {
			t.Errorf("child %s points at %v, want the root %s", span.Name, span.ParentSpanID, root.SpanID())
		}
		if span.Correlation.SessionID == nil || *span.Correlation.SessionID != "sess-9" {
			t.Errorf("child %s did not inherit the session", span.Name)
		}
	}
	if children != workers*perWorker {
		t.Fatalf("counted %d children, want %d", children, workers*perWorker)
	}

	sink.mu.Lock()
	logs, metrics := 0, 0
	for _, batch := range sink.batches {
		logs += len(batch.Logs)
		metrics += len(batch.Metrics)
	}
	sink.mu.Unlock()
	if logs != workers*perWorker || metrics != workers*perWorker {
		t.Errorf("got %d logs and %d metrics, want %d of each", logs, metrics, workers*perWorker)
	}
}

// One span written to from every direction at once. The span's own lock is the
// only thing standing between instrumentation and a corrupted record, and the
// invariant is that exactly one finished record comes out.
func TestOneSpanMutatedFromManyGoroutinesEndsExactlyOnce(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "contended")

	var wg sync.WaitGroup
	for w := 0; w < 24; w++ {
		wg.Add(1)
		go func(w int) {
			defer wg.Done()
			switch w % 6 {
			case 0:
				span.SetInput(map[string]any{"w": w})
			case 1:
				span.SetTags(fmt.Sprintf("tag-%d", w))
			case 2:
				span.RecordError(errors.New("concurrent failure"))
			case 3:
				span.SetModel("gpt-4o", "openai")
			case 4:
				_ = span.Traceparent()
				_ = span.IsRecording()
			case 5:
				span.End()
			}
		}(w)
	}
	wg.Wait()
	span.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	spans := sink.allSpans()
	if len(spans) != 1 {
		t.Fatalf("exported %d records for one span, want 1", len(spans))
	}
	got := spans[0]
	if got.EndTime == nil || got.DurationNs == nil {
		t.Fatal("the contended span was exported unfinished")
	}
	// Whether the error landed before or after the end is a race the caller
	// owns. That the status matches whatever did land is ours.
	if got.Error != nil && got.Status != SpanStatusError {
		t.Errorf("an error was recorded but the status is %q", got.Status)
	}
	if got.Error == nil && got.Status != SpanStatusOk {
		t.Errorf("no error was recorded but the status is %q", got.Status)
	}
}

// Flush and Shutdown are documented as safe to call at any time. Calling them
// from many goroutines at once is the case a short-lived process hits when
// several request handlers unwind together.
func TestConcurrentFlushesAndShutdownsAllReturn(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	for i := 0; i < 16; i++ {
		_, span := client.Start(ctx, "step")
		span.End()
	}

	var wg sync.WaitGroup
	errs := make([]error, 24)
	for i := 0; i < 24; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			if i%3 == 0 {
				errs[i] = client.Shutdown(ctx)
				return
			}
			errs[i] = client.Flush(ctx)
		}(i)
	}
	done := make(chan struct{})
	go func() { wg.Wait(); close(done) }()
	select {
	case <-done:
	case <-time.After(20 * time.Second):
		t.Fatal("a concurrent Flush or Shutdown never returned")
	}
	for i, err := range errs {
		if err != nil {
			t.Errorf("call %d returned %v", i, err)
		}
	}
	if len(sink.allSpans()) != 16 {
		t.Fatalf("shutdown lost spans: %d of 16 arrived", len(sink.allSpans()))
	}
}

// ------------------------------------------------- B5, flush and shutdown ---

// Shutdown's whole purpose is that a short-lived process keeps what it
// recorded. Nothing else asserts that the spans actually arrive.
func TestShutdownExportsWhatWasNeverFlushed(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	for i := 0; i < 5; i++ {
		_, span := client.Start(ctx, fmt.Sprintf("step-%d", i))
		span.End()
	}
	if len(sink.allSpans()) != 0 {
		t.Fatal("something exported before shutdown, so this proves nothing")
	}

	if err := client.Shutdown(ctx); err != nil {
		t.Fatalf("shutdown: %v", err)
	}
	spans := sink.allSpans()
	if len(spans) != 5 {
		t.Fatalf("shutdown exported %d of 5 spans", len(spans))
	}
	for _, span := range spans {
		if span.EndTime == nil {
			t.Errorf("%s arrived unfinished", span.Name)
		}
	}
}

// -------------------------------------------------- B8, the span's life ---

// A run that crashes leaves its spans open. They still have to arrive, without
// an end time, because that is exactly the run you need to see.
func TestASpanLeftUnendedIsStillExportedAtShutdown(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	ctx, root := client.Start(ctx, "agent")
	client.Start(ctx, "abandoned")
	root.End()

	if err := client.Shutdown(ctx); err != nil {
		t.Fatalf("shutdown: %v", err)
	}
	byName := map[string]Span{}
	for _, span := range sink.allSpans() {
		byName[span.Name] = span
	}
	open, ok := byName["abandoned"]
	if !ok {
		t.Fatal("a span that was never ended was never exported")
	}
	if open.EndTime != nil || open.DurationNs != nil {
		t.Errorf("an unended span was given an end: %+v", open)
	}
	if open.Status != SpanStatusUnset {
		t.Errorf("an unended span has status %q, want unset", open.Status)
	}
}

// End is documented as idempotent so that `defer span.End()` next to an
// explicit end is safe. The second call must not move the end time, change the
// status, or produce a second record.
func TestASpanEndedTwiceKeepsTheFirstEnding(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.End()
	firstEnd := span.snapshot().EndTime
	firstStatus := span.snapshot().Status

	time.Sleep(2 * time.Millisecond)
	span.EndWithStatus(SpanStatusCancelled)
	span.End()

	if got := span.snapshot().Status; got != firstStatus {
		t.Errorf("the second end changed the status to %q", got)
	}
	if got := span.snapshot().EndTime; got == nil || !got.Equal(*firstEnd) {
		t.Errorf("the second end moved the end time to %v", got)
	}

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	if spans := sink.allSpans(); len(spans) != 1 {
		t.Fatalf("ending twice produced %d records", len(spans))
	}
}

// Setting anything on a span after it has ended must be ignored: the record it
// produced has already left.
func TestMutationAfterEndIsIgnored(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.SetInput("before")
	span.End()

	span.SetInput("after")
	span.SetOutput("after")
	span.RecordError(errors.New("too late"))
	span.SetStatus(SpanStatusCancelled)
	span.SetUser("u-late")

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	spans := sink.allSpans()
	if len(spans) != 1 {
		t.Fatalf("expected 1 span, got %d", len(spans))
	}
	got := spans[0]
	if string(got.Input) != `"before"` {
		t.Errorf("the input became %s after the span ended", got.Input)
	}
	if got.Error != nil || got.Status != SpanStatusOk {
		t.Errorf("a post-end error changed the record: status %q, error %+v", got.Status, got.Error)
	}
	if got.Correlation.UserID != nil {
		t.Errorf("a post-end user reached the wire: %v", got.Correlation.UserID)
	}
}

// Nothing requires a child to outlive its parent, and an agent that fires a
// background step routinely breaks that order. The parentage must survive it.
func TestAChildEndedAfterItsParentKeepsItsParentage(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	ctx, root := client.Start(ctx, "agent")
	_, child := client.Start(ctx, "background_step")
	root.End()
	time.Sleep(time.Millisecond)
	child.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	byName := map[string]Span{}
	for _, span := range sink.allSpans() {
		byName[span.Name] = span
	}
	step, ok := byName["background_step"]
	if !ok {
		t.Fatal("the child was not exported")
	}
	if step.ParentSpanID == nil || *step.ParentSpanID != root.SpanID() {
		t.Errorf("the child lost its parent: %v", step.ParentSpanID)
	}
	if step.EndTime == nil || byName["agent"].EndTime == nil {
		t.Fatal("both spans should be finished")
	}
	if !step.EndTime.After(*byName["agent"].EndTime) {
		t.Error("the child's end time is not after its parent's, so the ordering was not preserved")
	}
}

// Recording continues to be safe after Shutdown, which is what a deferred
// span.End() in main does after a deferred Shutdown has already run.
func TestASpanEndedAfterShutdownIsSafeAndQueuedNotSent(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	_, before := client.Start(ctx, "before")
	if err := client.Shutdown(ctx); err != nil {
		t.Fatalf("shutdown: %v", err)
	}
	sent := len(sink.allSpans())

	before.End()
	_, after := client.Start(ctx, "after")
	after.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush after shutdown: %v", err)
	}
	if err := client.Shutdown(ctx); err != nil {
		t.Fatalf("second shutdown: %v", err)
	}
	// Nothing new can leave, because the worker has gone. Nothing may panic
	// or block either, which is the point.
	if got := len(sink.allSpans()); got != sent {
		t.Errorf("%d spans arrived after shutdown, want the %d from before it", got, sent)
	}
}

// An event is a point in time. It is complete when it is made, so End on it
// must not restate it or move it.
func TestAnEventIsCompleteWhenItIsMadeAndEndDoesNothing(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "agent")
	span.Event("first_token")
	span.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	var event *Span
	for i, s := range sink.allSpans() {
		if s.Name == "first_token" {
			event = &sink.allSpans()[i]
		}
	}
	if event == nil {
		t.Fatal("the event was not exported")
	}
	if !event.IsEvent {
		t.Error("the record is not marked as an event")
	}
	if event.Status != SpanStatusOk {
		t.Errorf("an event has status %q, want ok", event.Status)
	}
	if event.ParentSpanID == nil || *event.ParentSpanID != span.SpanID() {
		t.Errorf("the event is not under its span: %v", event.ParentSpanID)
	}
	if event.EndTime != nil {
		t.Errorf("an event was given an end time: %v", event.EndTime)
	}
}

// ------------------------------------------------------- B4, sampling ---

// deterministicTraceIDs returns ids from a fixed seed, so a sampling test that
// fails does so for everyone and can be reproduced from the failure output.
func deterministicTraceIDs(n int) []string {
	src := rand.New(rand.NewSource(20250828))
	out := make([]string, n)
	for i := range out {
		out[i] = fmt.Sprintf("%016x%016x", src.Uint64(), src.Uint64())
	}
	return out
}

// The whole point of hashing the trace id instead of rolling a die is that two
// processes reach the same answer without talking to each other. That only
// holds if the answer depends on nothing but the id and the rate.
func TestTheSamplingDecisionDependsOnNothingButTheTraceIDAndTheRate(t *testing.T) {
	ids := deterministicTraceIDs(200)
	for _, rate := range []float64{0.1, 0.25, 0.5, 0.75} {
		first := New(WithEnabled(false), WithSampleRate(rate), WithLogger(nil))
		second := New(WithEnabled(false), WithSampleRate(rate), WithLogger(nil))
		for _, id := range ids {
			a, b := first.sampledTrace(id), second.sampledTrace(id)
			if a != b {
				t.Fatalf("two clients at rate %v disagreed about %s: %v and %v", rate, id, a, b)
			}
			if again := first.sampledTrace(id); again != a {
				t.Fatalf("the same client changed its mind about %s at rate %v", id, rate)
			}
		}
		first.Shutdown(context.Background())
		second.Shutdown(context.Background())
	}
}

// The daemon and the Python SDK both compare the id's low 64 bits against
// `rate * 2^64`. Go scales to 53 bits so the float multiply stays exact, which
// must not change the answer. A disagreement here splits a fleet's traces in
// half, and the two halves are the ones nobody looks at.
func TestSamplingAgreesWithTheRuleTheDaemonAndTheOtherSDKsUse(t *testing.T) {
	ids := deterministicTraceIDs(5000)
	for _, rate := range []float64{0.01, 0.1, 0.25, 0.5, 0.75, 0.99} {
		client := New(WithEnabled(false), WithSampleRate(rate), WithLogger(nil))
		// 2^64 as a float64, exactly representable.
		threshold := uint64(rate * 18446744073709551616.0)
		for _, id := range ids {
			want := traceHash(id) < threshold
			if got := client.sampledTrace(id); got != want {
				t.Fatalf("rate %v, id %s: Go says %v, the shared rule says %v (hash %#x, threshold %#x)",
					rate, id, got, want, traceHash(id), threshold)
			}
		}
		client.Shutdown(context.Background())
	}
}

// A sampler that agrees with itself but keeps the wrong fraction is still
// broken. Nothing checked that the realised rate is anywhere near the asked
// one, which is what an off-by-one in the bit scaling would show up as.
func TestTheRealisedSampleRateIsCloseToTheRequestedOne(t *testing.T) {
	const ids = 20000
	traceIDs := deterministicTraceIDs(ids)
	for _, rate := range []float64{0.05, 0.25, 0.5, 0.9} {
		client := New(WithEnabled(false), WithSampleRate(rate), WithLogger(nil))
		kept := 0
		for _, id := range traceIDs {
			if client.sampledTrace(id) {
				kept++
			}
		}
		client.Shutdown(context.Background())

		realised := float64(kept) / float64(ids)
		// Four standard deviations of a binomial at this size, rounded up.
		tolerance := 4 * math.Sqrt(rate*(1-rate)/float64(ids))
		if math.Abs(realised-rate) > tolerance {
			t.Errorf("asked for %v, kept %v (%d of %d), outside the tolerance of %v",
				rate, realised, kept, ids, tolerance)
		}
	}
}

// An upstream decision always wins, in both directions. A distributed trace
// that some services keep and others drop is not a trace.
func TestAnUpstreamDecisionOverridesTheLocalSampleRate(t *testing.T) {
	const traceID = "4bf92f3577b34da6a3ce929d0e0e4736"

	// Upstream sampled this trace out. A local rate of 1 must not revive it.
	dropping, dropSink := newRawClient(t, WithSampleRate(1))
	header := http.Header{}
	header.Set(TraceparentHeader, "00-"+traceID+"-00f067aa0ba902b7-00")
	ctx := Extract(context.Background(), header)
	_, span := dropping.Start(ctx, "handler")
	if span.IsRecording() {
		t.Error("a trace the upstream dropped was recorded downstream")
	}
	span.End()
	if err := dropping.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	if got := len(dropSink.allSpans()); got != 0 {
		t.Errorf("%d spans were exported for an unsampled upstream trace", got)
	}

	// Upstream kept it. A local rate of 0 must not drop it.
	keeping, keepSink := newRawClient(t, WithSampleRate(0))
	header = http.Header{}
	header.Set(TraceparentHeader, "00-"+traceID+"-00f067aa0ba902b7-01")
	ctx = Extract(context.Background(), header)
	_, kept := keeping.Start(ctx, "handler")
	if !kept.IsRecording() {
		t.Error("a trace the upstream kept was dropped downstream")
	}
	kept.End()
	if err := keeping.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	if got := len(keepSink.allSpans()); got != 1 {
		t.Errorf("%d spans were exported for a sampled upstream trace, want 1", got)
	}
}

// The sampled flag a span emits has to match what the span actually did, or the
// next service downstream inherits a lie.
func TestTheEmittedTraceparentCarriesTheDecisionThatWasMade(t *testing.T) {
	dropped, _ := newRawClient(t, WithSampleRate(0))
	_, span := dropped.Start(context.Background(), "agent")
	if !strings.HasSuffix(span.Traceparent(), "-00") {
		t.Errorf("an unsampled span emits %q, want flags 00", span.Traceparent())
	}

	kept, _ := newRawClient(t, WithSampleRate(1))
	_, keptSpan := kept.Start(context.Background(), "agent")
	if !strings.HasSuffix(keptSpan.Traceparent(), "-01") {
		t.Errorf("a sampled span emits %q, want flags 01", keptSpan.Traceparent())
	}

	// A disabled client records nothing, so it must not tell a downstream
	// service that this trace is being kept.
	off := New(WithEnabled(false), WithLogger(nil))
	defer off.Shutdown(context.Background())
	_, offSpan := off.Start(context.Background(), "agent")
	if !strings.HasSuffix(offSpan.Traceparent(), "-00") {
		t.Errorf("a disabled client emits %q, want flags 00", offSpan.Traceparent())
	}
}

// --------------------------------------------------- B7, trace context ---

// W3C allows a later version to append fields, and says a receiver that
// understands version 00 should still read the first four. Rejecting them would
// break the trace at the first service that upgraded.
func TestATraceparentFromALaterVersionIsAcceptedWithItsExtraFields(t *testing.T) {
	parsed, ok := ParseTraceparent("01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-cc")
	if !ok {
		t.Fatal("a version 01 header with an extra field was rejected")
	}
	if parsed.TraceID != "4bf92f3577b34da6a3ce929d0e0e4736" || parsed.SpanID != "00f067aa0ba902b7" {
		t.Errorf("the ids were misread: %+v", parsed)
	}
	if !parsed.Sampled {
		t.Error("the sampled flag was misread")
	}
}

// The flags byte is a bit field. Only the low bit means sampled, and an unknown
// bit set alongside it must not make the header unreadable.
func TestUnknownTraceFlagBitsDoNotHideTheSampledBit(t *testing.T) {
	cases := map[string]bool{
		"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00": false,
		"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01": true,
		"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-03": true,
		"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-fe": false,
		"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-ff": true,
	}
	for header, want := range cases {
		parsed, ok := ParseTraceparent(header)
		if !ok {
			t.Errorf("rejected %q", header)
			continue
		}
		if parsed.Sampled != want {
			t.Errorf("%q parsed as sampled=%v, want %v", header, parsed.Sampled, want)
		}
	}
}

// A header that arrived with surrounding whitespace is still a valid header.
// Rejecting it would silently start a new trace and break the correlation.
func TestATraceparentPaddedWithWhitespaceIsStillRead(t *testing.T) {
	parsed, ok := ParseTraceparent("  00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01\t")
	if !ok {
		t.Fatal("a padded header was rejected")
	}
	if parsed.TraceID != "4bf92f3577b34da6a3ce929d0e0e4736" {
		t.Errorf("the trace id was misread as %q", parsed.TraceID)
	}
}

// A service that extracts an upstream trace and calls onwards without starting
// a span of its own still has to propagate. Nothing tested that path.
func TestInjectPropagatesAnUpstreamParentWhenThereIsNoLocalSpan(t *testing.T) {
	inbound := http.Header{}
	inbound.Set(TraceparentHeader, "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
	ctx := Extract(context.Background(), inbound)

	outbound := http.Header{}
	Inject(ctx, outbound)
	if got := outbound.Get(TraceparentHeader); got != "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01" {
		t.Fatalf("Inject wrote %q", got)
	}
}

// A malformed header is never a reason to fail a request. The service starts a
// new trace and carries on, and the new trace is a root of its own.
func TestAMalformedTraceparentStartsAFreshTraceInsteadOfFailing(t *testing.T) {
	client, sink := newRawClient(t)

	for _, bad := range []string{
		"",
		"garbage",
		"00-00000000000000000000000000000000-00f067aa0ba902b7-01",
		"ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
		"00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
	} {
		header := http.Header{}
		header.Set(TraceparentHeader, bad)
		ctx := Extract(context.Background(), header)
		if _, ok := RemoteParentFromContext(ctx); ok {
			t.Errorf("%q was accepted as an upstream parent", bad)
		}
		_, span := client.Start(ctx, "handler")
		if !isTraceID(span.TraceID()) {
			t.Errorf("%q left the span with an invalid trace id %q", bad, span.TraceID())
		}
		span.End()
	}

	if err := client.Flush(context.Background()); err != nil {
		t.Fatalf("flush: %v", err)
	}
	traces := map[string]bool{}
	for _, span := range sink.allSpans() {
		if !span.IsRoot {
			t.Errorf("a span that found no upstream parent is not a root: %+v", span.ParentSpanID)
		}
		if traces[span.TraceID] {
			t.Errorf("two handlers ended up on the same trace %s", span.TraceID)
		}
		traces[span.TraceID] = true
	}
}

// An explicit parent beats whatever the context happens to carry. This is what
// a queue consumer uses to attach work to the trace that enqueued it.
func TestAnExplicitParentBeatsTheSpanInTheContext(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	ctx, inContext := client.Start(ctx, "wrong_parent")
	explicit := TraceContext{
		TraceID: "4bf92f3577b34da6a3ce929d0e0e4736",
		SpanID:  "00f067aa0ba902b7",
		Sampled: true,
	}
	_, span := client.Start(ctx, "consumer", WithParent(explicit))
	span.End()
	inContext.End()

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	for _, s := range sink.allSpans() {
		if s.Name != "consumer" {
			continue
		}
		if s.TraceID != explicit.TraceID {
			t.Errorf("the span landed on trace %s, want %s", s.TraceID, explicit.TraceID)
		}
		if s.ParentSpanID == nil || *s.ParentSpanID != explicit.SpanID {
			t.Errorf("the span's parent is %v, want %s", s.ParentSpanID, explicit.SpanID)
		}
	}
}

// ------------------------------------------------------- B9, timestamps ---

// Local timestamps make a distributed trace unreadable, so the SDK converts
// before it sends. A start time handed over in a non-UTC zone is the case that
// catches a missing conversion.
func TestTimestampsAreSentAsUTCWithNanosecondResolution(t *testing.T) {
	client, sink := newRawClient(t)

	// A zone with a minute offset, so a conversion that only handles whole
	// hours shows up as well.
	kathmandu := time.FixedZone("NPT", 5*3600+45*60)
	start := time.Date(2026, 8, 28, 9, 30, 15, 123456789, kathmandu)

	ctx := context.Background()
	_, span := client.Start(ctx, "step", WithStartTime(start))
	span.End()
	client.Log(ctx, LogLevelInfo, "line", nil)
	client.Metric(ctx, "value", 1, MetricKindGauge)
	client.Score(ctx, TraceTarget(span.TraceID()), "helpfulness", 1, 0, 1)
	client.Feedback(ctx, TraceTarget(span.TraceID()), FeedbackKindThumbs, "up")
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	// Read the wire, not the decoded value: a time.Time decoded from an offset
	// would still compare equal to the same instant in UTC, so decoding hides
	// exactly the defect this test is for.
	raw := sink.rawJSON()
	if strings.Contains(raw, "+05:45") {
		t.Errorf("a local offset reached the wire: %s", firstBytes(raw))
	}
	if !strings.Contains(raw, `"start_time":"2026-08-28T03:45:15.123456789Z"`) {
		t.Errorf("the start time was not converted to UTC with its nanoseconds intact: %s", firstBytes(raw))
	}

	spans := sink.allSpans()
	if len(spans) != 1 {
		t.Fatalf("expected 1 span, got %d", len(spans))
	}
	if got := spans[0].StartTime.Nanosecond(); got != 123456789 {
		t.Errorf("the start time arrived with %d nanoseconds, want 123456789", got)
	}
	sink.mu.Lock()
	defer sink.mu.Unlock()
	for _, batch := range sink.batches {
		for _, record := range batch.Logs {
			if name, _ := record.Timestamp.Zone(); name != "UTC" {
				t.Errorf("a log timestamp is in %q", name)
			}
		}
		for _, point := range batch.Metrics {
			if name, _ := point.Timestamp.Zone(); name != "UTC" {
				t.Errorf("a metric timestamp is in %q", name)
			}
		}
		for _, score := range batch.Scores {
			if name, _ := score.Timestamp.Zone(); name != "UTC" {
				t.Errorf("a score timestamp is in %q", name)
			}
		}
		for _, item := range batch.Feedback {
			if name, _ := item.Timestamp.Zone(); name != "UTC" {
				t.Errorf("a feedback timestamp is in %q", name)
			}
		}
	}
}

// A duration rounded to a millisecond loses every fast step in a run. The clock
// this package reads has to carry the resolution all the way to the field.
//
// Each span is backdated by 1500 nanoseconds. On Linux the clock itself lands
// between microseconds; on macOS it does not, and forty durations measured
// there all came out on a whole microsecond, which read as the rounding this
// test exists to catch. The backdate puts half a microsecond into every
// duration whatever the host clock does, so the assertion is about what the
// package keeps and not about where the operating system's clock ticks.
func TestDurationsKeepSubMicrosecondResolution(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	for i := 0; i < 40; i++ {
		_, span := client.Start(ctx, "fast", WithStartTime(time.Now().Add(-1500*time.Nanosecond)))
		span.End()
	}
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}

	spans := sink.allSpans()
	if len(spans) != 40 {
		t.Fatalf("expected 40 spans, got %d", len(spans))
	}
	finerThanAMicrosecond := 0
	for _, span := range spans {
		if span.DurationNs == nil {
			t.Fatalf("span %s carries no duration", span.SpanID)
		}
		if *span.DurationNs%1000 != 0 {
			finerThanAMicrosecond++
		}
	}
	if finerThanAMicrosecond == 0 {
		t.Fatalf("all 40 durations landed on a whole microsecond, so the clock is coarser than the field claims")
	}
}

// A backdated span must not be given a negative duration, which is what a naive
// subtraction produces when the caller's clock has moved backwards.
func TestAStartTimeInTheFutureDoesNotProduceANegativeDuration(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "clock_skew", WithStartTime(time.Now().Add(time.Hour)))
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	spans := sink.allSpans()
	if len(spans) != 1 {
		t.Fatalf("expected 1 span, got %d", len(spans))
	}
	if spans[0].DurationNs == nil {
		t.Fatal("no duration was recorded")
	}
	if *spans[0].DurationNs != 0 {
		t.Errorf("a span that started in the future has a duration of %d ns, want 0", *spans[0].DurationNs)
	}
}

// The arrival time is the server's to set. A client sending its own would let a
// machine with a wrong clock rewrite retention.
func TestTheClientSendsTheEpochAsReceivedAtAndLetsTheServerStampIt(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.End()
	client.Log(ctx, LogLevelInfo, "line", nil)
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	for _, s := range sink.allSpans() {
		if !s.ReceivedAt.Equal(time.Unix(0, 0)) {
			t.Errorf("the client stamped its own arrival time: %v", s.ReceivedAt)
		}
	}
}

// ---------------------------------------------------------- B10, errors ---

// emptyMessageError is the awkward case B10 names: an error type that says
// nothing when asked.
type emptyMessageError struct{}

func (emptyMessageError) Error() string { return "" }

// "Something failed" is not actionable, so an error with no message falls back
// to naming its type rather than recording an empty string.
func TestAnErrorWithNoMessageStillNamesItsType(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.RecordError(emptyMessageError{})
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	spans := sink.allSpans()
	if len(spans) != 1 || spans[0].Error == nil {
		t.Fatalf("the error was not exported: %+v", spans)
	}
	if spans[0].Error.Message != "mcpd.emptyMessageError" {
		t.Errorf("the message is %q, want the type name", spans[0].Error.Message)
	}
	if spans[0].Status != SpanStatusError {
		t.Errorf("status is %q, want error", spans[0].Status)
	}
}

// A wrapped error is the normal shape in Go. The message must be the whole
// chain, and the kind must not be the wrapper's implementation type.
func TestAWrappedErrorKeepsTheWholeChainAndDoesNotReportTheWrapperType(t *testing.T) {
	client, sink := newRawClient(t)

	inner := &smtpFailure{code: 451}
	wrapped := fmt.Errorf("sending the reply: %w", inner)

	ctx := context.Background()
	_, span := client.Start(ctx, "step")
	span.RecordError(wrapped)
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	spans := sink.allSpans()
	if len(spans) != 1 || spans[0].Error == nil {
		t.Fatalf("the error was not exported: %+v", spans)
	}
	if spans[0].Error.Message != "sending the reply: mailbox busy" {
		t.Errorf("the message is %q", spans[0].Error.Message)
	}
	if spans[0].Error.Kind != nil {
		t.Errorf("the wrapper's type was recorded as the error class: %q", *spans[0].Error.Kind)
	}
}

// The last error recorded is the one the span carries, and an explicit end
// status still wins over it. Both are what a retry loop relies on.
func TestTheLastRecordedErrorWinsAndAnExplicitEndStatusBeatsIt(t *testing.T) {
	client, sink := newRawClient(t)

	ctx := context.Background()
	_, span := client.Start(ctx, "retrying")
	span.RecordError(errors.New("attempt one"))
	span.RecordError(errors.New("attempt two"), ErrorRetryable(false))
	span.EndWithStatus(SpanStatusCancelled)

	if err := client.Flush(ctx); err != nil {
		t.Fatalf("flush: %v", err)
	}
	spans := sink.allSpans()
	if len(spans) != 1 || spans[0].Error == nil {
		t.Fatalf("the error was not exported: %+v", spans)
	}
	if spans[0].Error.Message != "attempt two" {
		t.Errorf("the message is %q, want the last error recorded", spans[0].Error.Message)
	}
	if spans[0].Error.Retryable == nil || *spans[0].Error.Retryable {
		t.Errorf("the second error's retryable flag was lost: %v", spans[0].Error.Retryable)
	}
	if spans[0].Status != SpanStatusCancelled {
		t.Errorf("status is %q, want the explicit cancelled", spans[0].Status)
	}
}

// ------------------------------------------------- an unreachable daemon ---

// freePort asks the kernel for a port and gives it straight back, so the
// address is one nothing is listening on. A hardcoded port would find whatever
// another process on this machine happens to be running.
func freePort(t *testing.T) int {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("could not reserve a port: %v", err)
	}
	port := listener.Addr().(*net.TCPAddr).Port
	listener.Close()
	return port
}

// The daemon being absent must cost the caller nothing at all, including the
// time the retries take. The worker absorbs that; the calling goroutine must
// not see it.
func TestAnUnreachableDaemonCostsTheCallerNothing(t *testing.T) {
	endpoint := fmt.Sprintf("http://127.0.0.1:%d", freePort(t))
	client := New(
		WithEndpoint(endpoint),
		WithEnabled(true),
		WithBatchSize(1),
		WithFlushInterval(10*time.Millisecond),
		WithTimeout(100*time.Millisecond),
		WithLogger(nil),
	)
	t.Cleanup(func() {
		// A short deadline on purpose: the worker is retrying against nothing,
		// and how long it takes to give up is not what this test measures.
		bounded, cancel := context.WithTimeout(context.Background(), 500*time.Millisecond)
		defer cancel()
		_ = client.Shutdown(bounded)
	})

	ctx := context.Background()
	started := time.Now()
	for i := 0; i < 300; i++ {
		reqCtx, span := client.Start(ctx, "step")
		span.SetInput(map[string]any{"i": i})
		span.SetTokens(10, 20)
		client.Log(reqCtx, LogLevelWarn, "no daemon", nil)
		span.End()
	}
	elapsed := time.Since(started)

	// Three attempts with backoff would be roughly a second per batch. The
	// caller must not have paid any of it.
	if elapsed > 500*time.Millisecond {
		t.Fatalf("recording against an absent daemon took %v", elapsed)
	}
	if queued := client.Stats().Queued; queued > client.cfg.maxQueue {
		t.Errorf("the queue reached %d, past its bound of %d", queued, client.cfg.maxQueue)
	}
}
