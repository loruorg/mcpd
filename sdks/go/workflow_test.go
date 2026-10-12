package mcpd

import (
	"context"
	"encoding/json"
	"reflect"
	"testing"
	"time"
)

func noop(ctx context.Context, step *StepContext) (any, error) { return "done", nil }

func documentBody(t *testing.T, w *Workflow) map[string]any {
	t.Helper()
	doc, err := w.Document()
	if err != nil {
		t.Fatalf("Document: %v", err)
	}
	var body map[string]any
	if err := json.Unmarshal(doc.Body, &body); err != nil {
		t.Fatalf("the body is not JSON: %v", err)
	}
	return body
}

func asJSON(t *testing.T, v any) any {
	t.Helper()
	raw, err := json.Marshal(v)
	if err != nil {
		t.Fatal(err)
	}
	var out any
	if err := json.Unmarshal(raw, &out); err != nil {
		t.Fatal(err)
	}
	return out
}

func TestTheDocumentSpellsEveryConstructTheWayTheDaemonReadsIt(t *testing.T) {
	w := NewWorkflow("support",
		WithVersion(2),
		WithInputSchema(map[string]any{"type": "object", "required": []string{"ticket"}}),
		WithLimits(Limits{MaxSteps: 50, RunTimeout: time.Minute, MaxCostUSD: 2}),
	).
		Step("classify", noop,
			Input(map[string]any{"ticket": "${input.ticket}"}),
			AtMostOnce(),
			StepTimeout(5*time.Second),
			Retry(RetryPolicy{MaxAttempts: 3, Backoff: BackoffExponential, Initial: 100 * time.Millisecond, Max: time.Second}),
		).
		Agent("draft", noop, Guard(When("steps.classify.output.kind", "eq", "question")), ContinueOnError()).
		Sleep("cool", 10*time.Millisecond).
		SleepUntil("bill", "${input.billing_date}").
		WaitFor("approval", "approved", "${input.ticket}", time.Second, OnTimeout(WaitTimeoutContinue), AcceptParked()).
		Parallel("checks", func(b *Branch) { b.Step("fraud", noop).Agent("policy", noop) }).
		ForEach("notify", "input.recipients", 10, func(b *Branch) {
			b.Step("send", noop, Input(map[string]any{"to": "${item}", "position": "${index}"}))
		}, Concurrency(2)).
		Loop("refine", 3, func(b *Branch) { b.Step("rewrite", noop) }, Until(When("steps.rewrite.output.done", "eq", true))).
		Subworkflow("escalate", "escalation",
			SubworkflowVersion(1),
			Input(map[string]any{"ticket": "${input.ticket}"}),
			Guard(Not(When("steps.checks.fraud.output.clean", "exists"))),
		).
		Output(map[string]any{"answer": "${steps.draft.output}"})

	doc, err := w.Document()
	if err != nil {
		t.Fatal(err)
	}
	if doc.ID != "support" || doc.Kind == nil || *doc.Kind != DefinitionKindWorkflow || doc.Version == nil || *doc.Version != 2 {
		t.Fatalf("document header: %+v", doc)
	}
	want := asJSON(t, map[string]any{
		"input_schema": map[string]any{"type": "object", "required": []string{"ticket"}},
		"limits":       map[string]any{"max_steps": 50, "run_timeout_ms": 60_000, "max_cost_usd": 2},
		"output":       map[string]any{"answer": "${steps.draft.output}"},
		"steps": []any{
			map[string]any{"kind": "task", "id": "classify", "input": map[string]any{"ticket": "${input.ticket}"},
				"effect": "at_most_once", "step_timeout_ms": 5000,
				"retry": map[string]any{"max_attempts": 3, "backoff": "exponential", "initial_ms": 100, "max_ms": 1000}},
			map[string]any{"kind": "agent", "id": "draft",
				"when": map[string]any{"path": "steps.classify.output.kind", "op": "eq", "value": "question"}, "on_error": "continue"},
			map[string]any{"kind": "sleep", "id": "cool", "ms": 10},
			map[string]any{"kind": "sleep_until", "id": "bill", "at": "${input.billing_date}"},
			map[string]any{"kind": "wait_event", "id": "approval", "event": "approved", "key": "${input.ticket}",
				"timeout_ms": 1000, "accept_parked": true, "on_timeout": "continue"},
			map[string]any{"kind": "parallel", "id": "checks", "branches": []any{
				map[string]any{"kind": "task", "id": "fraud"}, map[string]any{"kind": "agent", "id": "policy"}}},
			map[string]any{"kind": "foreach", "id": "notify", "over": "input.recipients",
				"body":      map[string]any{"kind": "task", "id": "send", "input": map[string]any{"to": "${item}", "position": "${index}"}},
				"max_items": 10, "concurrency": 2},
			map[string]any{"kind": "loop", "id": "refine", "body": map[string]any{"kind": "task", "id": "rewrite"},
				"max_iterations": 3, "until": map[string]any{"path": "steps.rewrite.output.done", "op": "eq", "value": true}},
			map[string]any{"kind": "subworkflow", "id": "escalate", "definition": "escalation", "version": 1,
				"input": map[string]any{"ticket": "${input.ticket}"},
				"when":  map[string]any{"not": map[string]any{"path": "steps.checks.fraud.output.clean", "op": "exists"}}},
		},
	})
	got := asJSON(t, documentBody(t, w))
	if !reflect.DeepEqual(got, want) {
		gotJSON, _ := json.MarshalIndent(got, "", "  ")
		wantJSON, _ := json.MarshalIndent(want, "", "  ")
		t.Fatalf("document body\n got: %s\nwant: %s", gotJSON, wantJSON)
	}
}

func TestWhatWasNotSaidIsLeftOutSoTheDaemonAppliesItsDefaults(t *testing.T) {
	doc, err := NewWorkflow("plain").Step("one", noop).Document()
	if err != nil {
		t.Fatal(err)
	}
	if doc.Version != nil {
		t.Fatalf("no version was named, got %d", *doc.Version)
	}
	if got := string(doc.Body); got != `{"steps":[{"id":"one","kind":"task"}]}` {
		t.Fatalf("body: %s", got)
	}
}

func TestPredicatesCombineTheWayTheContractSpellsThem(t *testing.T) {
	got := asJSON(t, All(When("a", "exists"), Any(When("b", "gt", 1), Not(When("c", "in", []int{1, 2})))))
	want := asJSON(t, map[string]any{"all": []any{
		map[string]any{"path": "a", "op": "exists"},
		map[string]any{"any": []any{
			map[string]any{"path": "b", "op": "gt", "value": 1},
			map[string]any{"not": map[string]any{"path": "c", "op": "in", "value": []int{1, 2}}},
		}},
	}})
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v want %v", got, want)
	}
}

func TestAnIDTheDaemonWouldRefuseIsRefusedByDocument(t *testing.T) {
	for _, w := range []*Workflow{
		NewWorkflow("Support"),
		NewWorkflow("ok").Step("9lives", noop),
		NewWorkflow("ok").Sleep("has-dash", time.Second),
		NewWorkflow("ok").Parallel("checks", func(b *Branch) { b.Step("Bad", noop) }),
	} {
		if _, err := w.Document(); err == nil {
			t.Fatalf("expected an id error from %s", w.ID())
		} else if w.Err() != err {
			t.Fatalf("Err and Document disagree: %v vs %v", w.Err(), err)
		}
	}
}

func TestAForEachOrLoopBodyIsOneClaimableStep(t *testing.T) {
	_, err := NewWorkflow("ok").ForEach("each", "input.items", 1, func(b *Branch) { b.Step("a", noop).Step("b", noop) }).Document()
	if err == nil {
		t.Fatal("two bodies should be refused")
	}
	_, err = NewWorkflow("ok").Loop("again", 1, func(b *Branch) {}).Document()
	if err == nil {
		t.Fatal("no body should be refused")
	}
}

func TestHandlerForResolvesAPlainStepABranchAnItemAndAnIteration(t *testing.T) {
	name := func(n string) StepHandler {
		return func(ctx context.Context, step *StepContext) (any, error) { return n, nil }
	}
	w := NewWorkflow("support").
		Step("classify", name("classify")).
		Parallel("checks", func(b *Branch) { b.Step("fraud", name("fraud")) }).
		ForEach("notify", "input.to", 5, func(b *Branch) { b.Step("send", name("send")) }).
		Loop("refine", 2, func(b *Branch) { b.Step("rewrite", name("rewrite")) })

	cases := map[string]string{
		"classify": "classify", "checks/fraud": "fraud", "notify[3]": "send", "notify[3]/send": "send",
		"refine[1]": "rewrite", "refine[1]/rewrite": "rewrite",
	}
	for key, want := range cases {
		handler := w.HandlerFor(key)
		if handler == nil {
			t.Fatalf("%s: no handler", key)
		}
		got, _ := handler(context.Background(), nil)
		if got != want {
			t.Fatalf("%s: got %v want %s", key, got, want)
		}
	}
	for _, key := range []string{"checks", "unknown", "checks/policy"} {
		if w.HandlerFor(key) != nil {
			t.Fatalf("%s should have no handler", key)
		}
	}
}
