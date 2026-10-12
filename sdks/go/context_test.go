package mcpd

import (
	"context"
	"net/http"
	"testing"
)

func TestTraceparentRoundTrips(t *testing.T) {
	original := TraceContext{
		TraceID: "4bf92f3577b34da6a3ce929d0e0e4736",
		SpanID:  "00f067aa0ba902b7",
		Sampled: true,
	}
	header := original.Traceparent()
	if header != "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01" {
		t.Fatalf("rendered %q", header)
	}

	parsed, ok := ParseTraceparent(header)
	if !ok {
		t.Fatal("a header this package produced was not parseable")
	}
	if parsed.TraceID != original.TraceID || parsed.SpanID != original.SpanID {
		t.Errorf("ids did not survive: %+v", parsed)
	}
	if !parsed.Sampled {
		t.Error("the sampled flag did not survive")
	}
	if !parsed.Remote {
		t.Error("a parsed context should be marked remote")
	}
}

func TestTraceparentSampledFlagIsCarried(t *testing.T) {
	parsed, ok := ParseTraceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00")
	if !ok {
		t.Fatal("a valid unsampled header was rejected")
	}
	if parsed.Sampled {
		t.Error("flags 00 means not sampled")
	}
}

func TestParseTraceparentRejectsMalformedHeaders(t *testing.T) {
	bad := []string{
		"",
		"nonsense",
		"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
		"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
		"ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
		"00-00000000000000000000000000000000-00f067aa0ba902b7-01",
		"00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
		"00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
		"00-4bf92f3577b34da6-00f067aa0ba902b7-01",
		"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-zz",
	}
	for _, value := range bad {
		if _, ok := ParseTraceparent(value); ok {
			t.Errorf("accepted a malformed traceparent: %q", value)
		}
	}
}

func TestGeneratedIDsAreOpenTelemetryShaped(t *testing.T) {
	seen := map[string]bool{}
	for i := 0; i < 500; i++ {
		traceID, spanID := newTraceID(), newSpanID()
		if !isTraceID(traceID) {
			t.Fatalf("bad trace id %q", traceID)
		}
		if !isSpanID(spanID) {
			t.Fatalf("bad span id %q", spanID)
		}
		if seen[traceID] {
			t.Fatalf("trace ids collided at %q", traceID)
		}
		seen[traceID] = true
	}
}

func TestInjectAndExtractCarryTheTraceAcrossAServiceBoundary(t *testing.T) {
	client := New(WithEnabled(false), WithLogger(nil))
	defer client.Shutdown(context.Background())

	ctx, span := client.Start(context.Background(), "caller")
	defer span.End()

	outbound := http.Header{}
	Inject(ctx, outbound)
	if outbound.Get(TraceparentHeader) == "" {
		t.Fatal("Inject wrote nothing")
	}

	// The far side of the boundary.
	downstream := Extract(context.Background(), outbound)
	parent, ok := RemoteParentFromContext(downstream)
	if !ok {
		t.Fatal("Extract found no parent in a header Inject had just written")
	}
	if parent.TraceID != span.TraceID() || parent.SpanID != span.SpanID() {
		t.Errorf("the extracted parent is %+v, want the caller's span", parent)
	}
}

func TestInjectDoesNothingWithoutATrace(t *testing.T) {
	header := http.Header{}
	Inject(context.Background(), header)
	if len(header) != 0 {
		t.Fatalf("Inject wrote %v with no trace in the context", header)
	}
}

func TestTraceHashMatchesTheDaemon(t *testing.T) {
	// The daemon reads the trailing 16 hex characters as a u64. Agreeing on
	// this is what makes a fleet keep or drop the same traces.
	if got := traceHash("4bf92f3577b34da6a3ce929d0e0e4736"); got != 0xa3ce929d0e0e4736 {
		t.Fatalf("traceHash returned %#x", got)
	}
}
