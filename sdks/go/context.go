package mcpd

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"net/http"
	"strconv"
	"strings"
	"sync/atomic"
	"time"
)

// Identifier lengths, in hex characters. mcpd uses OpenTelemetry's shapes
// exactly: 128 bits for a trace, 64 for a span. Keeping the wire format
// identical is what lets a trace started here correlate with every other tool
// in the stack.
const (
	traceIDLen = 32
	spanIDLen  = 16
)

// TraceparentHeader is the W3C trace-context header this package reads and
// writes.
const TraceparentHeader = "traceparent"

// TraceContext identifies one point in one trace: enough to continue the trace
// in another process, and nothing more.
type TraceContext struct {
	// TraceID is 32 lowercase hex characters.
	TraceID string
	// SpanID is 16 lowercase hex characters.
	SpanID string
	// Sampled carries the decision made at the root of the trace. It is
	// inherited, never re-decided, so a trace is never half-recorded.
	Sampled bool
	// Remote is true when this context was parsed from an inbound header
	// rather than created here.
	Remote bool
}

// Valid reports whether both identifiers are well formed.
func (tc TraceContext) Valid() bool {
	return isTraceID(tc.TraceID) && isSpanID(tc.SpanID)
}

// Traceparent renders the context as a W3C `traceparent` header value, or an
// empty string when the context is not valid enough to be worth propagating.
func (tc TraceContext) Traceparent() string {
	if !tc.Valid() {
		return ""
	}
	flags := "00"
	if tc.Sampled {
		flags = "01"
	}
	return "00-" + tc.TraceID + "-" + tc.SpanID + "-" + flags
}

// ParseTraceparent parses a W3C `traceparent` header value.
//
// It reports false for anything malformed, and specifically for version `ff`,
// which the specification forbids. A bad header is never a reason to fail a
// request: the caller starts a new trace instead.
func ParseTraceparent(value string) (TraceContext, bool) {
	parts := strings.Split(strings.TrimSpace(value), "-")
	if len(parts) < 4 {
		return TraceContext{}, false
	}
	version, traceID, parentID, flags := parts[0], parts[1], parts[2], parts[3]
	// Later versions may add fields; version 00 has exactly four.
	if version == "00" && len(parts) != 4 {
		return TraceContext{}, false
	}
	if len(version) != 2 || version == "ff" || !isHex(version) {
		return TraceContext{}, false
	}
	if !isTraceID(traceID) || !isSpanID(parentID) {
		return TraceContext{}, false
	}
	if len(flags) != 2 || !isHex(flags) {
		return TraceContext{}, false
	}
	bits, err := strconv.ParseUint(flags, 16, 8)
	if err != nil {
		return TraceContext{}, false
	}
	return TraceContext{
		TraceID: traceID,
		SpanID:  parentID,
		Sampled: bits&0x01 != 0,
		Remote:  true,
	}, true
}

type activeSpanKey struct{}
type remoteParentKey struct{}
type runCorrelationKey struct{}
type usageSinkKey struct{}

// contextWithUsageSink makes every span started from ctx with no parent span
// in this process report its usage to sink when it ends. A Worker sets it on
// the context a step body receives.
func contextWithUsageSink(ctx context.Context, sink usageSink) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	if sink == nil {
		return ctx
	}
	return context.WithValue(ctx, usageSinkKey{}, sink)
}

func usageSinkFromContext(ctx context.Context) usageSink {
	if ctx == nil {
		return nil
	}
	sink, _ := ctx.Value(usageSinkKey{}).(usageSink)
	return sink
}

// ContextWithRunCorrelation returns a context whose spans, when they have no
// parent span in this process, carry the run's correlation: session, thread,
// user, request and release. A Worker sets it on the context a step body
// receives, so a span started from that context belongs to the run and not
// only to this process.
func ContextWithRunCorrelation(ctx context.Context, correlation Correlation) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	return context.WithValue(ctx, runCorrelationKey{}, correlation)
}

// RunCorrelationFromContext returns the run correlation set by
// ContextWithRunCorrelation, if any.
func RunCorrelationFromContext(ctx context.Context) (Correlation, bool) {
	if ctx == nil {
		return Correlation{}, false
	}
	cor, ok := ctx.Value(runCorrelationKey{}).(Correlation)
	return cor, ok
}

// ContextWithSpan returns a context carrying span as the active span. Start
// does this for you; use it directly only when you move a span across a
// goroutine boundary yourself.
func ContextWithSpan(ctx context.Context, span *ActiveSpan) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	if span == nil {
		return ctx
	}
	return context.WithValue(ctx, activeSpanKey{}, span)
}

// SpanFromContext returns the active span, or nil when there is none. The
// returned span is safe to call methods on either way.
func SpanFromContext(ctx context.Context) *ActiveSpan {
	if ctx == nil {
		return nil
	}
	span, _ := ctx.Value(activeSpanKey{}).(*ActiveSpan)
	return span
}

// ContextWithRemoteParent returns a context whose next span continues the trace
// described by parent, which usually came from an inbound header.
func ContextWithRemoteParent(ctx context.Context, parent TraceContext) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	if !parent.Valid() {
		return ctx
	}
	parent.Remote = true
	return context.WithValue(ctx, remoteParentKey{}, parent)
}

// RemoteParentFromContext returns the extracted upstream parent, if any.
func RemoteParentFromContext(ctx context.Context) (TraceContext, bool) {
	if ctx == nil {
		return TraceContext{}, false
	}
	parent, ok := ctx.Value(remoteParentKey{}).(TraceContext)
	return parent, ok
}

// TraceContextFromContext returns what should be propagated onwards: the active
// span when there is one, the extracted upstream parent otherwise.
func TraceContextFromContext(ctx context.Context) (TraceContext, bool) {
	if span := SpanFromContext(ctx); span != nil {
		tc := span.TraceContext()
		if tc.Valid() {
			return tc, true
		}
	}
	return RemoteParentFromContext(ctx)
}

// Extract reads a `traceparent` from inbound headers and returns a context that
// continues that trace. Headers without one are returned unchanged.
func Extract(ctx context.Context, header http.Header) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	if header == nil {
		return ctx
	}
	parent, ok := ParseTraceparent(header.Get(TraceparentHeader))
	if !ok {
		return ctx
	}
	return ContextWithRemoteParent(ctx, parent)
}

// Inject writes the active trace context onto outbound headers. This is the
// half of trace context that makes a trace survive a service boundary.
func Inject(ctx context.Context, header http.Header) {
	if header == nil {
		return
	}
	tc, ok := TraceContextFromContext(ctx)
	if !ok {
		return
	}
	if value := tc.Traceparent(); value != "" {
		header.Set(TraceparentHeader, value)
	}
}

// newTraceID and newSpanID never return the all-zero identifier, which
// OpenTelemetry reserves to mean "no trace" and every downstream system
// discards.
func newTraceID() string { return randomHex(traceIDLen / 2) }
func newSpanID() string  { return randomHex(spanIDLen / 2) }

func randomHex(n int) string {
	b := make([]byte, n)
	for attempt := 0; ; attempt++ {
		fillRandom(b)
		if !allZero(b) {
			break
		}
		// Drawing all zeroes is vanishingly unlikely, but an instrumentation
		// library must not be able to spin here.
		if attempt >= 2 {
			b[0] = 1
			break
		}
	}
	return hex.EncodeToString(b)
}

// fallbackState seeds the degraded generator below. It is only ever touched on
// a platform where the kernel's entropy source is unavailable.
var fallbackState uint64

func fillRandom(b []byte) {
	if _, err := rand.Read(b); err == nil {
		return
	}
	// crypto/rand failing means the platform has no entropy source. An id from
	// a weaker generator still correlates correctly; a panic here would take
	// down the process being observed.
	state := atomic.AddUint64(&fallbackState, uint64(time.Now().UnixNano())|1)
	for i := range b {
		state ^= state << 13
		state ^= state >> 7
		state ^= state << 17
		b[i] = byte(state)
	}
}

func allZero(b []byte) bool {
	for _, v := range b {
		if v != 0 {
			return false
		}
	}
	return true
}

// isHex accepts lowercase hexadecimal only: uppercase ids break correlation
// with tools that compare them as strings.
func isHex(s string) bool {
	if s == "" {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if (c < '0' || c > '9') && (c < 'a' || c > 'f') {
			return false
		}
	}
	return true
}

func isID(s string, length int) bool {
	if len(s) != length || !isHex(s) {
		return false
	}
	for i := 0; i < len(s); i++ {
		if s[i] != '0' {
			return true
		}
	}
	return false
}

func isTraceID(s string) bool { return isID(s, traceIDLen) }
func isSpanID(s string) bool  { return isID(s, spanIDLen) }
