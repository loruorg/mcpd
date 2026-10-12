// A durable workflow in Go: the graph goes to the daemon as a definition, the
// step bodies stay in this process for a Worker to serve.
//
// The regions are read into the documentation at build time.
package main

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	mcpd "github.com/mcpd/mcpd/sdks/go"
)

func deliver(text, idempotencyKey string) error {
	if text == "" || idempotencyKey == "" {
		return errors.New("nothing to send")
	}
	return nil
}

func workflowExample() {
	ctx := context.Background()
	m := mcpd.New(mcpd.WithProject("support"))

	// #region workflow
	support := mcpd.NewWorkflow("support_ticket").
		Step("classify", func(ctx context.Context, step *mcpd.StepContext) (any, error) {
			var in struct {
				Ticket string `json:"ticket"`
			}
			if err := step.Input(&in); err != nil {
				return nil, err
			}
			kind := "bug"
			if strings.HasPrefix(in.Ticket, "Q") {
				kind = "question"
			}
			return map[string]any{"kind": kind}, nil
		}).
		Agent("draft", func(ctx context.Context, step *mcpd.StepContext) (any, error) {
			// Your own model call. A span started from ctx lands under this step,
			// and what it spent is the step's usage: the daemon prices the sum.
			_, span := m.Start(ctx, "model_call", mcpd.WithModel("claude-sonnet-4", "anthropic"))
			defer span.End()
			span.SetTokens(812, 37)
			return "an answer", nil
		}, mcpd.Input(map[string]any{"kind": "${steps.classify.output.kind}"})).
		WaitFor("approval", "approved", "${input.ticket}", 7*24*time.Hour,
			mcpd.Guard(mcpd.When("steps.classify.output.kind", "eq", "bug"))).
		Step("send", func(ctx context.Context, step *mcpd.StepContext) (any, error) {
			if err := deliver("the draft", step.IdempotencyKey); err != nil {
				return nil, mcpd.Retryable(err, 2*time.Second)
			}
			return map[string]any{"sent": true}, nil
		}, mcpd.Input(map[string]any{"text": "${steps.draft.output}"}), mcpd.AtMostOnce(),
			mcpd.Retry(mcpd.RetryPolicy{
				MaxAttempts: 3, Backoff: mcpd.BackoffExponential, Initial: 500 * time.Millisecond,
			})).
		Output(map[string]any{"sent": "${steps.send.output.sent}"})
	// #endregion workflow

	// #region worker
	// Serve step bodies for as long as this process lives: register, long-poll
	// for leases, heartbeat while a body runs, answer every 409 by its reason.
	worker := mcpd.NewWorker(m, mcpd.WorkerConcurrency(8)).Register(support)
	go func() { _ = worker.Serve(ctx) }()
	// #endregion worker

	// #region run
	// Start a run from anywhere that holds an execute key, answer the wait from
	// a webhook, then follow the journal to the end and read the run.
	run, err := m.StartRun(ctx, mcpd.StartRun{
		Definition: "support_ticket",
		Input:      map[string]any{"ticket": "Q-42"},
	})
	if err != nil {
		panic(err)
	}
	// A repeat of this event id is a duplicate, not a second event.
	_, err = m.PublishEvent(ctx, mcpd.PublishEvent{
		Event:   "approved",
		Key:     "Q-42",
		Payload: map[string]any{"by": "ops"},
		EventID: "hook-1",
	})
	if err != nil {
		panic(err)
	}
	_ = m.WatchRun(ctx, run.RunID, -1, func(e mcpd.RunEvent) error {
		fmt.Println(e.Seq, e.Kind)
		return nil
	})
	detail, _ := m.GetRun(ctx, run.RunID)
	fmt.Println(detail.Run.Status, string(detail.Run.Output))
	// #endregion run

	worker.Stop()
	_ = m.Shutdown(ctx)
}
