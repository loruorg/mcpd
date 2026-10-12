// Package mcpd is the Go SDK for mcpd, an agent-observability daemon.
//
// Recording is non-blocking: a span is appended to a bounded in-memory queue
// and a background worker exports it in batches. Nothing in this package
// returns an error from an instrumentation call and nothing panics, because an
// observability library must never take down the thing it observes. The only
// methods that return an error are the explicit ones, Flush, Shutdown and the
// read API, where the caller has asked to wait for an answer.
//
//	m := mcpd.New()
//	defer m.Shutdown(context.Background())
//
//	ctx, span := m.Start(ctx, "support_agent")
//	defer span.End()
package mcpd

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// Defaults, all overridable in code and by environment variable.
const (
	DefaultEndpoint      = "http://localhost:4317"
	DefaultProject       = "default"
	DefaultBatchSize     = 128
	DefaultFlushInterval = 2 * time.Second
	DefaultMaxQueue      = 8192
	DefaultTimeout       = 10 * time.Second
)

// receivedAt is the arrival time, which only the server can know. Sending the
// epoch tells mcpd to stamp its own; sending anything else would let a client
// with a wrong clock rewrite retention.
var receivedAt = time.Unix(0, 0).UTC()

// Logger is the small slice of *log.Logger this package needs. Pass nil to
// silence the SDK entirely.
type Logger interface {
	Printf(format string, args ...any)
}

type config struct {
	endpoint       string
	apiKey         string
	project        string
	environment    string
	serviceName    string
	serviceVersion string
	sample         float64
	batchSize      int
	flushInterval  time.Duration
	maxQueue       int
	timeout        time.Duration
	enabled        bool
	// capturePayloads decides whether input and output may leave the process.
	capturePayloads bool
	// logLevel is the lowest level Log records. A line below it never
	// becomes a record.
	logLevel   LogLevel
	httpClient *http.Client
	logger     Logger
	sink       Sink
}

// Option configures a Client. Code wins over the environment.
type Option func(*config)

// WithEndpoint sets the mcpd base URL, such as http://localhost:4317.
func WithEndpoint(endpoint string) Option {
	return func(c *config) {
		if endpoint != "" {
			c.endpoint = endpoint
		}
	}
}

// WithAPIKey sets the bearer token sent with every request.
func WithAPIKey(key string) Option { return func(c *config) { c.apiKey = key } }

// WithProject sets the tenancy key every signal is recorded under.
func WithProject(project string) Option {
	return func(c *config) {
		if project != "" {
			c.project = project
		}
	}
}

// WithEnvironment sets the deployment environment, such as production.
func WithEnvironment(environment string) Option {
	return func(c *config) { c.environment = environment }
}

// WithService names the emitting service and its version, so a regression can
// be pinned to a release.
func WithService(name, version string) Option {
	return func(c *config) { c.serviceName, c.serviceVersion = name, version }
}

// WithSampleRate keeps the given fraction of traces, 0 to 1. The decision is
// made once at the root of each trace, from the trace id, so every process in a
// fleet keeps or drops the same traces without coordinating.
func WithSampleRate(rate float64) Option {
	return func(c *config) { c.sample = rate }
}

// WithBatchSize sets how many signals go in one request.
func WithBatchSize(size int) Option {
	return func(c *config) {
		if size > 0 {
			c.batchSize = size
		}
	}
}

// WithFlushInterval sets how often the worker exports what it has.
func WithFlushInterval(interval time.Duration) Option {
	return func(c *config) {
		if interval > 0 {
			c.flushInterval = interval
		}
	}
}

// WithMaxQueue bounds the in-memory buffer. When it is full the oldest signals
// are dropped and a warning is logged at most once per flush interval.
func WithMaxQueue(max int) Option {
	return func(c *config) {
		if max > 0 {
			c.maxQueue = max
		}
	}
}

// WithTimeout bounds a single HTTP attempt.
func WithTimeout(timeout time.Duration) Option {
	return func(c *config) {
		if timeout > 0 {
			c.timeout = timeout
		}
	}
}

// WithEnabled turns recording off without changing any call site: every method
// still returns a valid object, and nothing is queued or sent.
func WithEnabled(enabled bool) Option { return func(c *config) { c.enabled = enabled } }

// WithHTTPClient supplies your own transport, for a proxy, a custom TLS
// configuration or a test double.
func WithHTTPClient(client *http.Client) Option {
	return func(c *config) {
		if client != nil {
			c.httpClient = client
		}
	}
}

// WithLogger redirects the SDK's own diagnostics. Pass nil to silence them.
func WithLogger(logger Logger) Option { return func(c *config) { c.logger = logger } }

// WithCapturePayloads decides whether a span's input and output may leave the
// process. With false, spans are still recorded with their timing, status,
// usage, model, correlation and errors, but the payloads are dropped before
// they reach the queue, so nothing sensitive is ever queued or exported. A
// span can override it for itself with WithSpanCapturePayloads. The
// environment variable is MCPD_CAPTURE_PAYLOADS; the default is true.
func WithCapturePayloads(capture bool) Option {
	return func(c *config) { c.capturePayloads = capture }
}

// WithLogLevel sets the lowest level Log records. A line below the floor is
// dropped at the call, before the record is built, so a debug line in a
// production process costs one comparison. The environment variable is
// MCPD_LOG_LEVEL; the default is LogLevelDebug, which keeps every level the
// spec defines except trace. A level the spec does not define is ignored.
func WithLogLevel(level LogLevel) Option {
	return func(c *config) {
		if level.Valid() {
			c.logLevel = level
		}
	}
}

// WithSink replaces where drained batches go. The default is the HTTP
// transport; a RecordingSink keeps batches in memory for a test and a
// ConsoleSink prints them. A sink is chosen in code only.
func WithSink(sink Sink) Option {
	return func(c *config) {
		if sink != nil {
			c.sink = sink
		}
	}
}

// Client records signals and queries them back. It is safe for concurrent use
// and cheap to keep for the life of the process.
type Client struct {
	cfg     config
	http    *http.Client
	q       *queue
	enabled bool

	// sampleThreshold is the trace-id hash below which a trace is kept.
	sampleThreshold uint64

	wake     chan struct{}
	flushReq chan *flushRequest
	stop     chan struct{}
	done     chan struct{}
	stopOnce sync.Once
	closed   atomic.Bool
	// abandoned is set when a shutdown's deadline passed: the worker starts
	// no further batch, and what is still queued is given up.
	abandoned atomic.Bool

	sink Sink

	exported atomic.Uint64
	failed   atomic.Uint64
	rejected atomic.Uint64

	logMu   sync.Mutex
	lastLog time.Time
}

type flushRequest struct {
	ctx  context.Context
	err  error
	done chan struct{}
}

// New returns a client. Called with no options at all it talks to a local mcpd
// on its default port, which is the whole point: nothing to configure before
// the first trace.
//
// It never fails. A misconfigured endpoint disables recording and says so once
// in the log rather than returning an error the caller has to handle at a place
// where there is nothing useful to do about it.
func New(opts ...Option) *Client {
	cfg := configFromEnv()
	for _, opt := range opts {
		if opt != nil {
			opt(&cfg)
		}
	}

	cfg.endpoint = strings.TrimRight(strings.TrimSpace(cfg.endpoint), "/")
	if cfg.endpoint == "" {
		cfg.endpoint = DefaultEndpoint
	}
	if cfg.project == "" {
		cfg.project = DefaultProject
	}
	if cfg.batchSize < 1 {
		cfg.batchSize = DefaultBatchSize
	}
	if cfg.maxQueue < cfg.batchSize {
		cfg.maxQueue = cfg.batchSize
	}
	if cfg.flushInterval <= 0 {
		cfg.flushInterval = DefaultFlushInterval
	}
	if cfg.timeout <= 0 {
		cfg.timeout = DefaultTimeout
	}
	if cfg.sample < 0 {
		cfg.sample = 0
	}
	if cfg.sample > 1 {
		cfg.sample = 1
	}
	if !cfg.logLevel.Valid() {
		cfg.logLevel = LogLevelDebug
	}

	c := &Client{
		cfg:      cfg,
		http:     cfg.httpClient,
		q:        newQueue(cfg.maxQueue),
		enabled:  cfg.enabled,
		wake:     make(chan struct{}, 1),
		flushReq: make(chan *flushRequest),
		stop:     make(chan struct{}),
		done:     make(chan struct{}),
	}
	if c.http == nil {
		// No Timeout on the client itself: every attempt carries a context
		// deadline of its own, and a client-wide one would cut a long poll off
		// at the ordinary timeout however long the daemon was asked to hold it.
		c.http = &http.Client{}
	}
	c.sink = cfg.sink
	if c.sink == nil {
		c.sink = httpSink{client: c}
	}
	// Scaled to the top 53 bits, the range a float64 represents exactly, so the
	// comparison cannot overflow at a rate near 1.
	c.sampleThreshold = uint64(cfg.sample * float64(uint64(1)<<53))

	if parsed, err := url.Parse(c.cfg.endpoint); err != nil || parsed.Host == "" ||
		(parsed.Scheme != "http" && parsed.Scheme != "https") {
		c.logf("endpoint %q is not an http or https URL; recording is off", c.cfg.endpoint)
		c.enabled = false
	}

	if c.enabled {
		go c.worker()
	} else {
		// Nothing will ever be exported, so Flush and Shutdown must not wait
		// for a worker that was never started.
		close(c.done)
	}
	return c
}

func configFromEnv() config {
	cfg := config{
		endpoint:      envString("MCPD_ENDPOINT", DefaultEndpoint),
		apiKey:        envString("MCPD_API_KEY", ""),
		project:       envString("MCPD_PROJECT", DefaultProject),
		environment:   envString("MCPD_ENVIRONMENT", ""),
		sample:        envFloat("MCPD_SAMPLE", 1.0),
		batchSize:     envInt("MCPD_BATCH_SIZE", DefaultBatchSize),
		flushInterval: envDurationMS("MCPD_FLUSH_INTERVAL_MS", DefaultFlushInterval),
		maxQueue:      envInt("MCPD_MAX_QUEUE", DefaultMaxQueue),
		timeout:       envDurationMS("MCPD_TIMEOUT_MS", DefaultTimeout),
		enabled:       envBool("MCPD_ENABLED", true),
		// Prompts and completions leave the process unless someone says not to.
		capturePayloads: envBool("MCPD_CAPTURE_PAYLOADS", true),
		logLevel:        envLogLevel("MCPD_LOG_LEVEL", LogLevelDebug),
		logger:          log.New(os.Stderr, "", log.LstdFlags),
	}
	cfg.serviceName = envString("MCPD_SERVICE_NAME", "")
	cfg.serviceVersion = envString("MCPD_SERVICE_VERSION", "")
	return cfg
}

func envString(key, fallback string) string {
	if v, ok := os.LookupEnv(key); ok && strings.TrimSpace(v) != "" {
		return strings.TrimSpace(v)
	}
	return fallback
}

func envFloat(key string, fallback float64) float64 {
	if v, ok := os.LookupEnv(key); ok {
		if parsed, err := strconv.ParseFloat(strings.TrimSpace(v), 64); err == nil {
			return parsed
		}
	}
	return fallback
}

func envInt(key string, fallback int) int {
	if v, ok := os.LookupEnv(key); ok {
		if parsed, err := strconv.Atoi(strings.TrimSpace(v)); err == nil {
			return parsed
		}
	}
	return fallback
}

// envDurationMS reads a duration given in whole milliseconds. The unit is in
// the variable's name on purpose: every mcpd SDK reads the same name and means
// the same thing by it, so a value copied between languages keeps working.
func envDurationMS(key string, fallback time.Duration) time.Duration {
	raw := os.Getenv(key)
	if raw == "" {
		return fallback
	}
	ms, err := strconv.Atoi(raw)
	if err != nil || ms <= 0 {
		return fallback
	}
	return time.Duration(ms) * time.Millisecond
}

func envDuration(key string, fallback time.Duration) time.Duration {
	if v, ok := os.LookupEnv(key); ok {
		if parsed, err := time.ParseDuration(strings.TrimSpace(v)); err == nil {
			return parsed
		}
	}
	return fallback
}

// envLogLevel reads a level by name. A name the spec does not define is
// ignored rather than refused: a typo in a deployment variable must not silence
// a service's logs.
func envLogLevel(key string, fallback LogLevel) LogLevel {
	if level, ok := ParseLogLevel(os.Getenv(key)); ok {
		return level
	}
	return fallback
}

func envBool(key string, fallback bool) bool {
	v, ok := os.LookupEnv(key)
	if !ok {
		return fallback
	}
	switch strings.ToLower(strings.TrimSpace(v)) {
	case "yes", "on":
		return true
	case "no", "off":
		return false
	}
	if parsed, err := strconv.ParseBool(strings.TrimSpace(v)); err == nil {
		return parsed
	}
	return fallback
}

// Project reports which project this client records under.
func (c *Client) Project() string {
	if c == nil {
		return ""
	}
	return c.cfg.project
}

// Enabled reports whether recording is on.
func (c *Client) Enabled() bool { return c != nil && c.enabled }

// LogLevel reports the lowest level this client records. Anything below it
// never reaches the queue.
func (c *Client) LogLevel() LogLevel {
	if c == nil {
		return LogLevelDebug
	}
	return c.cfg.logLevel
}

// Endpoint reports the mcpd base URL in use.
func (c *Client) Endpoint() string {
	if c == nil {
		return ""
	}
	return c.cfg.endpoint
}

// Stats is a snapshot of the client's own counters. mcpd's SDKs report their
// own health for the same reason the daemon does: a collector you cannot
// observe is a liability.
type Stats struct {
	// Queued is how many signals are waiting to be exported.
	Queued int
	// Dropped is how many were discarded because the queue was full.
	Dropped uint64
	// Capacity is how many signals the queue can hold.
	Capacity int
	// Exported is how many signals the sink accepted.
	Exported uint64
	// Failed is how many batches the sink refused, after the retry budget ran
	// out when the sink is the HTTP transport.
	Failed uint64
	// Rejected is how many spans Record refused as malformed.
	Rejected uint64
}

// Stats returns the counters as they stand.
func (c *Client) Stats() Stats {
	if c == nil {
		return Stats{}
	}
	return Stats{
		Queued:   c.q.len(),
		Dropped:  c.q.droppedCount(),
		Capacity: c.cfg.maxQueue,
		Exported: c.exported.Load(),
		Failed:   c.failed.Load(),
		Rejected: c.rejected.Load(),
	}
}

// CapturesPayloads reports whether input and output leave the process by
// default. A span may say otherwise for itself.
func (c *Client) CapturesPayloads() bool { return c != nil && c.cfg.capturePayloads }

// ---------------------------------------------------------------- tracing ---

// spanRequest is everything startSpan needs that is not an option.
type spanRequest struct {
	parent    TraceContext
	hasParent bool
	parentOf  *ActiveSpan
	forceRoot bool
	event     bool
	opts      []SpanOption
}

// Start begins a span and returns a context carrying it. A span in ctx becomes
// the parent; with none, this is the root of a new trace and the sampling
// decision is made here, once, for the whole trace.
//
//	ctx, span := m.Start(ctx, "support_agent")
//	defer span.End()
func (c *Client) Start(ctx context.Context, name string, opts ...SpanOption) (context.Context, *ActiveSpan) {
	if ctx == nil {
		ctx = context.Background()
	}
	if c == nil {
		return ctx, inertSpan(name)
	}
	span := c.startSpan(ctx, name, spanRequest{opts: opts})
	return ContextWithSpan(ctx, span), span
}

// Trace begins a root span, ignoring any span already in ctx. Use it when one
// process runs many independent agent runs and you do not want them nested.
func (c *Client) Trace(ctx context.Context, name string, opts ...SpanOption) (context.Context, *ActiveSpan) {
	if ctx == nil {
		ctx = context.Background()
	}
	if c == nil {
		return ctx, inertSpan(name)
	}
	span := c.startSpan(ctx, name, spanRequest{forceRoot: true, opts: opts})
	return ContextWithSpan(ctx, span), span
}

func (c *Client) startSpan(ctx context.Context, name string, req spanRequest) *ActiveSpan {
	cfg := spanConfig{}
	for _, opt := range req.opts {
		if opt != nil {
			opt(&cfg)
		}
	}

	parent, hasParent := req.parent, req.hasParent
	parentSpan := req.parentOf
	switch {
	case cfg.hasParent && cfg.parent != nil:
		// An explicit parent beats anything in the context.
		parent, hasParent, parentSpan = *cfg.parent, true, nil
	case hasParent, req.forceRoot:
		// Already decided by the caller.
	default:
		if active := SpanFromContext(ctx); active != nil {
			parent, hasParent, parentSpan = active.TraceContext(), true, active
		} else if remote, ok := RemoteParentFromContext(ctx); ok {
			parent, hasParent = remote, true
		}
	}

	kind := cfg.kind
	if kind == "" {
		// A root is an agent run until told otherwise. A step is generic,
		// because guessing what a child is would be worse than saying nothing.
		kind = SpanKindAgentRun
		if hasParent {
			kind = SpanKindGeneric
		}
	}

	traceID := parent.TraceID
	sampled := parent.Sampled
	if !hasParent {
		traceID = newTraceID()
		sampled = c.sampledTrace(traceID)
	}

	start := cfg.startTime
	if start.IsZero() {
		start = time.Now()
	}

	correlation := c.baseCorrelation()
	if parentSpan != nil {
		// Inherit session, user and thread from the parent: they belong to the
		// run, not to one step of it.
		correlation = parentSpan.correlation()
		correlation.Project = c.cfg.project
	} else if run, ok := RunCorrelationFromContext(ctx); ok {
		// A durable run's step body: the run's own session, user and the rest
		// reach the span, the way a parent span's would.
		inheritCorrelation(&correlation, run)
	}
	applyCorrelation(&correlation, &cfg)

	capture := c.cfg.capturePayloads
	if cfg.hasCapture {
		capture = cfg.capture
	}
	span := &ActiveSpan{
		client:    c,
		recording: c.enabled && sampled,
		capture:   capture,
		data: Span{
			TraceID:     traceID,
			SpanID:      newSpanID(),
			Name:        name,
			Kind:        kind,
			Status:      SpanStatusUnset,
			StartTime:   start.UTC(),
			IsRoot:      !hasParent,
			IsEvent:     req.event,
			Correlation: correlation,
			ReceivedAt:  receivedAt,
		},
	}
	if hasParent {
		parentID := parent.SpanID
		span.data.ParentSpanID = &parentID
	}
	// A step's usage is the sum of what its spans spent. The sink comes from
	// the parent span when there is one, so every span nested under the step
	// reports upward, and from the context otherwise, which is how the first
	// span a body starts finds it.
	if parentSpan != nil {
		span.usageSink = parentSpan.usageSink
	}
	if span.usageSink == nil {
		span.usageSink = usageSinkFromContext(ctx)
	}
	c.applySpanConfig(span, &cfg)

	if req.event {
		// An event is a point in time: it is complete the moment it is made,
		// and End on it is a no-op.
		span.data.Status = SpanStatusOk
		span.ended = true
		span.reportUsage()
	}
	if span.recording {
		// Exported on start as well as on end, so a run that crashes mid-flight
		// is still visible: that is exactly the run you need to see.
		span.enqueue()
	}
	return span
}

// Record queues a span that already ended, ids, parent and times included:
// what an adapter for another tracing system, a batch import or a replay
// holds. The record is queued as given, never re-parented, re-timed or
// re-sampled. Correlation fields it leaves empty are filled from the client's
// own; fields it sets are kept.
//
// A record whose ids are not OpenTelemetry-shaped, whose name is empty or
// whose start time is missing is refused: dropped, counted in Stats().Rejected
// and logged. Refusing never panics and never returns an error, the same rule
// as every other recording call. WithCapturePayloads(false) strips a recorded
// span's input and output like any other span's.
func (c *Client) Record(span Span) {
	if c == nil || !c.enabled {
		return
	}
	if reason := invalidRecord(&span); reason != "" {
		c.rejected.Add(1)
		c.logf("a recorded span was refused: %s", reason)
		return
	}
	base := c.baseCorrelation()
	if span.Correlation.Project == "" {
		span.Correlation.Project = base.Project
	}
	if span.Correlation.Environment == nil {
		span.Correlation.Environment = base.Environment
	}
	if span.Correlation.ServiceName == nil {
		span.Correlation.ServiceName = base.ServiceName
	}
	if span.Correlation.ServiceVersion == nil {
		span.Correlation.ServiceVersion = base.ServiceVersion
	}
	if !c.cfg.capturePayloads {
		span.Input, span.Output = nil, nil
	}
	if span.ReceivedAt.IsZero() {
		span.ReceivedAt = receivedAt
	}
	// Held as an ended ActiveSpan so the queue treats it like any other span:
	// one slot, released when it is snapshotted.
	holder := &ActiveSpan{client: c, recording: true, capture: c.cfg.capturePayloads, data: span, ended: true}
	holder.enqueue()
}

// invalidRecord names what is wrong with a span handed to Record, or nothing.
func invalidRecord(span *Span) string {
	switch {
	case !isTraceID(span.TraceID):
		return fmt.Sprintf("trace_id %q is not 32 lowercase hex characters, or is all zero", span.TraceID)
	case !isSpanID(span.SpanID):
		return fmt.Sprintf("span_id %q is not 16 lowercase hex characters, or is all zero", span.SpanID)
	case span.ParentSpanID != nil && !isSpanID(*span.ParentSpanID):
		return fmt.Sprintf("parent_span_id %q is not 16 lowercase hex characters, or is all zero", *span.ParentSpanID)
	case span.Name == "":
		return "name is empty"
	case span.StartTime.IsZero():
		return "start_time is missing"
	}
	return ""
}

func (c *Client) applySpanConfig(span *ActiveSpan, cfg *spanConfig) {
	d := &span.data
	if cfg.entityKind != "" {
		k := cfg.entityKind
		d.EntityKind = &k
	}
	d.EntityID = strPtr(cfg.entityID)
	d.EntityName = strPtr(cfg.entityName)
	d.Model = strPtr(cfg.model)
	d.Provider = strPtr(cfg.provider)
	if len(cfg.tags) > 0 {
		d.Tags = cfg.tags
	}
	if len(cfg.links) > 0 {
		d.Links = cfg.links
	}
	if cfg.usage != nil {
		d.Usage = cfg.usage
	}
	d.ExternalParentSpanID = strPtr(cfg.externalParent)
	if !span.recording {
		// Encoding payloads for a span nobody will read is wasted work.
		return
	}
	// With capture off the payloads never reach the record, so nothing
	// sensitive can be queued and nothing sensitive can leave on a flush.
	if cfg.hasInput && span.capture {
		if raw, ok := c.encode("input", cfg.input); ok {
			d.Input = raw
		}
	}
	if cfg.hasOutput && span.capture {
		if raw, ok := c.encode("output", cfg.output); ok {
			d.Output = raw
		}
	}
	if len(cfg.metadata) > 0 {
		if raw, ok := c.encode("metadata", cfg.metadata); ok {
			d.Metadata = raw
		}
	}
	if len(cfg.attributes) > 0 {
		if raw, ok := c.encode("attributes", cfg.attributes); ok {
			d.Attributes = raw
		}
	}
}

func (c *Client) baseCorrelation() Correlation {
	return Correlation{
		Project:        c.cfg.project,
		Environment:    strPtr(c.cfg.environment),
		ServiceName:    strPtr(c.cfg.serviceName),
		ServiceVersion: strPtr(c.cfg.serviceVersion),
	}
}

func inheritCorrelation(cor *Correlation, from Correlation) {
	if from.SessionID != nil {
		cor.SessionID = from.SessionID
	}
	if from.ThreadID != nil {
		cor.ThreadID = from.ThreadID
	}
	if from.UserID != nil {
		cor.UserID = from.UserID
	}
	if from.RequestID != nil {
		cor.RequestID = from.RequestID
	}
	if from.Release != nil {
		cor.Release = from.Release
	}
}

func applyCorrelation(cor *Correlation, cfg *spanConfig) {
	if cfg.environment != "" {
		cor.Environment = strPtr(cfg.environment)
	}
	if cfg.session != "" {
		cor.SessionID = strPtr(cfg.session)
	}
	if cfg.thread != "" {
		cor.ThreadID = strPtr(cfg.thread)
	}
	if cfg.user != "" {
		cor.UserID = strPtr(cfg.user)
	}
	if cfg.runID != "" {
		cor.RunID = strPtr(cfg.runID)
	}
	if cfg.requestID != "" {
		cor.RequestID = strPtr(cfg.requestID)
	}
	if cfg.resourceID != "" {
		cor.ResourceID = strPtr(cfg.resourceID)
	}
	if cfg.release != "" {
		cor.Release = strPtr(cfg.release)
	}
}

// sampledTrace decides once, at the root, by hashing the trace id. Hashing
// rather than rolling a die is what makes two processes in the same fleet agree
// about the same trace without talking to each other.
func (c *Client) sampledTrace(traceID string) bool {
	if c.cfg.sample >= 1 {
		return true
	}
	if c.cfg.sample <= 0 {
		return false
	}
	return traceHash(traceID)>>11 < c.sampleThreshold
}

// traceHash maps a trace id onto the full uint64 range, using the trailing
// bytes: some producers derive the leading bytes from a timestamp, which would
// make a leading-byte sampler correlate with the time of day. It matches the
// daemon's own sampler exactly.
func traceHash(traceID string) uint64 {
	if n := len(traceID); n >= 16 {
		if v, err := strconv.ParseUint(traceID[n-16:], 16, 64); err == nil {
			return v
		}
	}
	hash := uint64(0xcbf29ce484222325)
	for i := 0; i < len(traceID); i++ {
		hash ^= uint64(traceID[i])
		hash *= 0x100000001b3
	}
	return hash
}

// ---------------------------------------------------------- other signals ---

// Log records a structured log line, correlated to the active span in ctx.
func (c *Client) Log(ctx context.Context, level LogLevel, message string, fields map[string]any) {
	if c == nil || !c.enabled || message == "" {
		return
	}
	if level == "" {
		level = LogLevelInfo
	}
	// Below the floor costs one comparison: no record, no queue, no
	// correlation lookup.
	if !MeetsLevel(level, c.cfg.logLevel) {
		return
	}
	record := LogRecord{
		LogID:       newTraceID(),
		Timestamp:   time.Now().UTC(),
		Level:       level,
		Message:     message,
		Correlation: c.correlationFor(ctx),
		ReceivedAt:  receivedAt,
	}
	if span := SpanFromContext(ctx); span != nil {
		if !span.recording {
			// The trace was not sampled, so there is nothing for this line to
			// hang off.
			return
		}
		traceID, spanID := span.TraceID(), span.SpanID()
		record.TraceID, record.SpanID = &traceID, &spanID
	}
	if len(fields) > 0 {
		// EncodeFields first: an error under a key marshals to {} on its own,
		// and one value the encoder refuses used to drop every other field
		// with it.
		if raw, ok := c.encode("log fields", EncodeFields(fields)); ok {
			record.Fields = raw
		}
	}
	c.push(signal{typ: SignalTypeLog, log: &record})
}

// MetricOption adds detail to one observation.
type MetricOption func(*MetricPoint)

// MetricUnit sets the UCUM unit, such as ms, By or {token}.
func MetricUnit(unit string) MetricOption {
	return func(m *MetricPoint) {
		if unit != "" {
			m.Unit = &unit
		}
	}
}

// MetricLabels sets the dimensions to split the series on. Keep them
// low-cardinality: mcpd's cardinality filter rejects unbounded keys.
func MetricLabels(labels map[string]any) MetricOption {
	return func(m *MetricPoint) {
		if len(labels) == 0 {
			return
		}
		if raw, err := json.Marshal(EncodeFields(labels)); err == nil {
			m.Labels = raw
		}
	}
}

// MetricTime backdates the observation.
func MetricTime(at time.Time) MetricOption {
	return func(m *MetricPoint) {
		if !at.IsZero() {
			m.Timestamp = at.UTC()
		}
	}
}

// Metric records one numeric observation, linked to the active span so a spike
// on a chart opens the run that caused it.
func (c *Client) Metric(ctx context.Context, name string, value float64, kind MetricKind, opts ...MetricOption) {
	if c == nil || !c.enabled || name == "" {
		return
	}
	// JSON has no literal for NaN or an infinity, so encoding the batch would
	// fail and every signal in it would be lost. The point is what cannot be
	// sent, so it alone is dropped (contract section 4.1).
	if !finite(value) {
		c.logf("metric %q has the value %v, which JSON cannot carry; the point was dropped", name, value)
		return
	}
	if kind == "" {
		kind = MetricKindGauge
	}
	point := MetricPoint{
		MetricID:    newTraceID(),
		Name:        name,
		Kind:        kind,
		Value:       value,
		Timestamp:   time.Now().UTC(),
		Correlation: c.correlationFor(ctx),
		ReceivedAt:  receivedAt,
	}
	if span := SpanFromContext(ctx); span != nil && span.recording {
		traceID, spanID := span.TraceID(), span.SpanID()
		point.TraceID, point.SpanID = &traceID, &spanID
	}
	for _, opt := range opts {
		if opt != nil {
			opt(&point)
		}
	}
	c.push(signal{typ: SignalTypeMetric, metric: &point})
}

// Target is what a score or a piece of feedback is about.
type Target struct {
	TraceID string
	SpanID  string
}

// TraceTarget judges a whole run.
func TraceTarget(traceID string) Target { return Target{TraceID: traceID} }

// SpanTarget judges one step of a run.
func SpanTarget(traceID, spanID string) Target { return Target{TraceID: traceID, SpanID: spanID} }

// ActiveTarget judges whatever is running in ctx. It is the zero Target's
// meaning too, so most calls can pass Target{}.
func ActiveTarget(ctx context.Context) Target {
	if span := SpanFromContext(ctx); span != nil {
		return Target{TraceID: span.TraceID(), SpanID: span.SpanID()}
	}
	if remote, ok := RemoteParentFromContext(ctx); ok {
		return Target{TraceID: remote.TraceID, SpanID: remote.SpanID}
	}
	return Target{}
}

func (t Target) or(ctx context.Context) Target {
	if t.TraceID != "" {
		return t
	}
	return ActiveTarget(ctx)
}

// ScoreOption adds detail to a score.
type ScoreOption func(*Score)

// ScoreReason records why the scorer gave this value. For an LLM judge it is
// the judge's explanation, and it is what makes a score arguable rather than
// magic.
func ScoreReason(reason string) ScoreOption {
	return func(s *Score) {
		if reason != "" {
			s.Reason = &reason
		}
	}
}

// ScoreLabel records a categorical result alongside the number.
func ScoreLabel(label string) ScoreOption {
	return func(s *Score) {
		if label != "" {
			s.Label = &label
		}
	}
}

// ScoreVersion records the scorer's version. A score is only comparable to
// another from the same version.
func ScoreVersion(version string) ScoreOption {
	return func(s *Score) {
		if version != "" {
			s.ScorerVersion = &version
		}
	}
}

// ScoreFrom records where the score came from. The default is live.
func ScoreFrom(source ScoreSource) ScoreOption {
	return func(s *Score) {
		if source != "" {
			s.Source = source
		}
	}
}

// ScoreMetadata attaches anything else about the score.
func ScoreMetadata(metadata map[string]any) ScoreOption {
	return func(s *Score) {
		if len(metadata) == 0 {
			return
		}
		if raw, err := json.Marshal(metadata); err == nil {
			s.Metadata = raw
		}
	}
}

// Score attaches a machine evaluation to a run or a step.
//
// min and max are required at the call site on purpose: a bare number with no
// declared scale cannot be compared to anything, and 0.7 means nothing until
// you know whether the scale runs to 1 or to 10.
func (c *Client) Score(ctx context.Context, target Target, scorerID string, value, min, max float64, opts ...ScoreOption) {
	if c == nil || !c.enabled || scorerID == "" {
		return
	}
	if !finite(value) || !finite(min) || !finite(max) {
		c.logf("score %q has value %v on [%v, %v], which JSON cannot carry; the score was dropped", scorerID, value, min, max)
		return
	}
	target = target.or(ctx)
	if target.TraceID == "" {
		c.logf("score %q was dropped: no trace to attach it to", scorerID)
		return
	}
	if max <= min {
		c.logf("score %q declares an empty scale (min %v, max %v) and will not be comparable", scorerID, min, max)
	}
	lo, hi := min, max
	record := Score{
		ScoreID:     newTraceID(),
		TraceID:     &target.TraceID,
		ScorerID:    scorerID,
		Source:      ScoreSourceLive,
		Value:       value,
		Min:         &lo,
		Max:         &hi,
		Timestamp:   time.Now().UTC(),
		Correlation: c.correlationFor(ctx),
	}
	if target.SpanID != "" {
		spanID := target.SpanID
		record.SpanID = &spanID
	}
	for _, opt := range opts {
		if opt != nil {
			opt(&record)
		}
	}
	c.push(signal{typ: SignalTypeScore, score: &record})
}

// FeedbackOption adds detail to a piece of human feedback.
type FeedbackOption func(*Feedback)

// FeedbackComment records the prose a person added.
func FeedbackComment(comment string) FeedbackOption {
	return func(f *Feedback) {
		if comment != "" {
			f.Comment = &comment
		}
	}
}

// FeedbackAuthor records who gave the judgement and in what capacity: an end
// user, a reviewer, an operator.
func FeedbackAuthor(id, kind string) FeedbackOption {
	return func(f *Feedback) {
		if id != "" {
			f.AuthorID = &id
		}
		if kind != "" {
			f.AuthorKind = &kind
		}
	}
}

// FeedbackMetadata attaches anything else.
func FeedbackMetadata(metadata map[string]any) FeedbackOption {
	return func(f *Feedback) {
		if len(metadata) == 0 {
			return
		}
		if raw, err := json.Marshal(metadata); err == nil {
			f.Metadata = raw
		}
	}
}

// Feedback attaches a person's judgement to a run or a step. Wire it to your
// product's thumbs-up control and the judgement lands on the exact trace that
// produced the answer.
//
// The shape of value follows kind: "up" or "down" for thumbs, a number for a
// rating, text for a correction.
func (c *Client) Feedback(ctx context.Context, target Target, kind FeedbackKind, value any, opts ...FeedbackOption) {
	if c == nil || !c.enabled || kind == "" {
		return
	}
	target = target.or(ctx)
	if target.TraceID == "" {
		c.logf("%s feedback was dropped: no trace to attach it to", kind)
		return
	}
	raw, ok := c.encode("feedback value", value)
	if !ok {
		return
	}
	record := Feedback{
		FeedbackID:  newTraceID(),
		TraceID:     &target.TraceID,
		Kind:        kind,
		Value:       raw,
		Timestamp:   time.Now().UTC(),
		Correlation: c.correlationFor(ctx),
	}
	if target.SpanID != "" {
		spanID := target.SpanID
		record.SpanID = &spanID
	}
	for _, opt := range opts {
		if opt != nil {
			opt(&record)
		}
	}
	c.push(signal{typ: SignalTypeFeedback, feedback: &record})
}

func (c *Client) correlationFor(ctx context.Context) Correlation {
	if span := SpanFromContext(ctx); span != nil {
		cor := span.correlation()
		cor.Project = c.cfg.project
		return cor
	}
	return c.baseCorrelation()
}

// ------------------------------------------------------------ the worker ---

func (c *Client) push(s signal) {
	if c.q.push(s) {
		c.logf("queue is full at %d signals; the oldest were dropped (%d so far)",
			c.cfg.maxQueue, c.q.droppedCount())
	}
	if c.q.len() >= c.cfg.batchSize {
		select {
		case c.wake <- struct{}{}:
		default: // a wake-up is already pending, which is all one needs
		}
	}
}

func (c *Client) worker() {
	defer close(c.done)

	ticker := time.NewTicker(c.cfg.flushInterval)
	defer ticker.Stop()

	for {
		select {
		case <-ticker.C:
			c.export(context.Background(), c.cfg.batchSize)
		case <-c.wake:
			c.export(context.Background(), c.cfg.batchSize)
		case req := <-c.flushReq:
			req.err = c.exportAll(req.ctx)
			close(req.done)
		case <-c.stop:
			ctx, cancel := context.WithTimeout(context.Background(), c.cfg.timeout)
			c.exportAll(ctx)
			cancel()
			return
		}
	}
}

// export sends at most limit signals and reports nothing: a failed export is
// logged and counted, never raised.
func (c *Client) export(ctx context.Context, limit int) error {
	// A shutdown that ran out of time gave up what is queued. The worker may
	// still pick a pending wake-up or tick after that, and must not act on it.
	if c.abandoned.Load() {
		return nil
	}
	signals := c.q.drain(limit)
	if len(signals) == 0 {
		return nil
	}
	batch := buildBatch(signals)
	// The idempotency key is minted once per batch and reused across the
	// transport's retries, which is what makes a retry after a timeout safe:
	// mcpd recognises the replay and counts it once.
	if err := c.deliver(ctx, batch, newTraceID()); err != nil {
		c.failed.Add(1)
		c.logf("export of %d signals failed: %v", len(signals), err)
		return err
	}
	c.exported.Add(uint64(len(signals)))
	return nil
}

// deliver hands one batch to the sink inside the exporter's guard: a sink that
// panics is a failed export, not a dead worker.
func (c *Client) deliver(ctx context.Context, batch IngestBatch, ingestKey string) (err error) {
	defer func() {
		if r := recover(); r != nil {
			err = fmt.Errorf("mcpd: the sink panicked: %v", r)
		}
	}()
	if batchSize(&batch) == 0 {
		return nil
	}
	return c.sink.Export(ctx, batch, ingestKey)
}

// exportAll drains the queue in batch-sized pieces. It stops after the number
// of batches the queue could hold, so a producer writing faster than the
// network can never trap the worker here, and it stops when ctx is done, so a
// caller's deadline is a bound on time and not only on batches.
func (c *Client) exportAll(ctx context.Context) error {
	var err error
	rounds := c.cfg.maxQueue/c.cfg.batchSize + 1
	for i := 0; i < rounds; i++ {
		if c.q.len() == 0 {
			break
		}
		// The caller has gone, or a shutdown gave up. What is left stays
		// queued rather than being drained into batches that fail at once.
		if ctx.Err() != nil {
			return ctx.Err()
		}
		if c.abandoned.Load() {
			return nil
		}
		// The batch goes out under a context the caller's deadline cannot
		// cancel: a batch in flight finishes and is counted rather than being
		// cut off halfway, and the transport's own timeout and retries bound
		// it.
		if e := c.export(context.WithoutCancel(ctx), c.cfg.batchSize); e != nil {
			err = e
		}
	}
	return err
}

// DefaultDeadline is how long Flush and Shutdown wait when the context they are
// given has no deadline of its own (contract section 2.4). A daemon that
// accepts the connection and never answers costs every batch three attempts
// of the full request timeout, which for a full queue is more than half an
// hour.
const DefaultDeadline = 10 * time.Second

// flushDeadline is DefaultDeadline, held in a variable so a test can shorten
// it.
var flushDeadline = DefaultDeadline

// withDeadline keeps the caller's deadline, or applies the default when the
// context has none.
func withDeadline(ctx context.Context) (context.Context, context.CancelFunc) {
	if _, ok := ctx.Deadline(); ok {
		return ctx, func() {}
	}
	return context.WithTimeout(ctx, flushDeadline)
}

// Flush sends everything queued and waits for the result, until the context's
// deadline or DefaultDeadline when it has none. It is safe to call at any
// time, and on a client that is disabled or already shut down it returns
// immediately. When the deadline passes it returns the context's error; what
// was not sent stays queued for the worker, and a batch already in flight
// finishes on its own.
func (c *Client) Flush(ctx context.Context) error {
	if c == nil || !c.enabled || c.closed.Load() {
		return nil
	}
	if ctx == nil {
		ctx = context.Background()
	}
	ctx, cancel := withDeadline(ctx)
	defer cancel()
	req := &flushRequest{ctx: ctx, done: make(chan struct{})}
	select {
	case c.flushReq <- req:
	case <-c.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
	select {
	case <-req.done:
		return req.err
	case <-c.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// Shutdown flushes what is queued, stops the worker and returns. It is
// idempotent, and a short-lived process that does not call it loses whatever
// had not been exported yet. One deadline covers the whole call: the
// context's, or DefaultDeadline when it has none. When it passes, the worker
// starts no further batch and what is still queued is given up.
func (c *Client) Shutdown(ctx context.Context) error {
	if c == nil || !c.enabled {
		return nil
	}
	if ctx == nil {
		ctx = context.Background()
	}
	ctx, cancel := withDeadline(ctx)
	defer cancel()
	var err error
	c.stopOnce.Do(func() {
		err = c.Flush(ctx)
		c.closed.Store(true)
		close(c.stop)
	})
	select {
	case <-c.done:
	case <-ctx.Done():
		c.abandoned.Store(true)
		return ctx.Err()
	}
	return err
}

// ----------------------------------------------------------------- detail ---

// encode turns a value into JSON. A value that cannot be encoded is dropped and
// logged, because a serialisation error in a user's payload must not surface as
// a failure in their agent.
func (c *Client) encode(field string, value any) (json.RawMessage, bool) {
	if value == nil {
		return nil, false
	}
	if raw, ok := value.(json.RawMessage); ok {
		return raw, len(raw) > 0
	}
	encoded, err := json.Marshal(value)
	if err != nil {
		if c != nil {
			c.logf("%s could not be encoded and was dropped: %v", field, err)
		}
		return nil, false
	}
	return json.RawMessage(encoded), true
}

// logf writes at most one diagnostic per flush interval. An unreachable server
// produces one line every couple of seconds, not one line per span.
func (c *Client) logf(format string, args ...any) {
	if c == nil || c.cfg.logger == nil {
		return
	}
	c.logMu.Lock()
	now := time.Now()
	if !c.lastLog.IsZero() && now.Sub(c.lastLog) < c.cfg.flushInterval {
		c.logMu.Unlock()
		return
	}
	c.lastLog = now
	logger := c.cfg.logger
	c.logMu.Unlock()

	// The logger is the user's code running inside ours, and B1 does not get
	// to make an exception for it. A Printf that panics would otherwise take
	// the caller's goroutine down from inside an instrumentation call, which
	// is the one thing an observability library must never do.
	defer func() {
		_ = recover()
	}()
	logger.Printf("mcpd: "+format, args...)
}

func strPtr(value string) *string {
	if value == "" {
		return nil
	}
	return &value
}
