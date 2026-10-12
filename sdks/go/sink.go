package mcpd

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"sync"
)

// Sink is where the exporter hands a drained batch. The HTTP transport is the
// default and the only sink that talks to a daemon; RecordingSink and
// ConsoleSink are the other two every mcpd SDK ships.
//
// A sink runs on the exporter's goroutine inside the same guard as the
// transport: one that returns an error or panics is counted as a failed
// export and logged, never surfaced to the code that recorded the signals.
// The batch it sees is what the daemon would have received, after sampling,
// payload capture and validation, with the batch's idempotency key alongside.
type Sink interface {
	Export(ctx context.Context, batch IngestBatch, ingestKey string) error
}

// httpSink is the default: the client's own transport, with its retries, its
// gzip threshold, its bearer token and the ingest key it was handed.
type httpSink struct{ client *Client }

func (s httpSink) Export(ctx context.Context, batch IngestBatch, ingestKey string) error {
	return s.client.send(ctx, batch, ingestKey)
}

// RecordingSink keeps every batch in memory, so a test can assert what the
// instrumentation recorded without standing up a server.
//
//	sink := &mcpd.RecordingSink{}
//	m := mcpd.New(mcpd.WithSink(sink))
//	...
//	m.Flush(ctx)
//	spans := sink.Spans()
//
// It is safe for concurrent use. The accessors return copies.
type RecordingSink struct {
	mu       sync.Mutex
	spans    []Span
	logs     []LogRecord
	metrics  []MetricPoint
	scores   []Score
	feedback []Feedback
	batches  int
}

// Export keeps the batch. It never fails.
func (s *RecordingSink) Export(_ context.Context, batch IngestBatch, _ string) error {
	if s == nil {
		return nil
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.batches++
	s.spans = append(s.spans, batch.Spans...)
	s.logs = append(s.logs, batch.Logs...)
	s.metrics = append(s.metrics, batch.Metrics...)
	s.scores = append(s.scores, batch.Scores...)
	s.feedback = append(s.feedback, batch.Feedback...)
	return nil
}

// Spans returns every span seen so far, in arrival order.
func (s *RecordingSink) Spans() []Span {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]Span(nil), s.spans...)
}

// Logs returns every log record seen so far, in arrival order.
func (s *RecordingSink) Logs() []LogRecord {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]LogRecord(nil), s.logs...)
}

// Metrics returns every metric point seen so far, in arrival order.
func (s *RecordingSink) Metrics() []MetricPoint {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]MetricPoint(nil), s.metrics...)
}

// Scores returns every score seen so far, in arrival order.
func (s *RecordingSink) Scores() []Score {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]Score(nil), s.scores...)
}

// Feedback returns every piece of feedback seen so far, in arrival order.
func (s *RecordingSink) Feedback() []Feedback {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]Feedback(nil), s.feedback...)
}

// Batches is how many batches the exporter handed over.
func (s *RecordingSink) Batches() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.batches
}

// Clear forgets everything seen so far.
func (s *RecordingSink) Clear() {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.spans, s.logs, s.metrics, s.scores, s.feedback, s.batches = nil, nil, nil, nil, nil, 0
}

// ConsoleSink writes one JSON line per signal, shaped
// {"signal":"span",...record} with the wire's field names, for a local dump
// while debugging. Writer defaults to standard error.
type ConsoleSink struct {
	Writer io.Writer
	mu     sync.Mutex
}

// Export writes the batch, one line per signal.
func (s *ConsoleSink) Export(_ context.Context, batch IngestBatch, _ string) error {
	if s == nil {
		return nil
	}
	w := s.Writer
	if w == nil {
		w = os.Stderr
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, span := range batch.Spans {
		if err := writeSignal(w, SignalTypeSpan, span); err != nil {
			return err
		}
	}
	for _, record := range batch.Logs {
		if err := writeSignal(w, SignalTypeLog, record); err != nil {
			return err
		}
	}
	for _, point := range batch.Metrics {
		if err := writeSignal(w, SignalTypeMetric, point); err != nil {
			return err
		}
	}
	for _, score := range batch.Scores {
		if err := writeSignal(w, SignalTypeScore, score); err != nil {
			return err
		}
	}
	for _, feedback := range batch.Feedback {
		if err := writeSignal(w, SignalTypeFeedback, feedback); err != nil {
			return err
		}
	}
	return nil
}

func writeSignal(w io.Writer, typ SignalType, record any) error {
	encoded, err := json.Marshal(record)
	if err != nil {
		return fmt.Errorf("mcpd: %s could not be encoded: %w", typ, err)
	}
	line := []byte(`{"signal":"` + string(typ) + `"`)
	if len(encoded) > 2 && encoded[0] == '{' {
		line = append(line, ',')
		line = append(line, encoded[1:]...)
	} else {
		line = append(line, '}')
	}
	line = append(line, '\n')
	_, err = w.Write(line)
	return err
}
