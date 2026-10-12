package mcpd

import (
	"encoding/json"
	"fmt"
	"math"
	"strings"
)

// The logging signal: what a field map may hold, and how a level relates to an
// OpenTelemetry severity number.
//
// Nothing here panics or returns an error. A field the caller handed us is
// data, and data that cannot be encoded is worth less than the batch it travels
// in.

// maxFieldDepth is how deep a nested field is followed before it is named
// rather than walked. It is also what stops a map that contains itself.
const maxFieldDepth = 12

// DescribeError flattens an error into the shape every mcpd SDK sends: its
// concrete type and its message.
//
// json.Marshal turns most errors into {} on their own, because the usual error
// types have no exported field, so an error logged under a field key arrived at
// the daemon empty and the one thing the reader needed was gone. Go carries no
// stack on an error value, so no stack is reported: the stack of whichever
// goroutine happened to be logging is not where the error came from.
func DescribeError(err error) map[string]string {
	if err == nil {
		return nil
	}
	message := errorMessage(err)
	if message == "" {
		message = fmt.Sprintf("%T", err)
	}
	return map[string]string{
		"type":    strings.TrimPrefix(fmt.Sprintf("%T", err), "*"),
		"message": message,
	}
}

// errorMessage is err.Error(), made safe to call. An error interface that holds
// a nil pointer is not itself nil, so the caller's own nil check passes, and an
// Error method that then reads through the pointer panics inside the agent
// (B1). A method that panics for any other reason is treated the same way. The
// message is then empty, and the callers name the type instead.
func errorMessage(err error) (message string) {
	defer func() {
		if recover() != nil {
			message = ""
		}
	}()
	return err.Error()
}

// EncodeFields returns a caller's field map in a shape json.Marshal is certain
// to accept.
//
// One value nothing can encode costs its own key and nothing else. The map used
// to be marshalled whole, so a single channel, function or cyclic pointer took
// every other field with it.
func EncodeFields(fields map[string]any) map[string]any {
	if len(fields) == 0 {
		return nil
	}
	out := make(map[string]any, len(fields))
	for key, value := range fields {
		out[key] = encodeFieldValue(value, 0)
	}
	return out
}

func encodeFieldValue(value any, depth int) any {
	switch typed := value.(type) {
	case nil:
		return nil
	case error:
		return DescribeError(typed)
	case string, bool,
		int, int8, int16, int32, int64,
		uint, uint8, uint16, uint32, uint64:
		return typed
	case float32:
		return encodeFloat(float64(typed))
	case float64:
		return encodeFloat(typed)
	case json.RawMessage:
		return typed
	case map[string]any:
		if depth >= maxFieldDepth {
			return "[too deep]"
		}
		out := make(map[string]any, len(typed))
		for key, item := range typed {
			out[key] = encodeFieldValue(item, depth+1)
		}
		return out
	case []any:
		if depth >= maxFieldDepth {
			return "[too deep]"
		}
		out := make([]any, 0, len(typed))
		for _, item := range typed {
			out = append(out, encodeFieldValue(item, depth+1))
		}
		return out
	default:
		// Anything else is offered to the encoder as it is, and named rather
		// than dropped when the encoder refuses it. A cycle, a channel, a
		// function and a NaN all land here.
		if _, err := json.Marshal(typed); err != nil {
			return unencodable(typed)
		}
		return typed
	}
}

// encodeFloat keeps a real number and names the three that JSON has no
// literal for. NaN is the quiet one: it is a float64 like any other until the
// encoder refuses the whole batch it is in.
func encodeFloat(value float64) any {
	switch {
	case math.IsNaN(value):
		return "NaN"
	case math.IsInf(value, 1):
		return "+Inf"
	case math.IsInf(value, -1):
		return "-Inf"
	}
	return value
}

// finite reports whether JSON has a literal for value.
func finite(value float64) bool {
	return !math.IsNaN(value) && !math.IsInf(value, 0)
}

// unencodable names a value by its type rather than by its contents. Printing
// the contents is not safe here: fmt follows a pointer that points at itself
// until the stack runs out, and a cyclic structure is one of the things that
// lands in this branch.
func unencodable(value any) string {
	return fmt.Sprintf("[unencodable %T]", value)
}

// LogLevelFromSeverity returns the level an OpenTelemetry severity number lands
// on.
//
// By range, the way the daemon reads an inbound OTLP LogRecord: 1 to 4 is
// trace, 5 to 8 debug, 9 to 12 info, 13 to 16 warn, 17 to 20 error, and 21 and
// above fatal. A number outside the range is clamped rather than refused,
// because a sender that invents a 40 still meant something serious.
func LogLevelFromSeverity(severity int) LogLevel {
	found := AllLogLevels[0]
	for _, candidate := range AllLogLevels {
		if severity >= candidate.Severity() {
			found = candidate
		}
	}
	return found
}

// ParseLogLevel reads a level by name, case-insensitively. The second result is
// false when the name is not one the spec defines.
func ParseLogLevel(name string) (LogLevel, bool) {
	wanted := LogLevel(strings.ToLower(strings.TrimSpace(name)))
	if wanted.Valid() {
		return wanted, true
	}
	return "", false
}

// MeetsLevel reports whether level is at or above floor.
func MeetsLevel(level, floor LogLevel) bool {
	return level.Severity() >= floor.Severity()
}
