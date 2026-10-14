package mcpd

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"
	"time"
)

// TestLiveExperimentRunsOverARealDataset runs RunExperiment against a real
// daemon with the shapes a fake collector would accept and the daemon might
// not: no scorers, a scorer config with every list left nil, a case whose
// input is JSON null, and one with a null and an empty list inside it.
func TestLiveExperimentRunsOverARealDataset(t *testing.T) {
	const project = "sdk-live-go-experiment"
	c := liveClient(t, project)
	ctx := context.Background()

	datasetID := fmt.Sprintf("go-live-%d", time.Now().UnixNano())
	dataset := Dataset{DatasetID: datasetID, Name: datasetID, CreatedAt: time.Now().UTC(),
		Correlation: Correlation{Project: project}}
	if err := c.postJSON(ctx, "/v1/api/datasets", nil, dataset, nil); err != nil {
		t.Fatalf("the dataset should be created: %v", err)
	}
	items := []DatasetItem{
		{ItemID: datasetID + "-a", Input: json.RawMessage(`{"q":"hi","n":null,"l":[]}`), Expected: json.RawMessage(`"hi"`)},
		{ItemID: datasetID + "-b", Input: json.RawMessage(`null`), Expected: json.RawMessage(`"x"`)},
	}
	if err := c.postJSON(ctx, "/v1/api/datasets/"+datasetID+"/items", nil, items, nil); err != nil {
		t.Fatalf("the cases should be added: %v", err)
	}

	seen := make(chan string, len(items))
	target := func(_ context.Context, input json.RawMessage) (any, error) {
		seen <- string(input)
		return "hi", nil
	}
	needle := "hi"
	results, err := c.RunExperiment(ctx, datasetID, target,
		ExperimentConfig(ScorerConfig{Needle: &needle}))
	if err != nil {
		t.Fatalf("the daemon should accept every request the runner sends: %v", err)
	}
	close(seen)
	inputs := map[string]bool{}
	for in := range seen {
		inputs[in] = true
	}
	for _, want := range []string{`{"l":[],"n":null,"q":"hi"}`, `null`} {
		if !inputs[want] {
			t.Errorf("the target should get the case input %s as stored, got %v", want, inputs)
		}
	}
	if results.Runs != 2 || results.Failed != 0 || len(results.Items) != 2 {
		t.Fatalf("results: runs %d, failed %d, items %d; want 2, 0, 2",
			results.Runs, results.Failed, len(results.Items))
	}

	recorded := map[string]string{}
	for _, item := range results.Items {
		detail, err := c.GetTrace(ctx, item.TraceID, "spans")
		if err != nil {
			t.Fatalf("the case run should read back: %v", err)
		}
		for _, span := range detail.Spans {
			if span.IsRoot && item.ItemID != nil {
				recorded[*item.ItemID] = string(span.Input)
			}
		}
	}
	if got := recorded[datasetID+"-a"]; got != `{"l":[],"n":null,"q":"hi"}` {
		t.Errorf("the case input should be recorded as given, got %s", got)
	}
	if got := recorded[datasetID+"-b"]; got != "" && got != "null" {
		t.Errorf("a null case input should be recorded as null or not at all, got %s", got)
	}

	// Nil scorers and a nil config are the other empty shapes.
	experiment, err := c.CreateExperiment(ctx, Experiment{Name: "go-live-empty", DatasetID: datasetID,
		Status: ExperimentStatusRunning})
	if err != nil {
		t.Fatalf("the experiment should be created: %v", err)
	}
	if _, err := c.ScoreExperiment(ctx, experiment.ExperimentID, nil, nil); err != nil {
		t.Fatalf("nil scorers and a nil config should be accepted: %v", err)
	}
	if _, err := c.ScoreExperiment(ctx, experiment.ExperimentID, nil, &ScorerConfig{}); err != nil {
		t.Fatalf("a config with every list nil should be accepted: %v", err)
	}
}
