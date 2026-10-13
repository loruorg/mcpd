package mcpd

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"sync"
	"time"
)

const (
	// DatasetItemAttribute is the span attribute the daemon reads to find the
	// dataset case a run answered.
	DatasetItemAttribute = "mcpd.dataset_item_id"

	// DefaultExperimentConcurrency is how many cases RunExperiment runs at
	// once when ExperimentConcurrency is not given.
	DefaultExperimentConcurrency = 4

	// maxCases is the most cases the daemon answers in one page.
	maxCases = 5000
)

// ExperimentTarget is the code under test. It is called once per dataset case
// with the case's input, and what it returns is recorded as the answer. ctx
// carries the case's root span, so spans started from it are children of the
// case. A returned error fails that case only.
type ExperimentTarget func(ctx context.Context, input json.RawMessage) (any, error)

// ExperimentOption configures RunExperiment.
type ExperimentOption func(*experimentConfig)

type experimentConfig struct {
	name        string
	scorers     []string
	config      *ScorerConfig
	concurrency int
	metadata    any
}

// ExperimentName names the experiment and each case's root span. It defaults
// to the dataset id.
func ExperimentName(name string) ExperimentOption {
	return func(c *experimentConfig) { c.name = name }
}

// ExperimentScorers chooses the scorers, as the score endpoint takes them.
// With none, the daemon runs the code scorers that need no settings.
func ExperimentScorers(scorers ...string) ExperimentOption {
	return func(c *experimentConfig) { c.scorers = append([]string(nil), scorers...) }
}

// ExperimentConfig gives the scorers their settings. Each case's expected
// answer is filled in by the daemon.
func ExperimentConfig(config ScorerConfig) ExperimentOption {
	return func(c *experimentConfig) { c.config = &config }
}

// ExperimentConcurrency is how many cases run at once. It must be at least 1.
func ExperimentConcurrency(n int) ExperimentOption {
	return func(c *experimentConfig) { c.concurrency = n }
}

// ExperimentMetadata keeps anything you want with the experiment, such as the
// prompt version under test. It must encode as JSON.
func ExperimentMetadata(metadata any) ExperimentOption {
	return func(c *experimentConfig) { c.metadata = metadata }
}

// GetTrajectory reads a run as the tool calls, model generations and handoffs
// its agent took.
func (c *Client) GetTrajectory(ctx context.Context, traceID string) (*Trajectory, error) {
	if traceID == "" {
		return nil, errors.New("mcpd: GetTrajectory needs a trace id")
	}
	out := &Trajectory{}
	path := "/v1/api/traces/" + url.PathEscape(traceID) + "/trajectory"
	if err := c.getJSON(ctx, path, c.projectValues(""), out); err != nil {
		return nil, err
	}
	return out, nil
}

// ListDatasetItems reads the cases of a dataset, at most limit of them when
// limit is above zero. It sends no project: the daemon finds it from the
// dataset.
func (c *Client) ListDatasetItems(ctx context.Context, datasetID string, limit int) ([]DatasetItem, error) {
	if datasetID == "" {
		return nil, errors.New("mcpd: ListDatasetItems needs a dataset id")
	}
	values := url.Values{}
	setInt(values, "limit", limit)
	var out []DatasetItem
	if err := c.getJSON(ctx, "/v1/api/datasets/"+url.PathEscape(datasetID)+"/items", values, &out); err != nil {
		return nil, err
	}
	return out, nil
}

// CreateExperiment records an experiment, or replaces one: the daemon upserts
// on ExperimentID, and an empty id gets a new one. The client's project is
// used when the correlation names none.
func (c *Client) CreateExperiment(ctx context.Context, experiment Experiment) (*Experiment, error) {
	if c == nil {
		return nil, ErrNoClient
	}
	if experiment.Correlation.Project == "" {
		experiment.Correlation.Project = c.cfg.project
	}
	out := &Experiment{}
	if err := c.postJSON(ctx, "/v1/api/experiments", nil, experiment, out); err != nil {
		return nil, err
	}
	return out, nil
}

// ScoreExperiment scores every run of an experiment, each against the
// expected answer of the dataset case it names. With no scorers, the daemon
// runs the code scorers that need no settings. A nil config sends none.
func (c *Client) ScoreExperiment(ctx context.Context, experimentID string, scorers []string, config *ScorerConfig) (*ScoreRunResult, error) {
	if experimentID == "" {
		return nil, errors.New("mcpd: ScoreExperiment needs an experiment id")
	}
	body := struct {
		Scorers []string      `json:"scorers"`
		Config  *ScorerConfig `json:"config,omitempty"`
	}{Scorers: scorers, Config: wireScorerConfig(config)}
	if body.Scorers == nil {
		body.Scorers = []string{}
	}
	out := &ScoreRunResult{}
	path := "/v1/api/experiments/" + url.PathEscape(experimentID) + "/score"
	if err := c.postJSON(ctx, path, c.projectValues(""), body, out); err != nil {
		return nil, err
	}
	return out, nil
}

// wireScorerConfig sends nil lists as empty ones: the daemon refuses null
// where it expects a list.
func wireScorerConfig(config *ScorerConfig) *ScorerConfig {
	if config == nil {
		return nil
	}
	out := *config
	for _, list := range []*[]string{&out.Forbidden, &out.Keywords, &out.ExpectedTools, &out.ForbiddenTools, &out.Context} {
		if *list == nil {
			*list = []string{}
		}
	}
	return &out
}

// ExperimentResults reads an experiment case by case, with a summary per
// scorer.
func (c *Client) ExperimentResults(ctx context.Context, experimentID string) (*ExperimentResults, error) {
	if experimentID == "" {
		return nil, errors.New("mcpd: ExperimentResults needs an experiment id")
	}
	out := &ExperimentResults{}
	path := "/v1/api/experiments/" + url.PathEscape(experimentID) + "/results"
	if err := c.getJSON(ctx, path, c.projectValues(""), out); err != nil {
		return nil, err
	}
	return out, nil
}

// RunExperiment runs target over every case of a dataset and scores the
// answers.
//
// Each case runs inside a root span named after the experiment, carrying the
// experiment id and the case id, on a fixed pool of goroutines, so at most
// DefaultExperimentConcurrency targets are in flight unless
// ExperimentConcurrency says otherwise. The span is kept whatever the sample
// rate, because a sampled-out case would be a case with no answer. A target
// that returns an error or panics fails its own case and the next case still
// runs. Then the runs are flushed and scored, the experiment is marked
// succeeded with each scorer's mean, and the results are returned.
//
// When the runner itself fails, the experiment is marked failed with the
// message and the error is returned. A disabled client is refused before the
// first request, because none of its runs would reach the daemon.
func (c *Client) RunExperiment(ctx context.Context, datasetID string, target ExperimentTarget, opts ...ExperimentOption) (*ExperimentResults, error) {
	if c == nil {
		return nil, ErrNoClient
	}
	if !c.enabled {
		return nil, errors.New("mcpd: RunExperiment needs an enabled client, since no run would reach the daemon to be scored; turn recording on with WithEnabled(true)")
	}
	if datasetID == "" || target == nil {
		return nil, errors.New("mcpd: RunExperiment needs a dataset id and a target")
	}
	cfg := experimentConfig{name: datasetID, concurrency: DefaultExperimentConcurrency}
	for _, opt := range opts {
		if opt != nil {
			opt(&cfg)
		}
	}
	if cfg.concurrency < 1 {
		return nil, fmt.Errorf("mcpd: RunExperiment concurrency must be at least 1, got %d; pass ExperimentConcurrency(1) or more", cfg.concurrency)
	}
	if ctx == nil {
		ctx = context.Background()
	}
	started := time.Now().UTC()
	draft := Experiment{
		Name:        cfg.name,
		DatasetID:   datasetID,
		Status:      ExperimentStatusRunning,
		StartedAt:   &started,
		Correlation: Correlation{Project: c.cfg.project},
	}
	if cfg.metadata != nil {
		raw, err := json.Marshal(cfg.metadata)
		if err != nil {
			return nil, fmt.Errorf("mcpd: RunExperiment metadata could not be encoded as JSON: %w", err)
		}
		draft.Metadata = raw
	}

	experiment, err := c.CreateExperiment(ctx, draft)
	if err != nil {
		return nil, err
	}
	results, err := c.runCreated(ctx, *experiment, target, cfg)
	if err != nil {
		finished := time.Now().UTC()
		message := err.Error()
		failed := *experiment
		failed.Status = ExperimentStatusFailed
		failed.FinishedAt = &finished
		failed.Error = &message
		// The caller's context may be what failed, and the failed status
		// still has to be recorded.
		if _, recordErr := c.CreateExperiment(context.WithoutCancel(ctx), failed); recordErr != nil {
			c.logf("RunExperiment could not record the failed status of experiment %s: %v",
				experiment.ExperimentID, recordErr)
		}
		return nil, err
	}
	return results, nil
}

// runCreated is every step after the experiment exists, so any failure in it
// is recorded on the experiment.
func (c *Client) runCreated(ctx context.Context, experiment Experiment, target ExperimentTarget, cfg experimentConfig) (*ExperimentResults, error) {
	items, err := c.ListDatasetItems(ctx, experiment.DatasetID, maxCases)
	if err != nil {
		return nil, err
	}

	// A fixed pool pulls the next case, so at most cfg.concurrency targets are
	// in flight however long the dataset is.
	cases := make(chan DatasetItem)
	var wg sync.WaitGroup
	for i := 0; i < min(cfg.concurrency, len(items)); i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for item := range cases {
				c.runCase(ctx, experiment, item, target)
			}
		}()
	}
feed:
	for _, item := range items {
		select {
		case cases <- item:
		case <-ctx.Done():
			break feed
		}
	}
	close(cases)
	wg.Wait()
	if err := ctx.Err(); err != nil {
		return nil, fmt.Errorf("mcpd: RunExperiment stopped before every case ran: %w", err)
	}

	if err := c.Flush(ctx); err != nil {
		return nil, fmt.Errorf("mcpd: the runs did not reach the daemon before the flush deadline, so none was scored: %w", err)
	}
	if _, err := c.ScoreExperiment(ctx, experiment.ExperimentID, cfg.scorers, cfg.config); err != nil {
		return nil, err
	}
	results, err := c.ExperimentResults(ctx, experiment.ExperimentID)
	if err != nil {
		return nil, err
	}
	summary := make(map[string]float64, len(results.Scorers))
	for _, scorer := range results.Scorers {
		summary[scorer.ScorerID] = scorer.Mean
	}
	rawSummary, err := json.Marshal(summary)
	if err != nil {
		return nil, fmt.Errorf("mcpd: the experiment summary could not be encoded as JSON: %w", err)
	}
	finished := time.Now().UTC()
	done := experiment
	done.Status = ExperimentStatusSucceeded
	done.FinishedAt = &finished
	done.ItemCount = uint32(len(items))
	done.Summary = rawSummary
	if _, err := c.CreateExperiment(ctx, done); err != nil {
		return nil, err
	}
	return results, nil
}

// runCase runs one case inside its root span. Whatever the target does, it
// fails this case only: the error or panic is recorded on the span.
func (c *Client) runCase(ctx context.Context, experiment Experiment, item DatasetItem, target ExperimentTarget) {
	caseCtx, span := c.Trace(ctx, experiment.Name,
		WithSpanKind(SpanKindAgentRun),
		WithInput(item.Input),
		WithAttributes(map[string]any{DatasetItemAttribute: item.ItemID}),
		func(sc *spanConfig) { sc.experimentID, sc.keep = experiment.ExperimentID, true },
	)
	defer span.End()
	defer func() {
		if recovered := recover(); recovered != nil {
			span.RecordError(fmt.Errorf("the target panicked: %v", recovered), ErrorStack(Stack()))
		}
	}()
	output, err := target(caseCtx, item.Input)
	if err != nil {
		span.RecordError(err)
		return
	}
	if output != nil {
		span.SetOutput(output)
	}
}
