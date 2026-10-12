package mcpd

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
	"testing"
)

// The tests in this file cover sections 7.2 (hooks and stats) and 7.3 (usage
// accumulation) of sdks/SDK-CONTRACT.md for the worker.

func TestUsageOnSpansUnderTheStepIsSummedOntoTheStep(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started}, "/complete": {acked}})
	h.worker.Register(NewWorkflow("support").Agent("classify", func(ctx context.Context, step *StepContext) (any, error) {
		// A span from the handler's ctx, a span nested under it through the
		// ambient parent, and one from the explicit form: all three count.
		ctx, first := h.client.Start(ctx, "first", WithModel("m1", "p1"))
		first.SetTokens(100, 10)
		_, nested := h.client.Start(ctx, "nested", WithModel("m2", "p2"))
		nested.SetUsage(TokenUsage{InputTokens: ptrU64(20), OutputTokens: ptrU64(5)})
		nested.End()
		first.End()
		_, explicit := step.Start(context.Background(), "explicit")
		explicit.SetTokens(1, 1)
		explicit.End()
		return "ok", nil
	}))
	h.claim(t, 1)
	expectPaths(t, h.daemon.paths(), "POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/complete")
	// first ended after nested, so its model is the last one seen; explicit
	// carried none and left it alone.
	equalJSON(t, h.daemon.call(2).body["usage"], map[string]any{
		"input_tokens": 121, "output_tokens": 16, "total_tokens": 112, "model": "m1", "provider": "p1",
	})
}

func TestSetUsageOverridesTheFieldsItSetsAndKeepsTheSum(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started}, "/complete": {acked}})
	h.worker.Register(NewWorkflow("support").Agent("classify", func(ctx context.Context, step *StepContext) (any, error) {
		_, span := h.client.Start(ctx, "call", WithModel("m1", "p1"))
		span.SetTokens(100, 10)
		span.End()
		step.SetUsage(StepUsage{InputTokens: ptrU64(7), Model: strPtr("by-hand")})
		if usage := step.Usage(); usage == nil || *usage.InputTokens != 7 || *usage.OutputTokens != 10 {
			t.Errorf("Usage() should show the merge: %+v", usage)
		}
		return "ok", nil
	}))
	h.claim(t, 1)
	equalJSON(t, h.daemon.call(2).body["usage"], map[string]any{
		"input_tokens": 7, "output_tokens": 10, "total_tokens": 110, "model": "by-hand", "provider": "p1",
	})
}

func TestABodyThatRecordsNoUsageCompletesWithNone(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started}, "/complete": {acked}})
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		_, span := h.client.Start(ctx, "no_tokens")
		span.End()
		return "ok", nil
	}))
	h.claim(t, 1)
	if _, present := h.daemon.call(2).body["usage"]; present {
		t.Fatalf("complete should carry no usage: %v", h.daemon.call(2).body)
	}
}

type hookLog struct {
	mu    sync.Mutex
	lines []string
}

func (l *hookLog) add(format string, args ...any) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.lines = append(l.lines, fmt.Sprintf(format, args...))
}

func (l *hookLog) all() []string {
	l.mu.Lock()
	defer l.mu.Unlock()
	return append([]string(nil), l.lines...)
}

func hookOptions(log *hookLog) []WorkerOption {
	return []WorkerOption{
		WorkerOnLease(func(lease StepLease) { log.add("lease %s", lease.LeaseID) }),
		WorkerOnStepStart(func(step *StepContext) { log.add("start %s@%d", step.StepKey, step.Attempt) }),
		WorkerOnStepEnd(func(step *StepContext, outcome StepOutcome) {
			log.add("end %s %s %s %s", step.StepKey, outcome.Status, outcome.Class, outcome.Message)
		}),
		WorkerOnConflict(func(lease StepLease, reason string) { log.add("conflict %s %s", lease.LeaseID, reason) }),
	}
}

func TestTheFourHooksSeeTheLeaseTheStartTheEndAndTheConflict(t *testing.T) {
	log := &hookLog{}
	h := newHarness(t, map[string][]reply{
		"/steps/claim": {claimed(wireLease(nil)), claimed(wireLease(map[string]any{"lease_id": "lease-2"}))},
		"/started":     {started, conflict("fenced", nil)},
		"/complete":    {acked},
	}, hookOptions(log)...)
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		return "ok", nil
	}))
	h.claim(t, 1)
	h.claim(t, 1)
	want := []string{"lease lease-1", "start classify@0", "end classify completed  ", "lease lease-2", "conflict lease-2 fenced"}
	if got := log.all(); strings.Join(got, "\n") != strings.Join(want, "\n") {
		t.Fatalf("hooks\n got: %q\nwant: %q", got, want)
	}
	stats := h.worker.Stats()
	if stats.Claimed != 2 || stats.Completed != 1 || stats.Inflight != 0 || stats.Unservable != 0 {
		t.Fatalf("stats: %+v", stats)
	}
	if len(stats.Failed) != 0 || stats.Conflicts["fenced"] != 1 || len(stats.Conflicts) != 1 {
		t.Fatalf("stats maps: %+v", stats)
	}
	stats.Conflicts["fenced"] = 99
	if h.worker.Stats().Conflicts["fenced"] != 1 {
		t.Fatal("Stats should hand out copies")
	}
}

func TestAHookThatPanicsIsReportedAndTheLeaseStillCompletes(t *testing.T) {
	h := newHarness(t, map[string][]reply{"/steps/claim": {claimed(wireLease(nil))}, "/started": {started}, "/complete": {acked}},
		WorkerOnStepStart(func(*StepContext) { panic("the hook broke") }),
		WorkerOnStepEnd(func(*StepContext, StepOutcome) { panic("this one too") }))
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		return "ok", nil
	}))
	h.claim(t, 1)
	expectPaths(t, h.daemon.paths(), "POST /v1/api/steps/claim", "POST /v1/api/steps/lease-1/started", "POST /v1/api/steps/lease-1/complete")
	reported := h.reported()
	if len(reported) != 2 || !strings.Contains(reported[0], "on_step_start hook panicked") || !strings.Contains(reported[1], "on_step_end hook panicked") {
		t.Fatalf("reported: %v", reported)
	}
	if stats := h.worker.Stats(); stats.Completed != 1 {
		t.Fatalf("stats: %+v", stats)
	}
}

func TestWorkerStatsCountAFatalFailureAnUnservableLeaseAndAConflict(t *testing.T) {
	log := &hookLog{}
	h := newHarness(t, map[string][]reply{
		"/steps/claim": {
			claimed(wireLease(nil)),
			claimed(wireLease(map[string]any{"lease_id": "lease-2", "step_key": "missing"})),
			claimed(wireLease(map[string]any{"lease_id": "lease-3"})),
		},
		"/started":  {started, started},
		"/fail":     {acked, acked},
		"/complete": {conflict("already_applied", nil)},
	}, hookOptions(log)...)
	h.worker.Register(NewWorkflow("support").Step("classify", func(ctx context.Context, step *StepContext) (any, error) {
		if step.Lease.LeaseID == "lease-1" {
			return nil, errors.New("boom")
		}
		return "ok", nil
	}))
	h.claim(t, 1)
	h.claim(t, 1)
	h.claim(t, 1)
	stats := h.worker.Stats()
	if stats.Claimed != 3 || stats.Completed != 1 || stats.Unservable != 1 {
		t.Fatalf("stats: %+v", stats)
	}
	if stats.Failed["fatal"] != 1 || len(stats.Failed) != 1 || stats.Conflicts["already_applied"] != 1 {
		t.Fatalf("stats maps: %+v", stats)
	}
	got := log.all()
	if !contains(got, "end classify failed fatal boom") || !contains(got, "conflict lease-3 already_applied") {
		t.Fatalf("hooks: %q", got)
	}
	if contains(got, "start missing@0") {
		t.Fatalf("an unservable lease must not start: %q", got)
	}
}

func contains(lines []string, want string) bool {
	for _, line := range lines {
		if line == want {
			return true
		}
	}
	return false
}
