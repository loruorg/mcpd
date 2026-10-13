// An experiment in Go: run every case of a dataset through your own code,
// score each answer against its case, and read the results.
//
// The regions are read into the documentation at build time.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"

	mcpd "github.com/mcpd/mcpd/sdks/go"
)

func experimentExample() {
	ctx := context.Background()
	m := mcpd.New(mcpd.WithProject("support"))

	// The code under test. Spans it starts from ctx nest under the case's run.
	answer := func(ctx context.Context, input json.RawMessage) (any, error) {
		var in struct {
			Question string `json:"question"`
		}
		if err := json.Unmarshal(input, &in); err != nil {
			return nil, err
		}
		_, span := m.Start(ctx, "model_call", mcpd.WithSpanKind(mcpd.SpanKindModelGeneration))
		defer span.End()
		span.SetModel("claude-sonnet-4", "anthropic")
		if strings.Contains(in.Question, "refund") {
			return "Refunds take five days.", nil
		}
		return "It ships tomorrow.", nil
	}

	// #region experiment
	// Each case runs in its own root span, four at a time. A case that returns
	// an error is recorded as a failed run, and the others still run.
	results, err := m.RunExperiment(ctx, "support-questions", answer,
		mcpd.ExperimentName("answers-v2"),
		mcpd.ExperimentScorers("exact_match", "similarity"),
		mcpd.ExperimentMetadata(map[string]any{"prompt": "answer@4"}),
	)
	if err != nil {
		panic(err)
	}

	for _, scorer := range results.Scorers {
		fmt.Printf("%s: mean %.2f over %d\n", scorer.ScorerID, scorer.Mean, scorer.N)
	}
	for _, item := range results.Items {
		if item.Status == mcpd.SpanStatusError {
			fmt.Printf("a case failed: %s\n", item.TraceID)
		}
	}

	// Read one run back as the steps its agent took.
	if len(results.Items) > 0 {
		trajectory, err := m.GetTrajectory(ctx, results.Items[0].TraceID)
		if err != nil {
			panic(err)
		}
		fmt.Printf("%d steps, tools: %s\n", trajectory.StepCount,
			strings.Join(trajectory.ToolSequence, " > "))
	}
	// #endregion experiment

	_ = m.Shutdown(ctx)
}
