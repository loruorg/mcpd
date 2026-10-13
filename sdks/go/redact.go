package mcpd

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
)

// isJSONObject reports whether encoded JSON is an object. Checking the
// encoding rather than the Go type accepts a struct or a typed map as readily
// as a map[string]any.
func isJSONObject(raw json.RawMessage) bool {
	trimmed := bytes.TrimLeft(raw, " \t\r\n")
	return len(trimmed) > 0 && trimmed[0] == '{'
}

// redactBatch applies the client's redactor to every payload field in a
// drained batch (contract 2.1.1). It works on the batch's copies, so the
// caller's live span keeps what it was given, and it runs before the sink sees
// anything, so no sink, the HTTP transport included, ever receives an
// unredacted field. Payload capture has already run by now: a stripped input
// or output is absent and is never handed to the redactor.
func (c *Client) redactBatch(batch *IngestBatch) {
	if c == nil || c.cfg.redact == nil {
		return
	}
	for i := range batch.Spans {
		s := &batch.Spans[i]
		s.Input = c.redactField("input", s.Input, false)
		s.Output = c.redactField("output", s.Output, false)
		s.Attributes = c.redactField("attributes", s.Attributes, true)
		s.Metadata = c.redactField("metadata", s.Metadata, true)
	}
	for i := range batch.Logs {
		batch.Logs[i].Fields = c.redactField("log fields", batch.Logs[i].Fields, true)
	}
}

// redactField runs the redactor over one encoded field and returns what to
// keep. It fails closed: an error, a panic or a result that cannot be encoded
// drops the field and is counted, because letting the original through is the
// worst thing a broken redactor could do. When object is set the field is one
// the wire defines as a JSON object, so a redactor that returns anything else
// is as unusable as one that failed. A nil result is a deliberate drop and is
// not counted.
func (c *Client) redactField(field string, raw json.RawMessage, object bool) json.RawMessage {
	if len(raw) == 0 {
		return raw
	}
	kept, err := c.callRedact(raw)
	if err == nil && object && kept != nil && !isJSONObject(kept) {
		err = errors.New("the redactor returned something that is not an object")
	}
	if err != nil {
		c.redactionFailures.Add(1)
		c.logf("redact failed on %s, which was dropped: %v", field, err)
		return nil
	}
	return kept
}

func (c *Client) callRedact(raw json.RawMessage) (out json.RawMessage, err error) {
	// The redactor is the user's code running inside the exporter, and B1
	// does not get to make an exception for it.
	defer func() {
		if r := recover(); r != nil {
			out, err = nil, fmt.Errorf("the redactor panicked: %v", r)
		}
	}()
	dec := json.NewDecoder(bytes.NewReader(raw))
	// UseNumber keeps a large integer exact through the round trip.
	dec.UseNumber()
	var value any
	if err := dec.Decode(&value); err != nil {
		return nil, fmt.Errorf("the field could not be decoded: %w", err)
	}
	kept, err := c.cfg.redact(value)
	if err != nil {
		return nil, err
	}
	if kept == nil {
		// Nil is the redactor's way of dropping the field on purpose, which
		// is not a failure.
		return nil, nil
	}
	encoded, err := json.Marshal(kept)
	if err != nil {
		return nil, fmt.Errorf("the redacted value could not be encoded: %w", err)
	}
	return encoded, nil
}
