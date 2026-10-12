// The canonical run, served in Go. Prints one JSON line at the end.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	mcpd "github.com/mcpd/mcpd/sdks/go"
)

type canonicalRun struct {
	DefinitionID string            `json:"definition_id"`
	Input        map[string]any    `json:"input"`
	Correlation  map[string]string `json:"correlation"`
	Event        struct {
		Name    string         `json:"name"`
		Key     string         `json:"key"`
		Payload map[string]any `json:"payload"`
		EventID string         `json:"event_id"`
	} `json:"event"`
}

func main() {
	raw, err := os.ReadFile(filepath.Join("..", "canonical.json"))
	if err != nil {
		panic(err)
	}
	var canonical canonicalRun
	if err := json.Unmarshal(raw, &canonical); err != nil {
		panic(err)
	}
	ctx := context.Background()
	m := mcpd.New(mcpd.WithEndpoint(os.Getenv("MCPD_ENDPOINT")), mcpd.WithProject(os.Getenv("MCPD_PROJECT")))

	wf := mcpd.NewWorkflow(canonical.DefinitionID).
		Step("classify", func(ctx context.Context, step *mcpd.StepContext) (any, error) {
			var input struct {
				Ticket string `json:"ticket"`
			}
			if err := step.Input(&input); err != nil {
				return nil, err
			}
			kind := "bug"
			if strings.HasPrefix(input.Ticket, "Q") {
				kind = "question"
			}
			return map[string]any{"kind": kind}, nil
		}).
		Agent("draft", func(ctx context.Context, step *mcpd.StepContext) (any, error) {
			var input struct {
				Kind string `json:"kind"`
			}
			if err := step.Input(&input); err != nil {
				return nil, err
			}
			step.SetState(map[string]any{"drafted": true})
			_, span := m.Start(ctx, "model_call", mcpd.WithSpanKind(mcpd.SpanKindModelGeneration), mcpd.WithModel("claude-sonnet-4", "anthropic"))
			span.SetTokens(120, 40)
			span.End()
			return "an answer to a " + input.Kind, nil
		}, mcpd.Input(map[string]any{"kind": "${steps.classify.output.kind}"})).
		WaitFor("approval", "approved", "${input.ticket}", time.Minute, mcpd.AcceptParked()).
		Step("send", func(ctx context.Context, step *mcpd.StepContext) (any, error) {
			var input struct {
				Text string `json:"text"`
				By   string `json:"by"`
			}
			if err := step.Input(&input); err != nil {
				return nil, err
			}
			if step.Attempt == 0 {
				return nil, mcpd.Retryable(fmt.Errorf("relay busy"), 100*time.Millisecond)
			}
			return map[string]any{"sent": true, "by": input.By}, nil
		},
			mcpd.Input(map[string]any{"text": "${steps.draft.output}", "by": "${steps.approval.output.by}"}),
			mcpd.AtMostOnce(),
			mcpd.Retry(mcpd.RetryPolicy{MaxAttempts: 2, Backoff: mcpd.BackoffFixed, Initial: 100 * time.Millisecond}),
		).
		Output(map[string]any{"answer": "${steps.draft.output}", "sent": "${steps.send.output.sent}", "by": "${steps.send.output.by}"})

	worker := mcpd.NewWorker(m, mcpd.WorkerID("probe-go"), mcpd.ClaimWait(time.Second), mcpd.WorkerOnError(func(message string, err error) {
		panic(fmt.Sprintf("%s: %v", message, err))
	})).Register(wf)
	if err := worker.RegisterDefinitions(ctx); err != nil {
		panic(err)
	}

	session, user := canonical.Correlation["session_id"], canonical.Correlation["user_id"]
	run, err := m.StartRun(ctx, mcpd.StartRun{Definition: canonical.DefinitionID, Input: canonical.Input,
		Correlation: &mcpd.Correlation{SessionID: &session, UserID: &user}})
	if err != nil {
		panic(err)
	}
	publish := mcpd.PublishEvent{Event: canonical.Event.Name, Key: canonical.Event.Key, Payload: canonical.Event.Payload, EventID: canonical.Event.EventID}
	first, err := m.PublishEvent(ctx, publish)
	if err != nil {
		panic(err)
	}
	second, err := m.PublishEvent(ctx, publish)
	if err != nil {
		panic(err)
	}

	detail, err := m.GetRun(ctx, run.RunID)
	if err != nil {
		panic(err)
	}
	for i := 0; i < 60 && detail.Run.Status == mcpd.RunStatusRunning; i++ {
		if _, err := worker.Claim(ctx, 4, run.RunID); err != nil {
			panic(err)
		}
		worker.Drain()
		if detail, err = m.GetRun(ctx, run.RunID); err != nil {
			panic(err)
		}
		if detail.Run.Status == mcpd.RunStatusRunning {
			time.Sleep(100 * time.Millisecond)
		}
	}
	if err := m.Shutdown(ctx); err != nil {
		panic(err)
	}
	out, _ := json.Marshal(map[string]any{"run_id": run.RunID, "trace_id": run.TraceID, "status": detail.Run.Status,
		"publish": []mcpd.EventOutcome{first.Outcome, second.Outcome}})
	fmt.Println(string(out))
}
