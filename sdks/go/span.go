package mcpd

import (
	"context"
	"encoding/json"
	"fmt"
	"runtime"
	"sync"
	"sync/atomic"
	"time"
)

// ActiveSpan is the live handle to one unit of work. The record that reaches
// the server is a Span; this is the mutable thing your code holds while the
// operation is still running.
//
// Every method is safe to call on a nil receiver, from any goroutine, at any
// point in the span's life. Instrumentation that panics is worse than no
// instrumentation at all.
type ActiveSpan struct {
	client *Client

	mu   sync.Mutex
	data Span
	// ended is set by the first End and makes every later mutation a no-op, so
	// a span cannot be changed after the record it produced has left.
	ended bool

	// recording is fixed at construction: false when the client is disabled or
	// the trace was not sampled. Read without the lock because it never changes.
	recording bool

	// pending is true while an entry in the export queue points at this span.
	// The exporter clears it before it snapshots, so a span that ends while its
	// entry is still queued is exported once, finished, rather than twice.
	pending atomic.Bool

	// capture is whether input and output may reach the record. Fixed at
	// construction from the client, or from the span's own option.
	capture bool

	// usageSink is where this span's usage goes when it ends: the step context
	// that opened the span, or the one an ancestor was opened from. Nil for a
	// span outside a durable step. Never on the wire.
	usageSink     usageSink
	usageReported bool
}

// usageReport is what a span hands upward when it ends carrying usage.
type usageReport struct {
	usage    TokenUsage
	model    string
	provider string
}

// usageSink receives a span's usage once, at its end.
type usageSink func(usageReport)

// TraceID returns the 32-character trace identifier this span belongs to.
func (s *ActiveSpan) TraceID() string {
	if s == nil {
		return ""
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.data.TraceID
}

// SpanID returns this span's 16-character identifier.
func (s *ActiveSpan) SpanID() string {
	if s == nil {
		return ""
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.data.SpanID
}

// TraceContext returns what a downstream service needs to continue this trace.
func (s *ActiveSpan) TraceContext() TraceContext {
	if s == nil {
		return TraceContext{}
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return TraceContext{
		TraceID: s.data.TraceID,
		SpanID:  s.data.SpanID,
		Sampled: s.recording,
	}
}

// Traceparent renders this span as a W3C `traceparent` header value.
func (s *ActiveSpan) Traceparent() string { return s.TraceContext().Traceparent() }

// IsRecording reports whether anything set on this span will be exported. Use
// it to skip expensive work, such as serialising a large prompt, when the trace
// was not sampled.
func (s *ActiveSpan) IsRecording() bool {
	if s == nil {
		return false
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.recording && !s.ended
}

// mutate runs f against the record under the lock, unless the span is inert or
// already ended.
func (s *ActiveSpan) mutate(f func(*Span)) {
	if s == nil || !s.recording {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.ended {
		return
	}
	f(&s.data)
}

// CapturesPayloads reports whether this span's input and output may leave the
// process. False when the client was built with WithCapturePayloads(false) or
// the span started with WithSpanCapturePayloads(false).
func (s *ActiveSpan) CapturesPayloads() bool { return s != nil && s.capture }

// SetInput records what went in. For a model call this is the prompt, and it is
// the single most valuable field mcpd stores. With payload capture off the
// value never reaches the record.
func (s *ActiveSpan) SetInput(value any) {
	if s == nil || !s.recording || !s.capture {
		return
	}
	if raw, ok := s.client.encode("input", value); ok {
		s.mutate(func(d *Span) { d.Input = raw })
	}
}

// SetOutput records what came out. With payload capture off the value never
// reaches the record.
func (s *ActiveSpan) SetOutput(value any) {
	if s == nil || !s.recording || !s.capture {
		return
	}
	if raw, ok := s.client.encode("output", value); ok {
		s.mutate(func(d *Span) { d.Output = raw })
	}
}

// SetUsage records token counts, including any cache and reasoning detail the
// provider reported. mcpd prices the span from this at ingest.
func (s *ActiveSpan) SetUsage(usage TokenUsage) {
	u := usage
	s.mutate(func(d *Span) { d.Usage = &u })
}

// SetTokens is the common case of SetUsage: a plain input and output count.
func (s *ActiveSpan) SetTokens(input, output uint64) {
	in, out := input, output
	total := input + output
	s.mutate(func(d *Span) {
		d.Usage = &TokenUsage{InputTokens: &in, OutputTokens: &out, TotalTokens: &total}
	})
}

// SetModelParameters records how the model was called: temperature, nucleus
// and top-k sampling, the output ceiling, the seed, the stop sequences, and
// whether the output was constrained to a fixed shape.
//
// These are settings, not payload. Nothing here carries a prompt or a
// completion, so they are recorded whether or not payload capture is on. They
// are what a run has to be reproduced from, and dropping them with the prompt
// would leave a run nobody can repeat.
//
// A nil pointer field means the caller set nothing; a pointer to zero is a real
// value and reaches the wire as one, which matters most for Seed. One call
// describes one model call, so a second call replaces the first: settings half
// from one request and half from another would describe a call nobody made.
func (s *ActiveSpan) SetModelParameters(params ModelParameters) {
	p := params
	if p.StopSequences != nil {
		p.StopSequences = append([]string(nil), params.StopSequences...)
	}
	// A setting JSON cannot carry is left out, as though it had not been
	// given, rather than failing the encode of the whole batch (contract
	// section 4.1).
	if p.Temperature != nil && !finite(*p.Temperature) {
		p.Temperature = nil
	}
	if p.TopP != nil && !finite(*p.TopP) {
		p.TopP = nil
	}
	s.mutate(func(d *Span) { d.ModelParameters = &p })
}

// SetModel records the model and its provider. Both are indexed, so a
// regression can be attributed to a model change without a full-text search.
func (s *ActiveSpan) SetModel(model, provider string) {
	s.mutate(func(d *Span) {
		if model != "" {
			m := model
			d.Model = &m
		}
		if provider != "" {
			p := provider
			d.Provider = &p
		}
	})
}

// SetMetadata attaches free-form context. mcpd never interprets it.
func (s *ActiveSpan) SetMetadata(metadata map[string]any) {
	if s == nil || !s.recording || len(metadata) == 0 {
		return
	}
	if raw, ok := s.client.encode("metadata", metadata); ok {
		s.mutate(func(d *Span) { d.Metadata = raw })
	}
}

// SetAttributes attaches kind-specific structured detail, such as a tool's
// arguments or a retrieval's parameters.
func (s *ActiveSpan) SetAttributes(attributes map[string]any) {
	if s == nil || !s.recording || len(attributes) == 0 {
		return
	}
	if raw, ok := s.client.encode("attributes", attributes); ok {
		s.mutate(func(d *Span) { d.Attributes = raw })
	}
}

// SetTags labels the trace. Tags are only meaningful on a root span, where they
// label the whole run.
func (s *ActiveSpan) SetTags(tags ...string) {
	if len(tags) == 0 {
		return
	}
	copied := append([]string(nil), tags...)
	s.mutate(func(d *Span) { d.Tags = copied })
}

// SetStatus sets the outcome explicitly. End sets it for you in the usual case.
func (s *ActiveSpan) SetStatus(status SpanStatus) {
	if status == "" {
		return
	}
	s.mutate(func(d *Span) { d.Status = status })
}

// SetSession, SetThread and SetUser fill in correlation after the fact, for the
// common case where the identity is only known once the work has begun.
func (s *ActiveSpan) SetSession(sessionID string) {
	s.mutate(func(d *Span) { d.Correlation.SessionID = strPtr(sessionID) })
}

// SetThread records the conversation thread this span belongs to.
func (s *ActiveSpan) SetThread(threadID string) {
	s.mutate(func(d *Span) { d.Correlation.ThreadID = strPtr(threadID) })
}

// SetUser records the end user. mcpd hashes it by default, because it is
// usually personal data.
func (s *ActiveSpan) SetUser(userID string) {
	s.mutate(func(d *Span) { d.Correlation.UserID = strPtr(userID) })
}

// ErrorOption adds detail to a recorded error.
type ErrorOption func(*SpanError)

// ErrorKind overrides the error class, which otherwise comes from the error's
// concrete Go type.
func ErrorKind(kind string) ErrorOption {
	return func(e *SpanError) {
		if kind != "" {
			e.Kind = &kind
		}
	}
}

// ErrorStack attaches a stack trace. Pass Stack() to capture the caller's.
func ErrorStack(stack string) ErrorOption {
	return func(e *SpanError) {
		if stack != "" {
			e.Stack = &stack
		}
	}
}

// ErrorDomain names the subsystem that raised the error.
func ErrorDomain(domain string) ErrorOption {
	return func(e *SpanError) {
		if domain != "" {
			e.Domain = &domain
		}
	}
}

// ErrorRetryable records whether the failure was worth retrying.
func ErrorRetryable(retryable bool) ErrorOption {
	return func(e *SpanError) { e.Retryable = &retryable }
}

// ErrorDetails attaches structured detail, such as a provider's error body.
func ErrorDetails(value any) ErrorOption {
	return func(e *SpanError) {
		if raw, err := json.Marshal(value); err == nil {
			e.Details = raw
		}
	}
}

// Stack captures the calling goroutine's stack, for ErrorStack.
func Stack() string {
	buf := make([]byte, 8192)
	return string(buf[:runtime.Stack(buf, false)])
}

// RecordError marks the span failed and records why. The message comes from the
// error itself, so a failure can never arrive without one: "something failed"
// is not actionable. A nil error is ignored.
func (s *ActiveSpan) RecordError(err error, opts ...ErrorOption) {
	if s == nil || err == nil || !s.recording {
		return
	}
	goType := fmt.Sprintf("%T", err)
	detail := SpanError{Message: errorMessage(err)}
	if detail.Message == "" {
		// A failure with no message is not actionable. Naming the type is thin,
		// but it is better than recording an empty string.
		detail.Message = goType
	}
	// The concrete type of a value from errors.New or fmt.Errorf is an
	// implementation detail, not an error class worth grouping by.
	switch goType {
	case "*errors.errorString", "*fmt.wrapError", "*errors.joinError":
	default:
		detail.Kind = &goType
	}
	for _, opt := range opts {
		if opt != nil {
			opt(&detail)
		}
	}
	s.mutate(func(d *Span) {
		d.Error = &detail
		d.Status = SpanStatusError
	})
}

// Event records a point in time under this span: a streamed chunk, a retry, a
// guardrail firing. An event has no duration and never needs ending.
func (s *ActiveSpan) Event(name string, opts ...SpanOption) {
	if s == nil || !s.recording || s.client == nil || name == "" {
		return
	}
	s.client.startSpan(context.Background(), name, spanRequest{
		parent:    s.TraceContext(),
		hasParent: true,
		parentOf:  s,
		event:     true,
		opts:      opts,
	})
}

// Child starts a span under this one, whatever the active span in ctx happens
// to be. The returned context carries the child.
func (s *ActiveSpan) Child(ctx context.Context, name string, opts ...SpanOption) (context.Context, *ActiveSpan) {
	if ctx == nil {
		ctx = context.Background()
	}
	if s == nil || s.client == nil {
		return ctx, inertSpan(name)
	}
	child := s.client.startSpan(ctx, name, spanRequest{
		parent:    s.TraceContext(),
		hasParent: true,
		parentOf:  s,
		opts:      opts,
	})
	return ContextWithSpan(ctx, child), child
}

// End closes the span with status ok, or error when an error was recorded. It
// is idempotent: the second call does nothing, so `defer span.End()` alongside
// an explicit end is safe.
func (s *ActiveSpan) End() { s.end("") }

// EndWithStatus closes the span with an explicit outcome, such as
// SpanStatusCancelled.
func (s *ActiveSpan) EndWithStatus(status SpanStatus) { s.end(status) }

func (s *ActiveSpan) end(status SpanStatus) {
	if s == nil {
		return
	}
	s.mu.Lock()
	if s.ended {
		s.mu.Unlock()
		return
	}
	s.ended = true

	now := time.Now().UTC()
	s.data.EndTime = &now
	elapsed := now.Sub(s.data.StartTime)
	if elapsed < 0 {
		elapsed = 0
	}
	ns := uint64(elapsed.Nanoseconds())
	s.data.DurationNs = &ns

	switch {
	case status != "" && status != SpanStatusUnset:
		s.data.Status = status
	case s.data.Error != nil:
		s.data.Status = SpanStatusError
	case s.data.Status == SpanStatusUnset || s.data.Status == "":
		s.data.Status = SpanStatusOk
	}
	recording := s.recording
	s.mu.Unlock()

	s.reportUsage()
	if recording {
		s.enqueue()
	}
}

// reportUsage hands the span's usage to the step that opened it, once. It
// runs whether or not the span is recording: a step's cost is what its body
// spent, and sampling the trace out does not make the tokens free.
func (s *ActiveSpan) reportUsage() {
	if s == nil || s.usageSink == nil {
		return
	}
	s.mu.Lock()
	if s.usageReported || s.data.Usage == nil {
		s.mu.Unlock()
		return
	}
	s.usageReported = true
	report := usageReport{usage: *s.data.Usage}
	if s.data.Model != nil {
		report.model = *s.data.Model
	}
	if s.data.Provider != nil {
		report.provider = *s.data.Provider
	}
	sink := s.usageSink
	s.mu.Unlock()
	sink(report)
}

// enqueue puts this span on the export queue unless an entry for it is already
// waiting there. Skipping the second push is what keeps a short span to one
// record while still exporting a long one the moment it starts.
func (s *ActiveSpan) enqueue() {
	if s == nil || s.client == nil {
		return
	}
	if !s.pending.CompareAndSwap(false, true) {
		return
	}
	s.client.push(signal{typ: SignalTypeSpan, span: s})
}

// release marks this span as no longer queued. The exporter calls it before
// taking its snapshot, so an End that lands in between still enqueues the
// finished record rather than being swallowed.
func (s *ActiveSpan) release() {
	if s == nil {
		return
	}
	s.pending.Store(false)
}

// snapshot copies the record as it stands. The copy shares the immutable byte
// slices and strings inside it; nothing here rewrites them in place.
func (s *ActiveSpan) snapshot() Span {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.data
}

func (s *ActiveSpan) correlation() Correlation {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.data.Correlation
}

// inertSpan returns a span that records nothing but answers every method, for
// the case where there is no client at all.
func inertSpan(name string) *ActiveSpan {
	return &ActiveSpan{data: Span{
		TraceID:   newTraceID(),
		SpanID:    newSpanID(),
		Name:      name,
		Kind:      SpanKindGeneric,
		Status:    SpanStatusUnset,
		StartTime: time.Now().UTC(),
	}}
}

// SpanOption configures a span at the moment it starts.
type SpanOption func(*spanConfig)

type spanConfig struct {
	kind       SpanKind
	entityKind EntityKind
	entityID   string
	entityName string
	input      any
	hasInput   bool
	output     any
	hasOutput  bool
	metadata   map[string]any
	attributes map[string]any
	tags       []string
	model      string
	provider   string
	usage      *TokenUsage
	links      []SpanLink
	startTime  time.Time

	session     string
	thread      string
	user        string
	runID       string
	requestID   string
	resourceID  string
	release     string
	environment string

	parent    *TraceContext
	hasParent bool

	externalParent string
	capture        bool
	hasCapture     bool

	// Set only by the experiment runner, which is why they have no exported
	// option.
	experimentID string
	keep         bool
}

// WithSpanKind sets what the operation is. The kind drives how the span is
// rendered and which metrics are extracted from it.
func WithSpanKind(kind SpanKind) SpanOption {
	return func(c *spanConfig) {
		if kind != "" {
			c.kind = kind
		}
	}
}

// WithInput sets the span's input at construction.
func WithInput(value any) SpanOption {
	return func(c *spanConfig) { c.input, c.hasInput = value, true }
}

// WithOutput sets the span's output at construction.
func WithOutput(value any) SpanOption {
	return func(c *spanConfig) { c.output, c.hasOutput = value, true }
}

// WithMetadata attaches free-form context.
func WithMetadata(metadata map[string]any) SpanOption {
	return func(c *spanConfig) { c.metadata = metadata }
}

// WithAttributes attaches kind-specific structured detail.
func WithAttributes(attributes map[string]any) SpanOption {
	return func(c *spanConfig) { c.attributes = attributes }
}

// WithTags labels the trace. Only meaningful on a root span.
func WithTags(tags ...string) SpanOption {
	return func(c *spanConfig) { c.tags = append([]string(nil), tags...) }
}

// WithModel records the model and provider this span used.
func WithModel(model, provider string) SpanOption {
	return func(c *spanConfig) { c.model, c.provider = model, provider }
}

// WithUsage records token counts at construction.
func WithUsage(usage TokenUsage) SpanOption {
	return func(c *spanConfig) { u := usage; c.usage = &u }
}

// WithEntity names the thing that produced the span: an agent, a tool, a
// scorer. Entity names are low-cardinality and safe to group by.
func WithEntity(kind EntityKind, id, name string) SpanOption {
	return func(c *spanConfig) { c.entityKind, c.entityID, c.entityName = kind, id, name }
}

// WithSession groups many traces into one conversation.
func WithSession(sessionID string) SpanOption {
	return func(c *spanConfig) { c.session = sessionID }
}

// WithThread names a thread inside a session.
func WithThread(threadID string) SpanOption {
	return func(c *spanConfig) { c.thread = threadID }
}

// WithUser records the end user.
func WithUser(userID string) SpanOption {
	return func(c *spanConfig) { c.user = userID }
}

// WithRunID records the producer's own identifier for this run.
func WithRunID(runID string) SpanOption {
	return func(c *spanConfig) { c.runID = runID }
}

// WithRequestID records the inbound request that caused this work.
func WithRequestID(requestID string) SpanOption {
	return func(c *spanConfig) { c.requestID = requestID }
}

// WithResourceID records the tenant or workspace inside the calling
// application.
func WithResourceID(resourceID string) SpanOption {
	return func(c *spanConfig) { c.resourceID = resourceID }
}

// WithRelease records the release or git sha, for regression attribution.
func WithRelease(release string) SpanOption {
	return func(c *spanConfig) { c.release = release }
}

// WithSpanEnvironment overrides the client's environment for one span.
func WithSpanEnvironment(environment string) SpanOption {
	return func(c *spanConfig) { c.environment = environment }
}

// WithLinks records causality that parentage cannot express, such as the twenty
// requests that caused one batch.
func WithLinks(links ...SpanLink) SpanOption {
	return func(c *spanConfig) { c.links = append([]SpanLink(nil), links...) }
}

// WithStartTime backdates the span, for work that was measured before it could
// be recorded.
func WithStartTime(start time.Time) SpanOption {
	return func(c *spanConfig) { c.startTime = start }
}

// WithParent continues an explicit trace, ignoring whatever is in the context.
func WithParent(parent TraceContext) SpanOption {
	return func(c *spanConfig) {
		if parent.Valid() {
			p := parent
			c.parent, c.hasParent = &p, true
		}
	}
}

// WithExternalParent records a parent from a tracing system that is not mcpd,
// such as the OpenTelemetry span an application was already inside. It is
// carried for correlation and never used for parenting inside mcpd's own
// tree.
func WithExternalParent(spanID string) SpanOption {
	return func(c *spanConfig) { c.externalParent = spanID }
}

// WithSpanCapturePayloads overrides the client's WithCapturePayloads for this
// span only, in either direction: keep one prompt in the process on a client
// that captures, or let one span through on a client that does not.
func WithSpanCapturePayloads(capture bool) SpanOption {
	return func(c *spanConfig) { c.capture, c.hasCapture = capture, true }
}
