package mcpd

// The authoring surface for durable workflows.
//
// A workflow is data the daemon holds and bodies the daemon never sees. The
// builder produces both from one file: Document is the definition the daemon
// walks, in the shape spec/definition.schema.json describes, and the handlers
// stay in this process for a Worker to serve. There is no YAML to keep in step
// with the code, because the code is where the graph is written.
//
// Every construct of the run contract has a method here. A branch, a foreach
// body and a loop body are one claimable step each; a multi-step body is a
// Subworkflow.

import (
	"context"
	"encoding/json"
	"fmt"
	"regexp"
	"strings"
	"time"
)

// Predicate is a guard the daemon evaluates: a leaf from When, or All, Any and
// Not over leaves. A path reads input, steps.<id>, run, and item, index or
// iter.index where they apply.
type Predicate map[string]any

// When is a leaf predicate: When("steps.order.output.amount_usd", "gt", 100).
// op is one of eq, ne, lt, lte, gt, gte, in, exists, matches; exists takes no
// value.
func When(path, op string, value ...any) Predicate {
	p := Predicate{"path": path, "op": op}
	if len(value) > 0 {
		p["value"] = value[0]
	}
	return p
}

// All holds when every predicate holds.
func All(predicates ...Predicate) Predicate { return Predicate{"all": predicates} }

// Any holds when at least one predicate holds.
func Any(predicates ...Predicate) Predicate { return Predicate{"any": predicates} }

// Not holds when the predicate does not.
func Not(predicate Predicate) Predicate { return Predicate{"not": predicate} }

// RetryPolicy says how failures classed retryable are retried. MaxAttempts
// counts every attempt including the first, so one means no retry.
type RetryPolicy struct {
	MaxAttempts int
	Backoff     Backoff
	Initial     time.Duration
	Max         time.Duration
}

func (r RetryPolicy) wire() map[string]any {
	out := map[string]any{}
	if r.MaxAttempts > 0 {
		out["max_attempts"] = r.MaxAttempts
	}
	if r.Backoff != "" {
		out["backoff"] = r.Backoff
	}
	if r.Initial > 0 {
		out["initial_ms"] = ms(r.Initial)
	}
	if r.Max > 0 {
		out["max_ms"] = ms(r.Max)
	}
	return out
}

// Limits are the ceilings a workflow declares.
type Limits struct {
	MaxSteps   int
	RunTimeout time.Duration
	MaxCostUSD float64
}

func (l Limits) wire() map[string]any {
	out := map[string]any{}
	if l.MaxSteps > 0 {
		out["max_steps"] = l.MaxSteps
	}
	if l.RunTimeout > 0 {
		out["run_timeout_ms"] = ms(l.RunTimeout)
	}
	if l.MaxCostUSD > 0 {
		out["max_cost_usd"] = l.MaxCostUSD
	}
	return out
}

// StepHandler is a step body. It returns the step's output, which is sent as
// JSON, or an error. An error that is a *Failure carries its own class;
// any other error is fatal. ctx is cancelled when the lease is lost or the run
// ends, and a span started from it nests under the step in the run's trace.
type StepHandler func(ctx context.Context, step *StepContext) (any, error)

type node map[string]any

type stepConfig struct {
	when         Predicate
	input        any
	hasInput     bool
	effect       Effect
	timeout      time.Duration
	retry        *RetryPolicy
	onError      OnError
	acceptParked *bool
	onTimeout    WaitTimeout
	concurrency  int
	until        Predicate
	version      uint32
	hasVersion   bool
}

// StepOption configures one node of a workflow. Each option says which nodes
// read it; the others ignore it.
type StepOption func(*stepConfig)

// Guard skips the node, writing it as skipped, when the predicate is false.
func Guard(predicate Predicate) StepOption {
	return func(c *stepConfig) { c.when = predicate }
}

// Input is the node's input template. Default: the run's input. ${path}
// references are rendered per attempt. Read by Step, Agent and Subworkflow.
func Input(template any) StepOption {
	return func(c *stepConfig) { c.input, c.hasInput = template, true }
}

// WithEffect says what a duplicated body would cost. Default at_least_once.
func WithEffect(effect Effect) StepOption {
	return func(c *stepConfig) { c.effect = effect }
}

// AtMostOnce is WithEffect(EffectAtMostOnce): started must be posted before
// the body, and a started attempt whose lease is lost fails rather than runs
// again.
func AtMostOnce() StepOption { return WithEffect(EffectAtMostOnce) }

// StepTimeout is one attempt's own ceiling, heartbeats or not.
func StepTimeout(d time.Duration) StepOption {
	return func(c *stepConfig) { c.timeout = d }
}

// Retry sets how failures classed retryable are retried.
func Retry(policy RetryPolicy) StepOption {
	return func(c *stepConfig) { c.retry = &policy }
}

// OnFailure says what happens after the retries. Default fail_run.
func OnFailure(onError OnError) StepOption {
	return func(c *stepConfig) { c.onError = onError }
}

// ContinueOnError is OnFailure(OnErrorContinue): the step is marked failed and
// the run moves on.
func ContinueOnError() StepOption { return OnFailure(OnErrorContinue) }

// AcceptParked lets a WaitFor take an event that arrived before the wait
// opened.
func AcceptParked() StepOption {
	t := true
	return func(c *stepConfig) { c.acceptParked = &t }
}

// OnTimeout says what a WaitFor does when its deadline passes. Default fail.
func OnTimeout(timeout WaitTimeout) StepOption {
	return func(c *stepConfig) { c.onTimeout = timeout }
}

// Concurrency bounds how many ForEach items run at once.
func Concurrency(n int) StepOption {
	return func(c *stepConfig) { c.concurrency = n }
}

// Until ends a Loop early. It is evaluated after each iteration and reads
// steps.<body id> for that iteration.
func Until(predicate Predicate) StepOption {
	return func(c *stepConfig) { c.until = predicate }
}

// SubworkflowVersion pins the child definition's version. Absent, the highest
// stored version at start time is used.
func SubworkflowVersion(version uint32) StepOption {
	return func(c *stepConfig) { c.version, c.hasVersion = version, true }
}

var stepID = regexp.MustCompile(`^[a-z][a-z0-9_]*$`)

func checkID(id string) error {
	if !stepID.MatchString(id) {
		return fmt.Errorf("mcpd: %q is not a step id; an id matches ^[a-z][a-z0-9_]*$", id)
	}
	return nil
}

func ms(d time.Duration) int64 { return int64(d / time.Millisecond) }

func configure(opts []StepOption) stepConfig {
	cfg := stepConfig{}
	for _, opt := range opts {
		if opt != nil {
			opt(&cfg)
		}
	}
	return cfg
}

func common(n node, cfg stepConfig) node {
	if cfg.when != nil {
		n["when"] = cfg.when
	}
	if cfg.onError != "" {
		n["on_error"] = cfg.onError
	}
	return n
}

func claimable(kind StepKind, id string, cfg stepConfig) node {
	n := common(node{"kind": kind, "id": id}, cfg)
	if cfg.hasInput {
		n["input"] = cfg.input
	}
	if cfg.effect != "" {
		n["effect"] = cfg.effect
	}
	if cfg.timeout > 0 {
		n["step_timeout_ms"] = ms(cfg.timeout)
	}
	if cfg.retry != nil {
		n["retry"] = cfg.retry.wire()
	}
	return n
}

// Branch is what a parallel branch, a foreach body or a loop body may hold:
// one claimable step each.
type Branch struct {
	nodes    []node
	handlers map[string]StepHandler
	err      error
}

// Step adds a branch or body that a worker serves.
func (b *Branch) Step(id string, handler StepHandler, opts ...StepOption) *Branch {
	return b.add(StepKindTask, id, handler, opts)
}

// Agent adds the same, recorded as an agent turn.
func (b *Branch) Agent(id string, handler StepHandler, opts ...StepOption) *Branch {
	return b.add(StepKindAgent, id, handler, opts)
}

func (b *Branch) add(kind StepKind, id string, handler StepHandler, opts []StepOption) *Branch {
	if err := checkID(id); err != nil {
		if b.err == nil {
			b.err = err
		}
		return b
	}
	if b.handlers == nil {
		b.handlers = map[string]StepHandler{}
	}
	b.nodes = append(b.nodes, claimable(kind, id, configure(opts)))
	b.handlers[id] = handler
	return b
}

// Workflow is a workflow under construction. Steps run in the order they are
// added. A mistake in the graph, such as an id the daemon would refuse, is
// kept and returned by Document, so a builder chain reads without error
// checks at every step.
type Workflow struct {
	id          string
	version     *uint32
	inputSchema any
	limits      *Limits
	nodes       []node
	handlers    map[string]StepHandler
	bodyOf      map[string]string
	output      any
	hasOutput   bool
	err         error
}

// WorkflowOption configures NewWorkflow.
type WorkflowOption func(*Workflow)

// WithVersion names the version this document owns. Without it the daemon
// assigns the next number.
func WithVersion(version uint32) WorkflowOption {
	return func(w *Workflow) { w.version = &version }
}

// WithInputSchema is a JSON Schema the run's input must satisfy.
func WithInputSchema(schema any) WorkflowOption {
	return func(w *Workflow) { w.inputSchema = schema }
}

// WithLimits sets the workflow's ceilings.
func WithLimits(limits Limits) WorkflowOption {
	return func(w *Workflow) { w.limits = &limits }
}

// NewWorkflow starts building a workflow. id is unique within the project.
func NewWorkflow(id string, opts ...WorkflowOption) *Workflow {
	w := &Workflow{id: id, handlers: map[string]StepHandler{}, bodyOf: map[string]string{}}
	w.err = checkID(id)
	for _, opt := range opts {
		if opt != nil {
			opt(w)
		}
	}
	return w
}

// ID is the definition id.
func (w *Workflow) ID() string { return w.id }

// Version is the version this builder owns, when it named one.
func (w *Workflow) Version() (uint32, bool) {
	if w.version == nil {
		return 0, false
	}
	return *w.version, true
}

// Err is the first mistake made while building, if any.
func (w *Workflow) Err() error { return w.err }

func (w *Workflow) fail(err error) *Workflow {
	if w.err == nil && err != nil {
		w.err = err
	}
	return w
}

// Step adds a claimable step a worker serves.
func (w *Workflow) Step(id string, handler StepHandler, opts ...StepOption) *Workflow {
	return w.add(StepKindTask, id, handler, opts)
}

// Agent adds a claimable step recorded as an agent turn: your own loop,
// however many turns, as one durable step.
func (w *Workflow) Agent(id string, handler StepHandler, opts ...StepOption) *Workflow {
	return w.add(StepKindAgent, id, handler, opts)
}

func (w *Workflow) add(kind StepKind, id string, handler StepHandler, opts []StepOption) *Workflow {
	if err := checkID(id); err != nil {
		return w.fail(err)
	}
	w.nodes = append(w.nodes, claimable(kind, id, configure(opts)))
	w.handlers[id] = handler
	return w
}

// Sleep adds a delay. No process is alive during it.
func (w *Workflow) Sleep(id string, d time.Duration, opts ...StepOption) *Workflow {
	if err := checkID(id); err != nil {
		return w.fail(err)
	}
	w.nodes = append(w.nodes, common(node{"kind": "sleep", "id": id, "ms": ms(d)}, configure(opts)))
	return w
}

// SleepUntil adds a delay until an instant rendered from a template such as
// ${input.billing_date}.
func (w *Workflow) SleepUntil(id string, at string, opts ...StepOption) *Workflow {
	if err := checkID(id); err != nil {
		return w.fail(err)
	}
	w.nodes = append(w.nodes, common(node{"kind": "sleep_until", "id": id, "at": at}, configure(opts)))
	return w
}

// WaitFor adds a pause until an event arrives. The daemon holds the deadline
// and the mailbox; key is rendered per run, so ${input.order_id} addresses
// this run and no other.
func (w *Workflow) WaitFor(id string, event, key string, timeout time.Duration, opts ...StepOption) *Workflow {
	if err := checkID(id); err != nil {
		return w.fail(err)
	}
	cfg := configure(opts)
	n := common(node{"kind": "wait_event", "id": id, "event": event, "key": key, "timeout_ms": ms(timeout)}, cfg)
	if cfg.acceptParked != nil {
		n["accept_parked"] = *cfg.acceptParked
	}
	if cfg.onTimeout != "" {
		n["on_timeout"] = cfg.onTimeout
	}
	w.nodes = append(w.nodes, n)
	return w
}

// Parallel adds named branches, each one claimable step, all on disk before
// any is offered.
func (w *Workflow) Parallel(id string, build func(*Branch), opts ...StepOption) *Workflow {
	if err := checkID(id); err != nil {
		return w.fail(err)
	}
	b := &Branch{}
	build(b)
	if b.err != nil {
		return w.fail(b.err)
	}
	branches := b.nodes
	if branches == nil {
		branches = []node{}
	}
	w.nodes = append(w.nodes, common(node{"kind": "parallel", "id": id, "branches": branches}, configure(opts)))
	for branch, handler := range b.handlers {
		w.handlers[branch] = handler
	}
	return w
}

// ForEach adds one claimable step per element of the array at over. The
// body's input template can read item and index; afterwards
// steps.<id>.outputs is the array of outputs in item order.
func (w *Workflow) ForEach(id string, over string, maxItems int, build func(*Branch), opts ...StepOption) *Workflow {
	if err := checkID(id); err != nil {
		return w.fail(err)
	}
	body, handlers, err := oneBody(id, "foreach", build)
	if err != nil {
		return w.fail(err)
	}
	cfg := configure(opts)
	n := common(node{"kind": "foreach", "id": id, "over": over, "body": body, "max_items": maxItems}, cfg)
	if cfg.concurrency > 0 {
		n["concurrency"] = cfg.concurrency
	}
	w.nodes = append(w.nodes, n)
	w.adopt(id, handlers)
	return w
}

// Loop adds a bounded loop. Until ends it early; iter.index is the iteration
// number.
func (w *Workflow) Loop(id string, maxIterations int, build func(*Branch), opts ...StepOption) *Workflow {
	if err := checkID(id); err != nil {
		return w.fail(err)
	}
	body, handlers, err := oneBody(id, "loop", build)
	if err != nil {
		return w.fail(err)
	}
	cfg := configure(opts)
	n := common(node{"kind": "loop", "id": id, "body": body, "max_iterations": maxIterations}, cfg)
	if cfg.until != nil {
		n["until"] = cfg.until
	}
	w.nodes = append(w.nodes, n)
	w.adopt(id, handlers)
	return w
}

// Subworkflow adds a child run of another definition. Its output becomes
// steps.<id>.output.
func (w *Workflow) Subworkflow(id string, definition string, opts ...StepOption) *Workflow {
	if err := checkID(id); err != nil {
		return w.fail(err)
	}
	cfg := configure(opts)
	n := common(node{"kind": "subworkflow", "id": id, "definition": definition}, cfg)
	if cfg.hasVersion {
		n["version"] = cfg.version
	}
	if cfg.hasInput {
		n["input"] = cfg.input
	}
	w.nodes = append(w.nodes, n)
	return w
}

// Output sets the template rendered when the run reaches the end of the graph.
func (w *Workflow) Output(template any) *Workflow {
	w.output, w.hasOutput = template, true
	return w
}

// Document is the definition document the daemon stores and walks, or the
// first mistake made while building.
func (w *Workflow) Document() (DefinitionDocument, error) {
	if w.err != nil {
		return DefinitionDocument{}, w.err
	}
	steps := w.nodes
	if steps == nil {
		steps = []node{}
	}
	body := map[string]any{"steps": steps}
	if w.inputSchema != nil {
		body["input_schema"] = w.inputSchema
	}
	if w.limits != nil {
		body["limits"] = w.limits.wire()
	}
	if w.hasOutput {
		body["output"] = w.output
	}
	raw, err := json.Marshal(body)
	if err != nil {
		return DefinitionDocument{}, fmt.Errorf("mcpd: workflow %s could not be encoded: %w", w.id, err)
	}
	kind := DefinitionKindWorkflow
	return DefinitionDocument{ID: w.id, Kind: &kind, Version: w.version, Body: raw}, nil
}

// HandlerFor is the handler a lease's step key addresses. order, checks/fraud,
// notify[3] and refine[2]/rewrite each resolve to the lexical id that
// registered the body. Nil when the key is not this workflow's.
func (w *Workflow) HandlerFor(stepKey string) StepHandler {
	head, sub, hasSub := strings.Cut(stepKey, "/")
	root, _, indexed := strings.Cut(head, "[")
	if hasSub {
		return w.handlers[sub]
	}
	if indexed {
		if body, ok := w.bodyOf[root]; ok {
			return w.handlers[body]
		}
		return w.handlers[root]
	}
	return w.handlers[root]
}

func oneBody(id, what string, build func(*Branch)) (node, map[string]StepHandler, error) {
	b := &Branch{}
	build(b)
	if b.err != nil {
		return nil, nil, b.err
	}
	if len(b.nodes) != 1 {
		return nil, nil, fmt.Errorf("mcpd: %s %q needs exactly one body step; use a subworkflow for more", what, id)
	}
	return b.nodes[0], b.handlers, nil
}

func (w *Workflow) adopt(id string, handlers map[string]StepHandler) {
	for body, handler := range handlers {
		w.handlers[body] = handler
		w.bodyOf[id] = body
	}
}
