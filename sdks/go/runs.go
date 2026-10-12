package mcpd

// The run surface of the daemon, as a client: definitions, runs, events, and
// the worker protocol Worker drives.
//
// These calls return errors, like the read API, because a run you asked for
// and did not get is news. A worker call refused with a 409 returns a
// *StepConflictError carrying the daemon's closed reason, which is what lets a
// worker tell a fence from its own landed call.

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strconv"
	"time"
)

// StepConflictError is a worker call the daemon refused with one of the four
// closed reasons. Unwrap yields the *APIError it came from.
type StepConflictError struct {
	// Conflict is the daemon's answer in full.
	Conflict StepConflict
	api      *APIError
}

func (e *StepConflictError) Error() string {
	return fmt.Sprintf("mcpd: %s: %s", e.Conflict.Reason, e.Conflict.Message)
}

// Unwrap returns the underlying *APIError.
func (e *StepConflictError) Unwrap() error { return e.api }

// typedConflict turns a 409 with a conflict body into a *StepConflictError
// and leaves every other error as it is.
func typedConflict(err error) error {
	var apiErr *APIError
	if !errors.As(err, &apiErr) || apiErr.Status != http.StatusConflict || len(apiErr.Body) == 0 {
		return err
	}
	var conflict StepConflict
	if json.Unmarshal(apiErr.Body, &conflict) != nil || conflict.Reason == "" {
		return err
	}
	return &StepConflictError{Conflict: conflict, api: apiErr}
}

func rawJSON(value any) (json.RawMessage, error) {
	if value == nil {
		return nil, nil
	}
	if raw, ok := value.(json.RawMessage); ok {
		return raw, nil
	}
	encoded, err := json.Marshal(value)
	if err != nil {
		return nil, fmt.Errorf("mcpd: value could not be encoded: %w", err)
	}
	return encoded, nil
}

func (c *Client) projectOrDefault(project string) *string {
	if project == "" && c != nil {
		project = c.cfg.project
	}
	if project == "" {
		return nil
	}
	return &project
}

// PutDefinition stores one version of a definition and reads the version
// back. An empty project means the client's own.
func (c *Client) PutDefinition(ctx context.Context, doc DefinitionDocument, project string) (*DefinitionVersion, error) {
	out := &DefinitionVersion{}
	if err := c.postJSON(ctx, "/v1/api/definitions", c.projectValues(project), doc, out); err != nil {
		return nil, err
	}
	return out, nil
}

// GetDefinition reads one definition: the latest version, or the one named
// when version is not zero.
func (c *Client) GetDefinition(ctx context.Context, id string, version uint32, project string) (*Definition, error) {
	values := c.projectValues(project)
	setUint(values, "version", uint64(version))
	out := &Definition{}
	if err := c.getJSON(ctx, "/v1/api/definitions/"+url.PathEscape(id), values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// ListDefinitions reads every stored version in the project, or every version
// of one id when id is not empty.
func (c *Client) ListDefinitions(ctx context.Context, id string, project string) ([]Definition, error) {
	values := c.projectValues(project)
	setString(values, "id", id)
	var out []Definition
	if err := c.getJSON(ctx, "/v1/api/definitions", values, &out); err != nil {
		return nil, err
	}
	return out, nil
}

// StartRun is what StartRun takes.
type StartRun struct {
	// Definition is which definition to run.
	Definition string
	// Version pins one; zero means the highest stored version at start time.
	Version uint32
	// Input is the run's input, validated against the definition's input_schema when it has one.
	Input any
	// Correlation carries session, user and the other fields the run's spans record.
	Correlation *Correlation
	// IdempotencyKey is unique per project while held: a retry of this call returns the same run.
	IdempotencyKey string
	// Project is which project. Needed only when the key reaches several.
	Project string
}

// StartRun starts a run. The response is the run as stored, with its trace id.
func (c *Client) StartRun(ctx context.Context, req StartRun) (*Run, error) {
	input, err := rawJSON(req.Input)
	if err != nil {
		return nil, err
	}
	body := StartRunRequest{
		Project:      c.projectOrDefault(req.Project),
		DefinitionID: req.Definition,
		Input:        input,
	}
	if req.Version > 0 {
		body.Version = &req.Version
	}
	if req.Correlation != nil {
		cor := *req.Correlation
		if cor.Project == "" && body.Project != nil {
			cor.Project = *body.Project
		}
		body.Correlation = &cor
	}
	if req.IdempotencyKey != "" {
		body.IdempotencyKey = &req.IdempotencyKey
	}
	out := &Run{}
	if err := c.postJSON(ctx, "/v1/api/runs", nil, body, out); err != nil {
		return nil, err
	}
	return out, nil
}

// GetRun reads one run and everything durable about it: its journal,
// deadlines and waits.
func (c *Client) GetRun(ctx context.Context, runID string) (*RunDetail, error) {
	out := &RunDetail{}
	if err := c.getJSON(ctx, "/v1/api/runs/"+url.PathEscape(runID), nil, out); err != nil {
		return nil, err
	}
	return out, nil
}

// RunQuery is what ListRuns takes.
type RunQuery struct {
	Project      string
	Status       RunStatus
	DefinitionID string
	Limit        int
	Cursor       string
}

// ListRuns reads runs in the project, most recently updated first.
func (c *Client) ListRuns(ctx context.Context, query RunQuery) (*RunListResponse, error) {
	values := c.projectValues(query.Project)
	setString(values, "status", string(query.Status))
	setString(values, "definition_id", query.DefinitionID)
	setInt(values, "limit", query.Limit)
	setString(values, "cursor", query.Cursor)
	out := &RunListResponse{}
	if err := c.getJSON(ctx, "/v1/api/runs", values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// CancelRun ends a run and every child run under it. Cancelling an ended run
// changes nothing.
func (c *Client) CancelRun(ctx context.Context, runID string) (*Run, error) {
	out := &Run{}
	if err := c.postJSON(ctx, "/v1/api/runs/"+url.PathEscape(runID)+"/cancel", nil, nil, out); err != nil {
		return nil, err
	}
	return out, nil
}

// RunEvents reads a page of the run's journal, from after exclusive. Pass -1
// to start at the beginning.
func (c *Client) RunEvents(ctx context.Context, runID string, after int64, limit int) (*RunEventPage, error) {
	values := url.Values{}
	values.Set("after", strconv.FormatInt(after, 10))
	setInt(values, "limit", limit)
	out := &RunEventPage{}
	if err := c.getJSON(ctx, "/v1/api/runs/"+url.PathEscape(runID)+"/events", values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// WatchRun hands every event of a run to fn as it happens, from after
// exclusive, until the run ends, ctx is done, or fn returns an error. A client
// that stops and starts again passes the last seq it saw and misses nothing
// that was committed.
func (c *Client) WatchRun(ctx context.Context, runID string, after int64, fn func(RunEvent) error) error {
	return c.watchRun(ctx, runID, after, 500*time.Millisecond, fn)
}

func (c *Client) watchRun(ctx context.Context, runID string, after int64, poll time.Duration, fn func(RunEvent) error) error {
	if ctx == nil {
		ctx = context.Background()
	}
	for {
		page, err := c.RunEvents(ctx, runID, after, 500)
		if err != nil {
			return err
		}
		ended := false
		for _, event := range page.Events {
			if int64(event.Seq) > after {
				after = int64(event.Seq)
			}
			if err := fn(event); err != nil {
				return err
			}
			if event.Kind == RunEventKindRunEnded {
				ended = true
			}
		}
		if ended {
			return nil
		}
		if page.HasMore {
			continue
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(poll):
		}
	}
}

// RunResult is what WaitRun answers: the run as it ended, with its output to
// hand.
type RunResult struct {
	// Status is always succeeded; any other ending is a *RunFailedError.
	Status RunStatus
	// Output is what the definition's output template rendered.
	Output json.RawMessage
	// CostUSD is the priced usage the steps reported.
	CostUSD float64
	// StepsUsed is how many attempts the run spent.
	StepsUsed uint32
	// TraceID is the trace every step of the run landed in.
	TraceID string
	// Run is the run in full.
	Run Run
}

// OutputAs decodes the run's output into v.
func (r *RunResult) OutputAs(v any) error {
	if len(r.Output) == 0 {
		return nil
	}
	return json.Unmarshal(r.Output, v)
}

// RunFailedError is a run that ended failed, timed_out or cancelled, returned
// by WaitRun with the run attached.
type RunFailedError struct {
	Run Run
}

func (e *RunFailedError) Error() string {
	if e.Run.Error != nil {
		return fmt.Sprintf("mcpd: run %s %s: %s", e.Run.RunID, e.Run.Status, *e.Run.Error)
	}
	return fmt.Sprintf("mcpd: run %s %s", e.Run.RunID, e.Run.Status)
}

// WaitRun waits for a run to end and returns it with its output. A run that
// ended failed, timed_out or cancelled is a *RunFailedError carrying the run.
// Bound the wait with ctx: its deadline or cancellation is returned as is.
func (c *Client) WaitRun(ctx context.Context, runID string, after int64) (*RunResult, error) {
	return c.waitRun(ctx, runID, after, 500*time.Millisecond)
}

func (c *Client) waitRun(ctx context.Context, runID string, after int64, poll time.Duration) (*RunResult, error) {
	if ctx == nil {
		ctx = context.Background()
	}
	for {
		page, err := c.RunEvents(ctx, runID, after, 500)
		if err != nil {
			return nil, err
		}
		ended := false
		for _, event := range page.Events {
			if int64(event.Seq) > after {
				after = int64(event.Seq)
			}
			if event.Kind == RunEventKindRunEnded {
				ended = true
			}
		}
		if ended {
			break
		}
		if page.HasMore {
			continue
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-time.After(poll):
		}
	}
	detail, err := c.GetRun(ctx, runID)
	if err != nil {
		return nil, err
	}
	run := detail.Run
	if run.Status != RunStatusSucceeded {
		return nil, &RunFailedError{Run: run}
	}
	return &RunResult{
		Status:    run.Status,
		Output:    run.Output,
		CostUSD:   run.CostUsd,
		StepsUsed: run.StepsUsed,
		TraceID:   run.TraceID,
		Run:       run,
	}, nil
}

// StartRunAndWait starts a run and waits for it to end.
func (c *Client) StartRunAndWait(ctx context.Context, req StartRun) (*RunResult, error) {
	run, err := c.StartRun(ctx, req)
	if err != nil {
		return nil, err
	}
	return c.WaitRun(ctx, run.RunID, -1)
}

// SignalRun delivers an event to one run through its own open wait. eventID
// may be empty; a repeat of a supplied id is answered duplicate.
func (c *Client) SignalRun(ctx context.Context, runID string, event string, payload any, eventID string) (*EventReceipt, error) {
	raw, err := rawJSON(payload)
	if err != nil {
		return nil, err
	}
	body := map[string]any{"event": event}
	if raw != nil {
		body["payload"] = raw
	}
	if eventID != "" {
		body["event_id"] = eventID
	}
	out := &EventReceipt{}
	if err := c.postJSON(ctx, "/v1/api/runs/"+url.PathEscape(runID)+"/signal", nil, body, out); err != nil {
		return nil, err
	}
	return out, nil
}

// PublishEvent is what PublishEvent takes.
type PublishEvent struct {
	Event   string
	Key     string
	Payload any
	// EventID is yours to supply from any handler that can be retried, such as
	// a webhook: a repeat of a supplied id is answered duplicate and writes nothing.
	EventID string
	Project string
}

// PublishEvent routes an event by name and key to the run waiting for it.
func (c *Client) PublishEvent(ctx context.Context, ev PublishEvent) (*EventReceipt, error) {
	raw, err := rawJSON(ev.Payload)
	if err != nil {
		return nil, err
	}
	body := PublishEventRequest{Project: c.projectOrDefault(ev.Project), Event: ev.Event, Key: ev.Key, Payload: raw}
	if ev.EventID != "" {
		body.EventID = &ev.EventID
	}
	out := &EventReceipt{}
	if err := c.postJSON(ctx, "/v1/api/events", nil, body, out); err != nil {
		return nil, err
	}
	return out, nil
}

// RegisterWorker announces a worker and stores what it serves.
func (c *Client) RegisterWorker(ctx context.Context, workerID string, definitions []DefinitionDocument, project string) (*WorkerRegistered, error) {
	if definitions == nil {
		definitions = []DefinitionDocument{}
	}
	body := WorkerRegistration{Project: c.projectOrDefault(project), WorkerID: workerID, Definitions: definitions}
	out := &WorkerRegistered{}
	if err := c.postJSON(ctx, "/v1/api/workers/register", nil, body, out); err != nil {
		return nil, err
	}
	return out, nil
}

// Claim is what ClaimSteps takes.
type Claim struct {
	WorkerID string
	// Max is how many leases to take at most, 1 to 100.
	Max int
	// Wait is how long the daemon holds the request open for work.
	Wait time.Duration
	// RunID limits the claim to one run.
	RunID   string
	Project string
}

// ClaimSteps offers up to Max runnable steps, waiting up to Wait for one.
func (c *Client) ClaimSteps(ctx context.Context, req Claim) (*ClaimResponse, error) {
	body := ClaimRequest{Project: c.projectOrDefault(req.Project), WorkerID: req.WorkerID, Max: uint32(req.Max), WaitMs: uint32(ms(req.Wait))}
	if req.RunID != "" {
		body.RunID = &req.RunID
	}
	out := &ClaimResponse{}
	if err := c.postJSONHeld(ctx, "/v1/api/steps/claim", nil, body, out, req.Wait); err != nil {
		return nil, err
	}
	return out, nil
}

// StepStarted declares that the body is running. Required under at_most_once.
func (c *Client) StepStarted(ctx context.Context, leaseID string) (*LeaseState, error) {
	out := &LeaseState{}
	if err := c.postJSON(ctx, "/v1/api/steps/"+url.PathEscape(leaseID)+"/started", nil, nil, out); err != nil {
		return nil, typedConflict(err)
	}
	return out, nil
}

// StepHeartbeat extends the lease. The only evidence the daemon has that a
// worker is alive.
func (c *Client) StepHeartbeat(ctx context.Context, leaseID string) (*LeaseState, error) {
	out := &LeaseState{}
	if err := c.postJSON(ctx, "/v1/api/steps/"+url.PathEscape(leaseID)+"/heartbeat", nil, nil, out); err != nil {
		return nil, typedConflict(err)
	}
	return out, nil
}

// CompleteStep finishes the step.
func (c *Client) CompleteStep(ctx context.Context, leaseID string, result StepResult) (*StepAck, error) {
	out := &StepAck{}
	if err := c.postJSON(ctx, "/v1/api/steps/"+url.PathEscape(leaseID)+"/complete", nil, result, out); err != nil {
		return nil, typedConflict(err)
	}
	return out, nil
}

// FailStep fails the step, with the class that decides whether it is retried.
func (c *Client) FailStep(ctx context.Context, leaseID string, failure StepError) (*StepAck, error) {
	out := &StepAck{}
	if err := c.postJSON(ctx, "/v1/api/steps/"+url.PathEscape(leaseID)+"/fail", nil, StepFailure{Error: failure}, out); err != nil {
		return nil, typedConflict(err)
	}
	return out, nil
}
