package mcpd

// The worker: the user's own process, serving step bodies to the daemon.
//
// It registers the workflows it holds, long-polls for leases, and for each
// lease posts started, heartbeats while the body runs, and posts complete or
// fail. Every 409 the daemon can answer is handled by its reason, because a
// worker that collapses them into one branch abandons steps it has already
// finished or reports a failure for a success.
//
// Nothing here runs a model or reads a stored prompt. The daemon hands the
// worker a lease with the rendered input and the prior steps; the body does
// the rest with the imports, the pool and the credentials this process holds.

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"sync"
	"sync/atomic"
	"time"
)

// Failure is an error with a class, returned from a step body to tell the
// daemon how to treat it. Any other error a body returns is fatal.
type Failure struct {
	Class      StepErrorClass
	Message    string
	RetryAfter time.Duration
	Details    any
	// Err is the error this wraps, when there is one.
	Err error
}

func (f *Failure) Error() string {
	if f.Message != "" {
		return f.Message
	}
	if f.Err != nil {
		if message := errorMessage(f.Err); message != "" {
			return message
		}
	}
	return string(f.Class)
}

// Unwrap returns the wrapped error.
func (f *Failure) Unwrap() error { return f.Err }

// Retryable marks an error transient: the daemon retries the step while its
// budget lasts, waiting at least retryAfter when it is not zero.
func Retryable(err error, retryAfter time.Duration) error {
	return &Failure{Class: StepErrorClassRetryable, Message: errText(err), RetryAfter: retryAfter, Err: err}
}

// NewFailure is an error of the given class with a message.
func NewFailure(class StepErrorClass, message string) error {
	return &Failure{Class: class, Message: message}
}

func errText(err error) string {
	if err == nil {
		return ""
	}
	return errorMessage(err)
}

// StepContext is what a step body is handed besides its context: the lease,
// the run's state, and the means to report usage and record spans. The
// handler's ctx is cancelled when the daemon fences this lease or ends the
// run, and a span started from it nests under the step the daemon opened.
type StepContext struct {
	// Lease is the lease, in full.
	Lease StepLease
	// RunID is which run.
	RunID string
	// StepKey is which step, with any loop or item index folded in.
	StepKey string
	// Attempt is which attempt, from zero.
	Attempt uint32
	// IdempotencyKey is a key for a third party that accepts one. Stable across retries of this step.
	IdempotencyKey string
	// Parent is the step's span, which the daemon opened.
	Parent TraceContext

	client *Client
	mu     sync.Mutex
	state  map[string]any
	usage  *StepUsage
	// acc is what the spans under this step reported when they ended.
	acc accumulatedUsage
}

// accumulatedUsage is the sum of what the spans under a step spent: the
// step's usage without the body adding tokens by hand.
type accumulatedUsage struct {
	any                           bool
	input, output, total          uint64
	hasInput, hasOutput, hasTotal bool
	model, provider               string
}

// Input decodes the step's rendered input into v. A value that does not fit
// is a *Failure classed invalid_input, never fatal: the input was rendered by
// the daemon from a template, so a shape the body did not expect is a
// template mistake, and one attempt is the right thing to charge for it.
func (s *StepContext) Input(v any) error { return decodeRaw(s.Lease.Input, v, "input") }

// Prior decodes the steps that already ended into v, keyed by step id. Same
// rule as Input.
func (s *StepContext) Prior(v any) error { return decodeRaw(s.Lease.Prior, v, "prior") }

// State decodes the run's accumulated state as of this lease into v. Same
// rule as Input.
func (s *StepContext) State(v any) error { return decodeRaw(s.Lease.State, v, "state") }

func decodeRaw(raw json.RawMessage, v any, what string) error {
	if len(raw) == 0 {
		return nil
	}
	if err := json.Unmarshal(raw, v); err != nil {
		return &Failure{
			Class:   StepErrorClassInvalidInput,
			Message: fmt.Sprintf("the step's %s could not be read: %v", what, err),
			Err:     err,
		}
	}
	return nil
}

// SetState merges keys into the run's state when the step completes.
// Top-level keys replace.
func (s *StepContext) SetState(patch map[string]any) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.state == nil {
		s.state = map[string]any{}
	}
	for k, v := range patch {
		s.state[k] = v
	}
}

// SetUsage reports the tokens this step spent, so the daemon prices and
// charges them. Fields set here replace fields set earlier.
func (s *StepContext) SetUsage(usage StepUsage) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.usage == nil {
		s.usage = &StepUsage{}
	}
	if usage.InputTokens != nil {
		s.usage.InputTokens = usage.InputTokens
	}
	if usage.OutputTokens != nil {
		s.usage.OutputTokens = usage.OutputTokens
	}
	if usage.TotalTokens != nil {
		s.usage.TotalTokens = usage.TotalTokens
	}
	if usage.InputDetails != nil {
		s.usage.InputDetails = usage.InputDetails
	}
	if usage.OutputDetails != nil {
		s.usage.OutputDetails = usage.OutputDetails
	}
	if usage.Model != nil {
		s.usage.Model = usage.Model
	}
	if usage.Provider != nil {
		s.usage.Provider = usage.Provider
	}
}

// SetTokens is SetUsage for the two counts most bodies have.
func (s *StepContext) SetTokens(input, output uint64) {
	s.SetUsage(StepUsage{InputTokens: &input, OutputTokens: &output})
}

// addUsage is the sink every span under this step reports to when it ends:
// the counts are summed and the last model and provider seen win.
func (s *StepContext) addUsage(report usageReport) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.acc.any = true
	if report.usage.InputTokens != nil {
		s.acc.input += *report.usage.InputTokens
		s.acc.hasInput = true
	}
	if report.usage.OutputTokens != nil {
		s.acc.output += *report.usage.OutputTokens
		s.acc.hasOutput = true
	}
	if report.usage.TotalTokens != nil {
		s.acc.total += *report.usage.TotalTokens
		s.acc.hasTotal = true
	}
	if report.model != "" {
		s.acc.model = report.model
	}
	if report.provider != "" {
		s.acc.provider = report.provider
	}
}

// Usage is what the step will report on complete: the sum of what its spans
// spent, with anything SetUsage set on top. Nil when nothing was recorded.
func (s *StepContext) Usage() *StepUsage {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.mergedUsage()
}

// mergedUsage lays the explicit usage over the accumulated one: a field
// SetUsage set replaces the sum, a field it left alone keeps the sum. The
// caller holds the lock.
func (s *StepContext) mergedUsage() *StepUsage {
	if !s.acc.any && s.usage == nil {
		return nil
	}
	out := &StepUsage{}
	if s.acc.any {
		if s.acc.hasInput {
			v := s.acc.input
			out.InputTokens = &v
		}
		if s.acc.hasOutput {
			v := s.acc.output
			out.OutputTokens = &v
		}
		if s.acc.hasTotal {
			v := s.acc.total
			out.TotalTokens = &v
		}
		out.Model = strPtr(s.acc.model)
		out.Provider = strPtr(s.acc.provider)
	}
	if explicit := s.usage; explicit != nil {
		if explicit.InputTokens != nil {
			out.InputTokens = explicit.InputTokens
		}
		if explicit.OutputTokens != nil {
			out.OutputTokens = explicit.OutputTokens
		}
		if explicit.TotalTokens != nil {
			out.TotalTokens = explicit.TotalTokens
		}
		if explicit.InputDetails != nil {
			out.InputDetails = explicit.InputDetails
		}
		if explicit.OutputDetails != nil {
			out.OutputDetails = explicit.OutputDetails
		}
		if explicit.Model != nil {
			out.Model = explicit.Model
		}
		if explicit.Provider != nil {
			out.Provider = explicit.Provider
		}
	}
	return out
}

// bodyContext is what a step body's ctx carries: the step span as the remote
// parent, the run's correlation, and this step as the sink every span under
// it reports its usage to.
func (s *StepContext) bodyContext(ctx context.Context) context.Context {
	ctx = ContextWithRunCorrelation(ContextWithRemoteParent(ctx, s.Parent), s.Lease.Correlation)
	return contextWithUsageSink(ctx, s.addUsage)
}

// Start records a span under this step in the run's trace, carrying the run's
// correlation and adding its usage to the step's. The handler's own ctx does
// the same through the client's Start; this is the explicit form for a
// context that did not come from the handler.
func (s *StepContext) Start(ctx context.Context, name string, opts ...SpanOption) (context.Context, *ActiveSpan) {
	return s.client.Start(s.bodyContext(ctx), name, opts...)
}

func (s *StepContext) collected() (json.RawMessage, *StepUsage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	var state json.RawMessage
	if s.state != nil {
		encoded, err := json.Marshal(s.state)
		if err != nil {
			return nil, nil, err
		}
		state = encoded
	}
	return state, s.mergedUsage(), nil
}

// StepOutcome is how a body finished, handed to the WorkerOnStepEnd hook.
type StepOutcome struct {
	// Status is StepOutcomeCompleted or StepOutcomeFailed.
	Status string
	// Class is the error class when the body failed.
	Class StepErrorClass
	// Message is the failure's message when the body failed.
	Message string
}

// The two ways a body finishes.
const (
	StepOutcomeCompleted = "completed"
	StepOutcomeFailed    = "failed"
)

// WorkerStats is a snapshot of what a worker has done since it was built.
// The daemon's journal records every attempt from its side; these are the
// counts only the worker can know, such as a fenced lease abandoned in
// silence, which from outside looks exactly like no work at all.
type WorkerStats struct {
	// Claimed is how many leases were taken.
	Claimed int
	// Inflight is how many bodies are running right now.
	Inflight int
	// Completed is how many bodies finished and completed their step.
	Completed int
	// Failed counts bodies that ran and failed, by error class.
	Failed map[string]int
	// Conflicts counts the 409s applied, by the daemon's reason.
	Conflicts map[string]int
	// Unservable is how many leases were refused before any body ran.
	Unservable int
}

// WorkerOption configures NewWorker.
type WorkerOption func(*Worker)

// WorkerID names this process, recorded as the owner of every lease it takes.
func WorkerID(id string) WorkerOption { return func(w *Worker) { w.id = id } }

// WorkerConcurrency is how many steps to run at once. Default 4.
func WorkerConcurrency(n int) WorkerOption {
	return func(w *Worker) {
		if n > 0 {
			w.concurrency = n
		}
	}
}

// WorkerProject is which project to serve. Needed only when the key reaches several.
func WorkerProject(project string) WorkerOption { return func(w *Worker) { w.project = project } }

// ClaimWait is how long each claim waits for work. Default 20 seconds, at
// most 60.
func ClaimWait(d time.Duration) WorkerOption {
	return func(w *Worker) {
		if d < 0 {
			d = 0
		}
		if d > 60*time.Second {
			d = 60 * time.Second
		}
		w.wait = d
	}
}

// WorkerOnError is where the worker reports its own failures. Defaults to
// the client's logger.
func WorkerOnError(fn func(message string, err error)) WorkerOption {
	return func(w *Worker) { w.onError = fn }
}

// WorkerOnLease is called for every lease claimed, before anything is posted
// for it. A hook that panics is reported through WorkerOnError and the
// worker goes on.
func WorkerOnLease(fn func(lease StepLease)) WorkerOption {
	return func(w *Worker) { w.onLease = fn }
}

// WorkerOnStepStart is called once started was accepted, just before the
// body runs.
func WorkerOnStepStart(fn func(step *StepContext)) WorkerOption {
	return func(w *Worker) { w.onStepStart = fn }
}

// WorkerOnStepEnd is called once the body finished: completed, or failed
// with a class. A lease abandoned on a 409 sees WorkerOnConflict instead.
func WorkerOnStepEnd(fn func(step *StepContext, outcome StepOutcome)) WorkerOption {
	return func(w *Worker) { w.onStepEnd = fn }
}

// WorkerOnConflict is called whenever a 409 is applied, with the daemon's
// reason: already_applied, fenced, not_started or run_terminal.
func WorkerOnConflict(fn func(lease StepLease, reason string)) WorkerOption {
	return func(w *Worker) { w.onConflict = fn }
}

// Worker serves step bodies from this process. See the package comment on
// worker.go for the protocol it drives.
type Worker struct {
	client      *Client
	id          string
	concurrency int
	project     string
	wait        time.Duration
	onError     func(string, error)
	onLease     func(StepLease)
	onStepStart func(*StepContext)
	onStepEnd   func(*StepContext, StepOutcome)
	onConflict  func(StepLease, string)
	workflows   []*Workflow
	heartbeat   time.Duration
	inflight    sync.WaitGroup
	running     atomic.Int32
	slot        chan struct{}
	stop        chan struct{}
	stopOnce    sync.Once

	statsMu     sync.Mutex
	claimed     int
	completed   int
	unservable  int
	failedBy    map[string]int
	conflictsBy map[string]int
}

// NewWorker makes a worker on the client.
func NewWorker(c *Client, opts ...WorkerOption) *Worker {
	w := &Worker{
		client:      c,
		id:          defaultWorkerID(),
		concurrency: 4,
		wait:        20 * time.Second,
		heartbeat:   10 * time.Second,
		slot:        make(chan struct{}, 1),
		stop:        make(chan struct{}),
	}
	for _, opt := range opts {
		if opt != nil {
			opt(w)
		}
	}
	if w.onError == nil {
		w.onError = func(message string, err error) { c.logf("%s: %v", message, err) }
	}
	return w
}

func defaultWorkerID() string {
	host, _ := os.Hostname()
	if host == "" {
		host = "worker"
	}
	return host + "-" + randomHex(4)
}

// Register adds the workflows this worker serves. Their definitions are
// posted by Serve or RegisterDefinitions.
func (w *Worker) Register(workflows ...*Workflow) *Worker {
	w.workflows = append(w.workflows, workflows...)
	return w
}

// ID is the name every lease records as its owner.
func (w *Worker) ID() string { return w.id }

// Inflight is how many steps are running right now.
func (w *Worker) Inflight() int { return int(w.running.Load()) }

// Stats returns the worker's counters as they stand. The maps are copies.
func (w *Worker) Stats() WorkerStats {
	w.statsMu.Lock()
	defer w.statsMu.Unlock()
	out := WorkerStats{
		Claimed:    w.claimed,
		Inflight:   w.Inflight(),
		Completed:  w.completed,
		Unservable: w.unservable,
		Failed:     make(map[string]int, len(w.failedBy)),
		Conflicts:  make(map[string]int, len(w.conflictsBy)),
	}
	for k, v := range w.failedBy {
		out.Failed[k] = v
	}
	for k, v := range w.conflictsBy {
		out.Conflicts[k] = v
	}
	return out
}

func (w *Worker) count(apply func()) {
	w.statsMu.Lock()
	defer w.statsMu.Unlock()
	apply()
}

func (w *Worker) countFailed(class StepErrorClass) {
	w.count(func() {
		if w.failedBy == nil {
			w.failedBy = map[string]int{}
		}
		w.failedBy[string(class)]++
	})
}

// hook runs one user callback inside the recording rule: a panic is reported
// through onError and the loop goes on.
func (w *Worker) hook(name string, fn func()) {
	if fn == nil {
		return
	}
	defer func() {
		if r := recover(); r != nil {
			w.onError(fmt.Sprintf("mcpd worker %s: the %s hook panicked", w.id, name), fmt.Errorf("%v", r))
		}
	}()
	fn()
}

// ended reports how a body finished: counted, then handed to the hook.
func (w *Worker) ended(step *StepContext, outcome StepOutcome) {
	if outcome.Status == StepOutcomeCompleted {
		w.count(func() { w.completed++ })
	} else {
		w.countFailed(outcome.Class)
	}
	if w.onStepEnd != nil {
		w.hook("on_step_end", func() { w.onStepEnd(step, outcome) })
	}
}

// Serve registers, then claims and serves until ctx is done or Stop is
// called. It returns once every step in flight has reported: nil after Stop,
// ctx.Err() after ctx.
func (w *Worker) Serve(ctx context.Context) error {
	if ctx == nil {
		ctx = context.Background()
	}
	if err := w.RegisterDefinitions(ctx); err != nil {
		return err
	}
	for {
		select {
		case <-ctx.Done():
			w.Drain()
			return ctx.Err()
		case <-w.stop:
			w.Drain()
			return nil
		default:
		}
		free := w.concurrency - w.Inflight()
		if free <= 0 {
			select {
			case <-w.slot:
			case <-time.After(time.Second):
			case <-ctx.Done():
			case <-w.stop:
			}
			continue
		}
		taken, err := w.Claim(ctx, free, "")
		if err != nil {
			if ctx.Err() != nil {
				continue
			}
			w.onError(fmt.Sprintf("mcpd worker %s: claim failed; retrying", w.id), err)
			w.pause(ctx, time.Second)
			continue
		}
		if taken == 0 {
			w.pause(ctx, 10*time.Millisecond)
		}
	}
}

func (w *Worker) pause(ctx context.Context, d time.Duration) {
	select {
	case <-time.After(d):
	case <-ctx.Done():
	case <-w.stop:
	}
}

// Stop makes Serve stop claiming. Serve returns once the steps in flight
// have reported; Drain waits for the same from any goroutine.
func (w *Worker) Stop() {
	w.stopOnce.Do(func() { close(w.stop) })
}

// RegisterDefinitions posts every registered definition and reads back the
// lease timings.
func (w *Worker) RegisterDefinitions(ctx context.Context) error {
	docs := make([]DefinitionDocument, 0, len(w.workflows))
	for _, wf := range w.workflows {
		doc, err := wf.Document()
		if err != nil {
			return err
		}
		docs = append(docs, doc)
	}
	registered, err := w.client.RegisterWorker(ctx, w.id, docs, w.project)
	if err != nil {
		return err
	}
	w.heartbeat = time.Duration(registered.HeartbeatMs) * time.Millisecond
	if w.heartbeat < time.Second {
		w.heartbeat = time.Second
	}
	return nil
}

// Claim makes one claim and serves what it granted on goroutines of its own,
// returning how many leases were taken. Serve calls this in a loop; a script
// that drives its own run calls it with that run's id until the run ends,
// then Drain.
func (w *Worker) Claim(ctx context.Context, max int, runID string) (int, error) {
	if max < 1 {
		max = 1
	}
	if max > 100 {
		max = 100
	}
	response, err := w.client.ClaimSteps(ctx, Claim{WorkerID: w.id, Max: max, Wait: w.wait, RunID: runID, Project: w.project})
	if err != nil {
		return 0, err
	}
	for _, lease := range response.Leases {
		lease := lease
		w.count(func() { w.claimed++ })
		if w.onLease != nil {
			w.hook("on_lease", func() { w.onLease(lease) })
		}
		w.inflight.Add(1)
		w.running.Add(1)
		go w.serve(ctx, lease)
	}
	return len(response.Leases), nil
}

// Drain waits for every step in flight.
func (w *Worker) Drain() { w.inflight.Wait() }

func (w *Worker) handlerFor(lease StepLease) StepHandler {
	for _, wf := range w.workflows {
		if wf.id != lease.DefinitionID {
			continue
		}
		if wf.version != nil && *wf.version != lease.DefinitionVersion {
			continue
		}
		if handler := wf.HandlerFor(lease.StepKey); handler != nil {
			return handler
		}
	}
	return nil
}

type outcome int

const (
	landed outcome = iota
	done
	abandon
)

func (w *Worker) serve(ctx context.Context, lease StepLease) {
	defer func() {
		w.running.Add(-1)
		w.inflight.Done()
		select {
		case w.slot <- struct{}{}:
		default:
		}
	}()
	c := w.client
	handler := w.handlerFor(lease)
	if handler == nil {
		w.count(func() { w.unservable++ })
		w.report(lease, func() error {
			_, err := c.FailStep(ctx, lease.LeaseID, StepError{
				Class:   StepErrorClassUnservable,
				Message: fmt.Sprintf("worker %s does not serve %s@%d step %s", w.id, lease.DefinitionID, lease.DefinitionVersion, lease.StepKey),
			})
			return err
		})
		return
	}

	// Before the body. Always. This is what tells the daemon the difference
	// between "offered" and "running", and on an at_most_once step it refuses
	// a complete without it.
	if w.report(lease, func() error { _, err := c.StepStarted(ctx, lease.LeaseID); return err }) == abandon {
		return
	}

	parent := TraceContext{TraceID: lease.TraceID, SpanID: lease.SpanID, Sampled: true}
	step := &StepContext{
		Lease:          lease,
		RunID:          lease.RunID,
		StepKey:        lease.StepKey,
		Attempt:        lease.Attempt,
		IdempotencyKey: lease.IdempotencyKey,
		Parent:         parent,
		client:         c,
	}
	bodyCtx, cancel := context.WithCancelCause(step.bodyContext(ctx))
	defer cancel(nil)
	// givenUp closes when the lease is given up, at its deadline or by a
	// fence. The worker stops waiting for the body then, whether or not the
	// body heeds its ctx, so the slot is freed as contract section 7.2 says.
	// A stopping worker cancels bodyCtx too, and still waits for the body.
	givenUp := make(chan struct{})
	var giveUpOnce sync.Once
	giveUp := func(cause error) {
		cancel(cause)
		giveUpOnce.Do(func() { close(givenUp) })
	}

	stopBeat := make(chan struct{})
	var beats sync.WaitGroup
	beats.Add(1)
	go func() {
		defer beats.Done()
		ticker := time.NewTicker(w.heartbeat)
		defer ticker.Stop()
		for {
			select {
			case <-stopBeat:
				return
			case <-ticker.C:
			}
			if _, err := c.StepHeartbeat(ctx, lease.LeaseID); err != nil {
				var conflict *StepConflictError
				if errors.As(err, &conflict) {
					// The daemon has moved on without us; the body learns through its ctx.
					w.conflicted(lease, conflict)
					giveUp(conflict)
					return
				}
				// A transport failure is not evidence of anything; the next beat may land.
				w.onError(fmt.Sprintf("mcpd worker %s: heartbeat failed for %s", w.id, lease.StepKey), err)
			}
		}
	}()
	defer func() {
		close(stopBeat)
		beats.Wait()
	}()

	// The step's own ceiling, enforced here as well as by the daemon: a body
	// that runs past deadline_ms is cancelled through its ctx and the attempt
	// is failed retryable, so the slot is freed and the journal says why,
	// instead of the daemon reclaiming a lease still heartbeating from inside
	// a body that will never return.
	if lease.DeadlineMs != nil && *lease.DeadlineMs > 0 {
		reason := fmt.Sprintf("step deadline of %dms passed", *lease.DeadlineMs)
		expired := make(chan struct{})
		timer := time.AfterFunc(time.Duration(*lease.DeadlineMs)*time.Millisecond, func() {
			defer close(expired)
			if bodyCtx.Err() != nil {
				return
			}
			giveUp(errors.New(reason))
			w.report(lease, func() error {
				_, err := c.FailStep(ctx, lease.LeaseID, StepError{Class: StepErrorClassRetryable, Message: reason})
				return err
			})
			w.ended(step, StepOutcome{Status: StepOutcomeFailed, Class: StepErrorClassRetryable, Message: reason})
		})
		// A timer that already fired is posting the failure on its own
		// goroutine; this lease is not finished until that post has landed.
		defer func() {
			if !timer.Stop() {
				<-expired
			}
		}()
	}

	if w.onStepStart != nil {
		w.hook("on_step_start", func() { w.onStepStart(step) })
	}

	type bodyResult struct {
		output any
		err    error
	}
	// Buffered, so a body that returns after its lease was given up does not
	// leave its goroutine blocked on a send nobody receives.
	done := make(chan bodyResult, 1)
	go func() {
		output, err := runBody(bodyCtx, handler, step)
		done <- bodyResult{output, err}
	}()
	var output any
	var err error
	select {
	case result := <-done:
		output, err = result.output, result.err
	case <-givenUp:
		// What the body returns after this is ignored.
		return
	}
	if bodyCtx.Err() != nil && ctx.Err() == nil {
		// Fenced, or the run ended: post nothing more for this lease.
		return
	}
	fail := func(failure StepError) {
		w.report(lease, func() error {
			_, err := c.FailStep(ctx, lease.LeaseID, failure)
			return err
		})
		w.ended(step, StepOutcome{Status: StepOutcomeFailed, Class: failure.Class, Message: failure.Message})
	}
	if err != nil {
		fail(toFailure(err))
		return
	}
	raw, encodeErr := rawJSON(output)
	if encodeErr != nil {
		fail(StepError{Class: StepErrorClassFatal, Message: encodeErr.Error()})
		return
	}
	state, usage, stateErr := step.collected()
	if stateErr != nil {
		fail(StepError{Class: StepErrorClassFatal, Message: "the step's state could not be encoded: " + stateErr.Error()})
		return
	}
	w.report(lease, func() error {
		_, err := c.CompleteStep(ctx, lease.LeaseID, StepResult{Output: raw, State: state, Usage: usage})
		return err
	})
	w.ended(step, StepOutcome{Status: StepOutcomeCompleted})
}

// runBody calls the handler, turning a panic into a fatal failure so one bad
// body does not take the worker down with it.
func runBody(ctx context.Context, handler StepHandler, step *StepContext) (output any, err error) {
	defer func() {
		if r := recover(); r != nil {
			output, err = nil, &Failure{Class: StepErrorClassFatal, Message: fmt.Sprintf("the step body panicked: %v", r)}
		}
	}()
	return handler(ctx, step)
}

func toFailure(err error) StepError {
	var failure *Failure
	if errors.As(err, &failure) {
		out := StepError{Class: failure.Class, Message: failure.Error()}
		if out.Class == "" {
			out.Class = StepErrorClassFatal
		}
		if failure.RetryAfter > 0 {
			after := uint64(ms(failure.RetryAfter))
			out.RetryAfterMs = &after
		}
		if failure.Details != nil {
			if raw, encodeErr := rawJSON(failure.Details); encodeErr == nil {
				out.Details = raw
			}
		}
		return out
	}
	message := errorMessage(err)
	if message == "" {
		message = "the step body returned an error"
	}
	return StepError{Class: StepErrorClassFatal, Message: message}
}

// report posts one worker call and applies the 409 table: already_applied is
// a success, and every other reason means abandon this lease.
func (w *Worker) report(lease StepLease, call func() error) outcome {
	err := call()
	if err == nil {
		return landed
	}
	var conflict *StepConflictError
	if errors.As(err, &conflict) {
		w.conflicted(lease, conflict)
		switch conflict.Conflict.Reason {
		case ConflictReasonAlreadyApplied:
			return done
		case ConflictReasonNotStarted:
			w.onError(fmt.Sprintf("mcpd worker %s: a bug in this worker: %s was reported before started", w.id, lease.StepKey), err)
			return abandon
		default:
			return abandon
		}
	}
	w.onError(fmt.Sprintf("mcpd worker %s: call failed for %s", w.id, lease.StepKey), err)
	return abandon
}

// conflicted counts a 409 by its reason and tells the hook, whatever the
// reason: a fenced lease abandoned in silence is the one an operator most
// needs to see.
func (w *Worker) conflicted(lease StepLease, conflict *StepConflictError) {
	reason := string(conflict.Conflict.Reason)
	w.count(func() {
		if w.conflictsBy == nil {
			w.conflictsBy = map[string]int{}
		}
		w.conflictsBy[reason]++
	})
	if w.onConflict != nil {
		w.hook("on_conflict", func() { w.onConflict(lease, reason) })
	}
}
