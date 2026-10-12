package mcpd

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"
)

// The logging signal of contract section 4: correlation that survives an empty
// override, errors serialised rather than collapsed, the level floor, and the
// OpenTelemetry severity mapping.

func loggingClient(t *testing.T, opts ...Option) (*Client, *RecordingSink) {
	t.Helper()
	sink := &RecordingSink{}
	base := []Option{
		WithSink(sink), WithProject("test"), WithFlushInterval(time.Hour), WithLogger(nil),
	}
	client := New(append(base, opts...)...)
	t.Cleanup(func() { _ = client.Shutdown(context.Background()) })
	return client, sink
}

func TestAnEmptyCorrelationOverrideKeepsTheRunsOwn(t *testing.T) {
	client, sink := loggingClient(t)
	ctx := context.Background()
	ctx, run := client.Start(ctx, "run", WithSession("session-7"), WithUser("u-1"))
	// Named with the absent value. Not an instruction to erase the run's.
	inner, step := client.Start(ctx, "step", WithSession(""))
	client.Log(inner, LogLevelInfo, "inside", nil)
	client.Metric(inner, "latency_ms", 4, MetricKindGauge)
	step.End()
	run.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}

	logs := sink.Logs()
	if len(logs) != 1 || logs[0].Correlation.SessionID == nil ||
		*logs[0].Correlation.SessionID != "session-7" {
		t.Fatalf("log correlation: %+v", logs[0].Correlation)
	}
	if logs[0].Correlation.UserID == nil || *logs[0].Correlation.UserID != "u-1" {
		t.Fatalf("user id was erased: %+v", logs[0].Correlation)
	}
	points := sink.Metrics()
	if len(points) != 1 || points[0].Correlation.SessionID == nil ||
		*points[0].Correlation.SessionID != "session-7" {
		t.Fatalf("metric correlation: %+v", points[0].Correlation)
	}
}

func TestAnErrorUnderAFieldKeyArrivesWithItsTypeAndMessage(t *testing.T) {
	client, sink := loggingClient(t)
	ctx := context.Background()
	wrapped := fmt.Errorf("calling the model: %w", errors.New("boom"))
	client.Log(ctx, LogLevelError, "the call failed", map[string]any{
		"error":   wrapped,
		"attempt": 2,
	})
	client.Metric(ctx, "failures", 1, MetricKindCounter,
		MetricLabels(map[string]any{"cause": errors.New("label")}))
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}

	var fields map[string]any
	if err := json.Unmarshal(sink.Logs()[0].Fields, &fields); err != nil {
		t.Fatal(err)
	}
	described, ok := fields["error"].(map[string]any)
	if !ok {
		t.Fatalf("the error did not survive as an object: %v", fields["error"])
	}
	if described["message"] != "calling the model: boom" {
		t.Fatalf("message: %v", described["message"])
	}
	if !strings.Contains(fmt.Sprint(described["type"]), "wrapError") {
		t.Fatalf("type: %v", described["type"])
	}
	if fields["attempt"] != float64(2) {
		t.Fatalf("the other fields did not survive: %v", fields)
	}

	var labels map[string]any
	if err := json.Unmarshal(sink.Metrics()[0].Labels, &labels); err != nil {
		t.Fatal(err)
	}
	if cause, ok := labels["cause"].(map[string]any); !ok || cause["message"] != "label" {
		t.Fatalf("labels: %v", labels)
	}
}

func TestAValueNothingCanEncodeCostsItsKeyAndNotTheRest(t *testing.T) {
	client, sink := loggingClient(t)
	ctx := context.Background()
	// A channel has no JSON form at all, and the whole map used to go with it.
	client.Log(ctx, LogLevelError, "a request went wrong", map[string]any{
		"queue":  make(chan int),
		"nested": map[string]any{"why": errors.New("boom")},
		"size":   10,
	})
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}

	logs := sink.Logs()
	if len(logs) != 1 || len(logs[0].Fields) == 0 {
		t.Fatalf("the field map was dropped whole: %+v", logs)
	}
	var fields map[string]any
	if err := json.Unmarshal(logs[0].Fields, &fields); err != nil {
		t.Fatal(err)
	}
	if fields["size"] != float64(10) {
		t.Fatalf("a good field was lost with the bad one: %v", fields)
	}
	if fields["queue"] != "[unencodable chan int]" {
		t.Fatalf("the unencodable field was not named: %v", fields["queue"])
	}
	nested, ok := fields["nested"].(map[string]any)
	if !ok {
		t.Fatalf("nested: %v", fields["nested"])
	}
	why, ok := nested["why"].(map[string]any)
	if !ok || why["message"] != "boom" {
		t.Fatalf("a nested error was not serialised: %v", nested)
	}
	if stats := client.Stats(); stats.Failed != 0 || stats.Exported != 1 {
		t.Fatalf("the batch was lost: %+v", stats)
	}
}

func TestDescribeErrorAnswersOnlyForAnError(t *testing.T) {
	described := DescribeError(errors.New("boom"))
	if described["message"] != "boom" || described["type"] == "" {
		t.Fatalf("described: %v", described)
	}
	if DescribeError(nil) != nil {
		t.Fatal("nil is not an error")
	}
}

// pointerError reads through its receiver, as most pointer error types do.
type pointerError struct{ code int }

func (e *pointerError) Error() string { return fmt.Sprintf("code %d", e.code) }

// An error interface holding a nil pointer is not nil, so the SDK's own nil
// check passes and Error() then panicked inside the caller (B1).
func TestATypedNilErrorNeverPanicsTheCaller(t *testing.T) {
	var typed *pointerError
	var err error = typed

	client, sink := loggingClient(t)
	ctx := context.Background()
	ctx, span := client.Start(ctx, "run")
	span.RecordError(err)
	client.Log(ctx, LogLevelError, "the call failed", map[string]any{"error": err})
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}

	spans := sink.Spans()
	if len(spans) == 0 || spans[len(spans)-1].Error == nil || spans[len(spans)-1].Error.Message != "*mcpd.pointerError" {
		t.Fatalf("the span should carry the error, named by its type: %+v", spans)
	}
	if got := DescribeError(err); got["type"] != "mcpd.pointerError" || got["message"] != "*mcpd.pointerError" {
		t.Errorf("described: %v", got)
	}
	if got := toFailure(err); got.Class != StepErrorClassFatal || got.Message == "" {
		t.Errorf("a step body returning it: %+v", got)
	}
	if got := Retryable(err, 0).Error(); got != string(StepErrorClassRetryable) {
		t.Errorf("a retryable wrapping it: %q", got)
	}
}

func TestTheLevelFloorDropsTraceByDefault(t *testing.T) {
	client, sink := loggingClient(t)
	ctx := context.Background()
	if client.LogLevel() != LogLevelDebug {
		t.Fatalf("default floor: %s", client.LogLevel())
	}
	client.Log(ctx, LogLevelTrace, "finer than debug", nil)
	client.Log(ctx, LogLevelDebug, "kept", nil)
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	logs := sink.Logs()
	if len(logs) != 1 || logs[0].Message != "kept" {
		t.Fatalf("logs: %+v", logs)
	}
}

func TestTheLevelFloorDropsEverythingBelowIt(t *testing.T) {
	client, sink := loggingClient(t, WithLogLevel(LogLevelWarn))
	ctx := context.Background()
	for _, level := range AllLogLevels {
		client.Log(ctx, level, string(level), nil)
	}
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	var kept []string
	for _, record := range sink.Logs() {
		kept = append(kept, record.Message)
	}
	if strings.Join(kept, ",") != "warn,error,fatal" {
		t.Fatalf("kept: %v", kept)
	}
	if stats := client.Stats(); stats.Dropped != 0 {
		t.Fatalf("a line below the floor was queued and evicted: %+v", stats)
	}
}

func TestTheLevelFloorComesFromTheEnvironmentAndCodeWins(t *testing.T) {
	t.Setenv("MCPD_LOG_LEVEL", "ERROR")
	fromEnv, sink := loggingClient(t)
	ctx := context.Background()
	if fromEnv.LogLevel() != LogLevelError {
		t.Fatalf("from the environment: %s", fromEnv.LogLevel())
	}
	fromEnv.Log(ctx, LogLevelWarn, "below the floor", nil)
	if err := fromEnv.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	if len(sink.Logs()) != 0 {
		t.Fatalf("logs: %+v", sink.Logs())
	}

	fromCode, _ := loggingClient(t, WithLogLevel(LogLevelTrace))
	if fromCode.LogLevel() != LogLevelTrace {
		t.Fatalf("code did not win: %s", fromCode.LogLevel())
	}

	t.Setenv("MCPD_LOG_LEVEL", "verbose")
	ignored, _ := loggingClient(t)
	if ignored.LogLevel() != LogLevelDebug {
		t.Fatalf("a name outside the enum was not ignored: %s", ignored.LogLevel())
	}
}

func TestTheLevelFloorNeverFiltersTheOtherSignals(t *testing.T) {
	client, sink := loggingClient(t, WithLogLevel(LogLevelFatal))
	ctx := context.Background()
	ctx, span := client.Start(ctx, "run")
	client.Metric(ctx, "latency_ms", 3, MetricKindGauge)
	client.Score(ctx, Target{}, "judge", 1, 0, 1)
	client.Feedback(ctx, Target{}, FeedbackKindThumbs, "up")
	span.End()
	if err := client.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	if len(sink.Spans()) != 1 || len(sink.Metrics()) != 1 || len(sink.Scores()) != 1 ||
		len(sink.Feedback()) != 1 {
		t.Fatalf("the floor reached another signal: %d %d %d %d",
			len(sink.Spans()), len(sink.Metrics()), len(sink.Scores()), len(sink.Feedback()))
	}
}

func TestSeverityNumbersMapBothWays(t *testing.T) {
	want := []int{1, 5, 9, 13, 17, 21}
	for i, level := range AllLogLevels {
		if level.Severity() != want[i] {
			t.Fatalf("%s: %d", level, level.Severity())
		}
		if LogLevelFromSeverity(level.Severity()) != level {
			t.Fatalf("%s did not round-trip", level)
		}
	}
	ranges := []struct {
		from, to int
		level    LogLevel
	}{
		{1, 4, LogLevelTrace},
		{5, 8, LogLevelDebug},
		{9, 12, LogLevelInfo},
		{13, 16, LogLevelWarn},
		{17, 20, LogLevelError},
		{21, 24, LogLevelFatal},
	}
	for _, r := range ranges {
		for n := r.from; n <= r.to; n++ {
			if got := LogLevelFromSeverity(n); got != r.level {
				t.Fatalf("severity %d: got %s, want %s", n, got, r.level)
			}
		}
	}
	if LogLevelFromSeverity(0) != LogLevelTrace || LogLevelFromSeverity(-3) != LogLevelTrace {
		t.Fatal("a number below the range is not clamped to trace")
	}
	if LogLevelFromSeverity(40) != LogLevelFatal {
		t.Fatal("a number above the range is not clamped to fatal")
	}
}
