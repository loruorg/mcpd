package mcpd

import "sync"

// signal is one record waiting to be exported. Exactly one of the pointers is
// set, and typ says which; a span is held as its live handle rather than as a
// value so that a span which ends before the queue drains is exported once, in
// its finished form, instead of twice.
type signal struct {
	typ      SignalType
	span     *ActiveSpan
	log      *LogRecord
	metric   *MetricPoint
	score    *Score
	feedback *Feedback
}

// queue is the bounded buffer between the calling goroutine and the exporter.
//
// It is a ring that grows to maxQueue and then drops its oldest entry to make
// room. Dropping the oldest rather than refusing the newest is deliberate: when
// mcpd is unreachable the recent past is what you want when it comes back, and
// an unbounded buffer would turn a network outage into an out-of-memory kill.
type queue struct {
	mu      sync.Mutex
	buf     []signal
	head    int
	size    int
	max     int
	dropped uint64
}

const initialQueueCapacity = 64

func newQueue(max int) *queue {
	if max < 1 {
		max = 1
	}
	initial := initialQueueCapacity
	if max < initial {
		initial = max
	}
	return &queue{buf: make([]signal, initial), max: max}
}

// push appends one signal and reports whether an older one had to be dropped to
// make room.
func (q *queue) push(s signal) bool {
	q.mu.Lock()
	defer q.mu.Unlock()

	dropped := false
	if q.size == q.max {
		evicted := q.buf[q.head]
		q.buf[q.head] = signal{}
		q.head = (q.head + 1) % len(q.buf)
		q.size--
		q.dropped++
		dropped = true
		// The dropped entry was this span's only place in the queue. Clearing
		// the flag lets a later End enqueue the finished record.
		evicted.span.release()
	}
	if q.size == len(q.buf) {
		q.grow()
	}
	q.buf[(q.head+q.size)%len(q.buf)] = s
	q.size++
	return dropped
}

// grow doubles the ring, up to the bound. Starting small keeps a client that
// records ten spans a minute from reserving memory for eight thousand.
func (q *queue) grow() {
	next := len(q.buf) * 2
	if next > q.max {
		next = q.max
	}
	if next <= len(q.buf) {
		return
	}
	buf := make([]signal, next)
	for i := 0; i < q.size; i++ {
		buf[i] = q.buf[(q.head+i)%len(q.buf)]
	}
	q.buf = buf
	q.head = 0
}

// drain removes and returns up to n signals, oldest first. A non-positive n
// takes everything.
func (q *queue) drain(n int) []signal {
	q.mu.Lock()
	defer q.mu.Unlock()

	if q.size == 0 {
		return nil
	}
	if n <= 0 || n > q.size {
		n = q.size
	}
	out := make([]signal, n)
	for i := 0; i < n; i++ {
		idx := (q.head + i) % len(q.buf)
		out[i] = q.buf[idx]
		q.buf[idx] = signal{}
	}
	q.head = (q.head + n) % len(q.buf)
	q.size -= n
	return out
}

func (q *queue) len() int {
	q.mu.Lock()
	defer q.mu.Unlock()
	return q.size
}

// droppedCount is cumulative for the life of the client.
func (q *queue) droppedCount() uint64 {
	q.mu.Lock()
	defer q.mu.Unlock()
	return q.dropped
}
