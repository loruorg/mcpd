package mcpd

import "testing"

func spanSignal(name string) signal {
	return signal{typ: SignalTypeSpan, span: &ActiveSpan{data: Span{SpanID: name}}}
}

func TestQueueIsBoundedAndDropsTheOldest(t *testing.T) {
	q := newQueue(8)

	for i := 0; i < 8; i++ {
		if dropped := q.push(spanSignal(string(rune('a' + i)))); dropped {
			t.Fatalf("push %d dropped a signal before the queue was full", i)
		}
	}
	if q.len() != 8 {
		t.Fatalf("queue holds %d, want 8", q.len())
	}

	for i := 0; i < 4; i++ {
		if !q.push(spanSignal(string(rune('A' + i)))) {
			t.Fatalf("push %d past the bound did not report a drop", i)
		}
	}
	if q.len() != 8 {
		t.Fatalf("the bound was exceeded: %d", q.len())
	}
	if q.droppedCount() != 4 {
		t.Fatalf("dropped count is %d, want 4", q.droppedCount())
	}

	drained := q.drain(0)
	if len(drained) != 8 {
		t.Fatalf("drained %d, want 8", len(drained))
	}
	// The four oldest went; the four newest are at the end, in order.
	want := []string{"e", "f", "g", "h", "A", "B", "C", "D"}
	for i, s := range drained {
		if s.span.data.SpanID != want[i] {
			t.Errorf("position %d holds %q, want %q", i, s.span.data.SpanID, want[i])
		}
	}
	if q.len() != 0 {
		t.Fatalf("the queue is not empty after a full drain: %d", q.len())
	}
}

func TestQueueGrowsUpToTheBound(t *testing.T) {
	q := newQueue(200)
	if len(q.buf) != initialQueueCapacity {
		t.Fatalf("initial capacity is %d, want %d", len(q.buf), initialQueueCapacity)
	}
	for i := 0; i < 200; i++ {
		q.push(spanSignal("s"))
	}
	if len(q.buf) != 200 {
		t.Fatalf("capacity grew to %d, want 200", len(q.buf))
	}
	if q.droppedCount() != 0 {
		t.Fatalf("dropped %d signals while still under the bound", q.droppedCount())
	}
}

func TestDroppedSpanCanBeQueuedAgain(t *testing.T) {
	q := newQueue(1)

	first := &ActiveSpan{data: Span{SpanID: "first"}}
	first.pending.Store(true)
	q.push(signal{typ: SignalTypeSpan, span: first})
	q.push(spanSignal("second"))

	// Evicting the entry released the span, so ending it later can enqueue the
	// finished record rather than being swallowed.
	if first.pending.Load() {
		t.Fatal("an evicted span was left marked as queued")
	}
}

func TestDrainTakesAtMostN(t *testing.T) {
	q := newQueue(10)
	for i := 0; i < 6; i++ {
		q.push(spanSignal("s"))
	}
	if got := len(q.drain(4)); got != 4 {
		t.Fatalf("drain(4) returned %d", got)
	}
	if q.len() != 2 {
		t.Fatalf("%d signals left, want 2", q.len())
	}
	if got := len(q.drain(10)); got != 2 {
		t.Fatalf("drain(10) returned %d, want the 2 that were left", got)
	}
	if q.drain(1) != nil {
		t.Fatal("draining an empty queue returned something")
	}
}
