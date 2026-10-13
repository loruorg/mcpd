package mcpd

import (
	"bytes"
	"compress/gzip"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/rand"
	"net/http"
	"net/url"
	"sort"
	"strconv"
	"strings"
	"time"
)

const (
	// Version is the SDK version, sent as the user agent so a server operator
	// can tell which client produced a batch.
	Version = "0.1.0"

	userAgent = "mcpd-go/" + Version

	// Bodies above this size are gzipped. Below it the compression costs more
	// than the bytes it saves.
	gzipThreshold = 1024

	maxAttempts      = 3
	baseBackoff      = 250 * time.Millisecond
	maxBackoff       = 5 * time.Second
	maxResponseBytes = 8 << 20
)

// ErrNoClient is returned by the read methods when they are called on a nil
// client.
var ErrNoClient = errors.New("mcpd: client is nil")

// APIError is a non-2xx response from mcpd, with the reason the server gave.
type APIError struct {
	// Status is the HTTP status code.
	Status int
	// Code is mcpd's machine-readable error code, such as not_found.
	Code string
	// Message is the human-readable reason.
	Message string
	// Body is the response body as the server sent it. A 409 from the run
	// surface carries its full reason here; see StepConflictError.
	Body []byte
}

func (e *APIError) Error() string {
	switch {
	case e.Code != "" && e.Message != "":
		return fmt.Sprintf("mcpd: %s (%d): %s", e.Code, e.Status, e.Message)
	case e.Message != "":
		return fmt.Sprintf("mcpd: HTTP %d: %s", e.Status, e.Message)
	default:
		return fmt.Sprintf("mcpd: HTTP %d", e.Status)
	}
}

// buildBatch turns drained signals into one wire batch.
//
// A queued span is released before it is snapshotted, never after: an End that
// lands between the two then enqueues the finished record instead of being
// swallowed by an entry that has already gone.
func buildBatch(signals []signal) IngestBatch {
	batch := IngestBatch{}
	for _, s := range signals {
		switch s.typ {
		case SignalTypeSpan:
			if s.span != nil {
				s.span.release()
				batch.Spans = append(batch.Spans, s.span.snapshot())
			}
		case SignalTypeLog:
			if s.log != nil {
				batch.Logs = append(batch.Logs, *s.log)
			}
		case SignalTypeMetric:
			if s.metric != nil {
				batch.Metrics = append(batch.Metrics, *s.metric)
			}
		case SignalTypeScore:
			if s.score != nil {
				batch.Scores = append(batch.Scores, *s.score)
			}
		case SignalTypeFeedback:
			if s.feedback != nil {
				batch.Feedback = append(batch.Feedback, *s.feedback)
			}
		}
	}
	return batch
}

func batchSize(batch *IngestBatch) int {
	return len(batch.Spans) + len(batch.Logs) + len(batch.Metrics) +
		len(batch.Scores) + len(batch.Feedback)
}

// send posts one batch to the native ingest endpoint under the idempotency
// key the exporter minted for it. The key is reused across retries, which is
// what makes a retry after a timeout safe: mcpd recognises the replay and
// counts it once.
func (c *Client) send(ctx context.Context, batch IngestBatch, ingestKey string) error {
	if batchSize(&batch) == 0 {
		return nil
	}
	if ingestKey == "" {
		ingestKey = newTraceID()
	}
	batch.IngestKey = &ingestKey

	body, err := json.Marshal(batch)
	if err != nil {
		return fmt.Errorf("mcpd: batch could not be encoded: %w", err)
	}
	encoding := ""
	if len(body) > gzipThreshold {
		if compressed, err := gzipBytes(body); err == nil {
			body, encoding = compressed, "gzip"
		}
	}

	raw, err := c.doRequest(ctx, http.MethodPost, "/v1/ingest", nil, body, encoding, 0)
	if err != nil {
		return err
	}

	var result IngestResult
	if err := json.Unmarshal(raw, &result); err == nil && result.Rejected > 0 {
		reason := ""
		if len(result.Errors) > 0 {
			reason = ": " + result.Errors[0].Message
		}
		c.logf("mcpd rejected %d of %d signals%s", result.Rejected, batchSize(&batch), reason)
	}
	return nil
}

func gzipBytes(body []byte) ([]byte, error) {
	var buf bytes.Buffer
	w := gzip.NewWriter(&buf)
	if _, err := w.Write(body); err != nil {
		w.Close()
		return nil, err
	}
	if err := w.Close(); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

// doRequest performs one exchange, retrying the failures that are worth
// retrying: 429, 502, 503, 504 and a connection error. Any other 4xx means the
// request is malformed and will stay malformed, so it is returned at once.
//
// hold is how long the daemon may keep the request open before it answers, as
// it does for a long poll. Each attempt's timeout is that plus the configured
// timeout; otherwise the client would cancel a poll longer than its own
// timeout, which is every idle claim. Zero for everything else.
func (c *Client) doRequest(ctx context.Context, method, path string, query url.Values, body []byte, encoding string, hold time.Duration) ([]byte, error) {
	if c == nil {
		return nil, ErrNoClient
	}
	if ctx == nil {
		ctx = context.Background()
	}
	target := c.cfg.endpoint + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}

	var lastErr error
	var retryAfter time.Duration
	for attempt := 1; attempt <= maxAttempts; attempt++ {
		if attempt > 1 {
			select {
			case <-time.After(backoff(attempt-1, retryAfter)):
			case <-ctx.Done():
				return nil, ctx.Err()
			}
		}
		data, retryable, wait, err := c.attempt(ctx, method, target, body, encoding, hold)
		if err == nil {
			return data, nil
		}
		lastErr, retryAfter = err, wait
		if !retryable || ctx.Err() != nil {
			return nil, err
		}
	}
	return nil, lastErr
}

func (c *Client) attempt(ctx context.Context, method, target string, body []byte, encoding string, hold time.Duration) ([]byte, bool, time.Duration, error) {
	reqCtx, cancel := context.WithTimeout(ctx, c.cfg.timeout+hold)
	defer cancel()

	var reader io.Reader
	if body != nil {
		reader = bytes.NewReader(body)
	}
	req, err := http.NewRequestWithContext(reqCtx, method, target, reader)
	if err != nil {
		return nil, false, 0, fmt.Errorf("mcpd: request could not be built: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", userAgent)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
		if encoding != "" {
			req.Header.Set("Content-Encoding", encoding)
		}
	}
	if c.cfg.apiKey != "" {
		req.Header.Set("Authorization", "Bearer "+c.cfg.apiKey)
	}

	resp, err := c.http.Do(req)
	if err != nil {
		// The daemon may be restarting, or the socket may have been reset
		// mid-flight. Both are worth another attempt.
		return nil, true, 0, fmt.Errorf("mcpd: %s %s: %w", method, target, err)
	}
	defer func() {
		io.Copy(io.Discard, resp.Body)
		resp.Body.Close()
	}()

	data, readErr := io.ReadAll(io.LimitReader(resp.Body, maxResponseBytes))
	if resp.StatusCode >= 200 && resp.StatusCode < 300 {
		if readErr != nil {
			return nil, true, 0, fmt.Errorf("mcpd: response could not be read: %w", readErr)
		}
		return data, false, 0, nil
	}

	apiErr := parseAPIError(resp.StatusCode, data)
	if isRetryable(resp.StatusCode) {
		return nil, true, parseRetryAfter(resp.Header.Get("Retry-After")), apiErr
	}
	return nil, false, 0, apiErr
}

func isRetryable(status int) bool {
	switch status {
	case http.StatusTooManyRequests, http.StatusBadGateway,
		http.StatusServiceUnavailable, http.StatusGatewayTimeout:
		return true
	}
	return false
}

func parseAPIError(status int, body []byte) error {
	apiErr := &APIError{Status: status, Body: body}
	var payload struct {
		Error   string `json:"error"`
		Message string `json:"message"`
	}
	if json.Unmarshal(body, &payload) == nil {
		apiErr.Code, apiErr.Message = payload.Error, payload.Message
	}
	if apiErr.Message == "" {
		apiErr.Message = strings.TrimSpace(string(body))
	}
	return apiErr
}

// parseRetryAfter reads the header in both of its forms: a number of seconds,
// or an HTTP date.
func parseRetryAfter(value string) time.Duration {
	value = strings.TrimSpace(value)
	if value == "" {
		return 0
	}
	if seconds, err := strconv.Atoi(value); err == nil && seconds >= 0 {
		return time.Duration(seconds) * time.Second
	}
	if when, err := http.ParseTime(value); err == nil {
		if d := time.Until(when); d > 0 {
			return d
		}
	}
	return 0
}

// backoff is exponential from 250 ms with jitter, so a fleet that lost the
// daemon at the same moment does not come back in lockstep. A Retry-After from
// the server always wins.
func backoff(attempt int, retryAfter time.Duration) time.Duration {
	if retryAfter > 0 {
		if retryAfter > maxBackoff {
			return maxBackoff
		}
		return retryAfter
	}
	window := baseBackoff << (attempt - 1)
	if window > maxBackoff {
		window = maxBackoff
	}
	half := window / 2
	return half + time.Duration(rand.Int63n(int64(half)+1))
}

// ------------------------------------------------------------------ reads ---
//
// The read API mirrors the HTTP surface one for one and returns the generated
// types. Unlike recording, these are explicit calls the caller is waiting on,
// so they return an error rather than swallowing one. They work whether or not
// recording is enabled: turning instrumentation off should not silently turn a
// query into an empty answer.

// TraceQuery filters the trace list. Zero fields are left out of the request.
type TraceQuery struct {
	// Project defaults to the client's project.
	Project string
	// Q is a full-text search across span names, prompts and completions.
	Q string
	// Status keeps only traces with this outcome.
	Status SpanStatus
	// Kind keeps only traces whose root is this kind.
	Kind SpanKind
	// Name is an exact root span name.
	Name string
	// Tags keeps traces carrying all of these tags.
	Tags []string
	// SessionID, ThreadID and UserID filter by correlation.
	SessionID string
	// ThreadID keeps one thread of a session.
	ThreadID string
	// UserID keeps one end user's runs.
	UserID string
	// Environment restricts to one environment.
	Environment string
	// Model keeps traces that used this model.
	Model string
	// From and To bound the window.
	From time.Time
	// To bounds the window, exclusive.
	To time.Time
	// MinDurationMs keeps only traces at least this slow.
	MinDurationMs uint64
	// MaxDurationMs keeps only traces at most this slow.
	MaxDurationMs uint64
	// MinCost keeps only traces at least this expensive.
	MinCost float64
	// MaxCost keeps only traces at most this expensive.
	MaxCost float64
	// MinTokens keeps only traces that used at least this many tokens, input
	// and output together.
	MinTokens uint64
	// HasScore keeps only traces carrying a score from this scorer.
	HasScore string
	// ScoreBelow, with HasScore, is the query that finds bad answers.
	ScoreBelow *float64
	// ScoreAbove, with HasScore, keeps traces where that scorer came in over
	// this value.
	ScoreAbove *float64
	// HasFeedback, when set, keeps only traces a person gave feedback on
	// (true) or only traces with none (false). Nil sends nothing.
	HasFeedback *bool
	// Sort is start_time, duration, cost or span_count.
	Sort string
	// Order is asc or desc.
	Order string
	// Limit is the page size, 1 to 500.
	Limit int
	// Cursor continues a previous page.
	Cursor string
}

// ListTraces returns a page of runs.
func (c *Client) ListTraces(ctx context.Context, query TraceQuery) (*TraceListResponse, error) {
	values := c.projectValues(query.Project)
	setString(values, "q", query.Q)
	setString(values, "status", string(query.Status))
	setString(values, "kind", string(query.Kind))
	setString(values, "name", query.Name)
	setString(values, "tags", strings.Join(query.Tags, ","))
	setString(values, "session_id", query.SessionID)
	setString(values, "thread_id", query.ThreadID)
	setString(values, "user_id", query.UserID)
	setString(values, "environment", query.Environment)
	setString(values, "model", query.Model)
	setTime(values, "from", query.From)
	setTime(values, "to", query.To)
	setUint(values, "min_duration_ms", query.MinDurationMs)
	setUint(values, "max_duration_ms", query.MaxDurationMs)
	setFloat(values, "min_cost", query.MinCost)
	setFloat(values, "max_cost", query.MaxCost)
	setUint(values, "min_tokens", query.MinTokens)
	setString(values, "has_score", query.HasScore)
	if query.ScoreBelow != nil {
		values.Set("score_below", formatFloat(*query.ScoreBelow))
	}
	if query.ScoreAbove != nil {
		values.Set("score_above", formatFloat(*query.ScoreAbove))
	}
	if query.HasFeedback != nil {
		values.Set("has_feedback", strconv.FormatBool(*query.HasFeedback))
	}
	setString(values, "sort", query.Sort)
	setString(values, "order", query.Order)
	setInt(values, "limit", query.Limit)
	setString(values, "cursor", query.Cursor)

	out := &TraceListResponse{}
	if err := c.getJSON(ctx, "/v1/api/traces", values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// GetTrace returns one run with every span, ordered so a waterfall renders
// without a client-side sort. Pass include values such as "logs", "scores" or
// "feedback" to fetch what is attached to it in the same request.
func (c *Client) GetTrace(ctx context.Context, traceID string, include ...string) (*TraceDetail, error) {
	if traceID == "" {
		return nil, errors.New("mcpd: GetTrace needs a trace id")
	}
	values := c.projectValues("")
	setString(values, "include", strings.Join(include, ","))

	out := &TraceDetail{}
	if err := c.getJSON(ctx, "/v1/api/traces/"+url.PathEscape(traceID), values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// DeleteTrace removes a run and everything attached to it, which is how an
// erasure request is satisfied.
func (c *Client) DeleteTrace(ctx context.Context, traceID string) (*DeleteResult, error) {
	if traceID == "" {
		return nil, errors.New("mcpd: DeleteTrace needs a trace id")
	}
	raw, err := c.doRequest(ctx, http.MethodDelete,
		"/v1/api/traces/"+url.PathEscape(traceID), c.projectValues(""), nil, "", 0)
	if err != nil {
		return nil, err
	}
	out := &DeleteResult{}
	if err := json.Unmarshal(raw, out); err != nil {
		return nil, decodeError(err)
	}
	return out, nil
}

// SpanQuery filters the span search.
type SpanQuery struct {
	// Project defaults to the client's project.
	Project string
	// Q is a full-text search across name, input and output.
	Q string
	// Kind keeps only this kind of step.
	Kind SpanKind
	// Status keeps only this outcome.
	Status SpanStatus
	// Name is an exact span name.
	Name string
	// Model keeps only spans against this model.
	Model string
	// TraceID restricts the search to one run.
	TraceID string
	// Prompt keeps only spans linked to this prompt with SetPrompt.
	Prompt string
	// PromptVersion, with Prompt, keeps only spans that used this version.
	PromptVersion uint32
	// From and To bound the window.
	From time.Time
	// To bounds the window.
	To time.Time
	// MinDurationMs keeps only spans at least this slow.
	MinDurationMs uint64
	// Sort is start_time, duration or cost.
	Sort string
	// Order is asc or desc.
	Order string
	// Limit is the page size, 1 to 500.
	Limit int
	// Cursor continues a previous page.
	Cursor string
}

// ListSpans searches individual steps across runs: which tool call is slow,
// which model call errored.
func (c *Client) ListSpans(ctx context.Context, query SpanQuery) (*SpanListResponse, error) {
	values := c.projectValues(query.Project)
	setString(values, "q", query.Q)
	setString(values, "kind", string(query.Kind))
	setString(values, "status", string(query.Status))
	setString(values, "name", query.Name)
	setString(values, "model", query.Model)
	setString(values, "trace_id", query.TraceID)
	setString(values, "prompt", query.Prompt)
	setUint(values, "prompt_version", uint64(query.PromptVersion))
	setTime(values, "from", query.From)
	setTime(values, "to", query.To)
	setUint(values, "min_duration_ms", query.MinDurationMs)
	setString(values, "sort", query.Sort)
	setString(values, "order", query.Order)
	setInt(values, "limit", query.Limit)
	setString(values, "cursor", query.Cursor)

	out := &SpanListResponse{}
	if err := c.getJSON(ctx, "/v1/api/spans", values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// LogQuery filters the log search.
type LogQuery struct {
	// Project defaults to the client's project.
	Project string
	// Q is a full-text search across the message and its fields.
	Q string
	// Level is the minimum severity, inclusive.
	Level LogLevel
	// TraceID keeps only lines from one run.
	TraceID string
	// SpanID keeps only lines from one step.
	SpanID string
	// From and To bound the window.
	From time.Time
	// To bounds the window.
	To time.Time
	// Limit is the page size, 1 to 1000.
	Limit int
	// Cursor continues a previous page.
	Cursor string
}

// ListLogs searches log records.
func (c *Client) ListLogs(ctx context.Context, query LogQuery) (*LogListResponse, error) {
	values := c.projectValues(query.Project)
	setString(values, "q", query.Q)
	setString(values, "level", string(query.Level))
	setString(values, "trace_id", query.TraceID)
	setString(values, "span_id", query.SpanID)
	setTime(values, "from", query.From)
	setTime(values, "to", query.To)
	setInt(values, "limit", query.Limit)
	setString(values, "cursor", query.Cursor)

	out := &LogListResponse{}
	if err := c.getJSON(ctx, "/v1/api/logs", values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// MetricQuery aggregates raw points into buckets. Name, From and To are
// required.
type MetricQuery struct {
	// Project defaults to the client's project.
	Project string
	// Name is the metric to query.
	Name string
	// From and To bound the window.
	From time.Time
	// To bounds the window.
	To time.Time
	// Bucket is the width, such as 1m, 5m or 1h. mcpd chooses when it is empty.
	Bucket string
	// Agg is sum, count, mean, min, max, p50, p90, p95 or p99.
	Agg string
	// GroupBy splits the result into one series per label combination.
	GroupBy []string
	// Filter keeps points whose labels have these values. It is sent as
	// key:value pairs joined by commas, so a key cannot contain ':' or ',' and
	// a value cannot contain ','.
	Filter map[string]any
}

// QueryMetrics returns one series per label combination.
func (c *Client) QueryMetrics(ctx context.Context, query MetricQuery) (*MetricSeriesResponse, error) {
	if query.Name == "" {
		return nil, errors.New("mcpd: QueryMetrics needs a metric name")
	}
	values := c.projectValues(query.Project)
	values.Set("name", query.Name)
	setTime(values, "from", query.From)
	setTime(values, "to", query.To)
	setString(values, "bucket", query.Bucket)
	setString(values, "agg", query.Agg)
	setString(values, "group_by", strings.Join(query.GroupBy, ","))
	filter, err := encodeMetricFilter(query.Filter)
	if err != nil {
		return nil, err
	}
	setString(values, "filter", filter)

	out := &MetricSeriesResponse{}
	if err := c.getJSON(ctx, "/v1/api/metrics", values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// ScoreQuery filters the score list.
type ScoreQuery struct {
	// Project defaults to the client's project.
	Project string
	// ScorerID keeps only one scorer's results.
	ScorerID string
	// Source keeps only scores from live, offline, experiment or manual runs.
	Source ScoreSource
	// TraceID keeps only scores for one run.
	TraceID string
	// MinValue and MaxValue bound the value, inclusive.
	MinValue *float64
	// MaxValue bounds the value, inclusive.
	MaxValue *float64
	// From and To bound the window.
	From time.Time
	// To bounds the window.
	To time.Time
	// Limit is the page size.
	Limit int
	// Cursor continues a previous page.
	Cursor string
}

// ListScores returns machine evaluations.
func (c *Client) ListScores(ctx context.Context, query ScoreQuery) (*ScoreListResponse, error) {
	values := c.projectValues(query.Project)
	setString(values, "scorer_id", query.ScorerID)
	setString(values, "source", string(query.Source))
	setString(values, "trace_id", query.TraceID)
	if query.MinValue != nil {
		values.Set("min_value", formatFloat(*query.MinValue))
	}
	if query.MaxValue != nil {
		values.Set("max_value", formatFloat(*query.MaxValue))
	}
	setTime(values, "from", query.From)
	setTime(values, "to", query.To)
	setInt(values, "limit", query.Limit)
	setString(values, "cursor", query.Cursor)

	out := &ScoreListResponse{}
	if err := c.getJSON(ctx, "/v1/api/scores", values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// FeedbackQuery names the run whose feedback to read. The endpoint reads one
// run at a time and answers 400 to any other filter, so these are all it has.
type FeedbackQuery struct {
	// Project defaults to the client's project.
	Project string
	// TraceID is the run whose judgements to read. Required.
	TraceID string
	// Limit is the page size.
	Limit int
}

// ListFeedback returns the human judgements attached to one run.
func (c *Client) ListFeedback(ctx context.Context, query FeedbackQuery) (*FeedbackListResponse, error) {
	if query.TraceID == "" {
		return nil, errors.New("mcpd: ListFeedback needs a TraceID: the daemon reads feedback one run at a time")
	}
	values := c.projectValues(query.Project)
	setString(values, "trace_id", query.TraceID)
	setInt(values, "limit", query.Limit)

	out := &FeedbackListResponse{}
	if err := c.getJSON(ctx, "/v1/api/feedback", values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// AnalyticsSummary returns run count, error rate, latency quantiles, token
// totals and cost for a window. groupBy is optional: model, entity_name,
// environment, status or session_id.
func (c *Client) AnalyticsSummary(ctx context.Context, from, to time.Time, groupBy string) (*AnalyticsSummary, error) {
	values := c.projectValues("")
	setTime(values, "from", from)
	setTime(values, "to", to)
	setString(values, "group_by", groupBy)

	out := &AnalyticsSummary{}
	if err := c.getJSON(ctx, "/v1/api/analytics/summary", values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// AnalyticsCost returns spend over a window, split by groupBy (model by
// default) and optionally bucketed into a series, such as "1h" or "1d".
func (c *Client) AnalyticsCost(ctx context.Context, from, to time.Time, groupBy, bucket string) (*CostBreakdown, error) {
	values := c.projectValues("")
	setTime(values, "from", from)
	setTime(values, "to", to)
	setString(values, "group_by", groupBy)
	setString(values, "bucket", bucket)

	out := &CostBreakdown{}
	if err := c.getJSON(ctx, "/v1/api/analytics/cost", values, out); err != nil {
		return nil, err
	}
	return out, nil
}

// Health reports whether the daemon is alive, which version it runs and which
// spec version it implements.
func (c *Client) Health(ctx context.Context) (*HealthStatus, error) {
	out := &HealthStatus{}
	if err := c.getJSON(ctx, "/healthz", nil, out); err != nil {
		return nil, err
	}
	return out, nil
}

func (c *Client) getJSON(ctx context.Context, path string, values url.Values, out any) error {
	raw, err := c.doRequest(ctx, http.MethodGet, path, values, nil, "", 0)
	if err != nil {
		return err
	}
	if err := json.Unmarshal(raw, out); err != nil {
		return decodeError(err)
	}
	return nil
}

// postJSON sends in as a JSON body and decodes the answer into out, when out
// is not nil. A nil in sends an empty object, which is what the bodiless
// worker calls take.
func (c *Client) postJSON(ctx context.Context, path string, values url.Values, in any, out any) error {
	return c.postJSONHeld(ctx, path, values, in, out, 0)
}

// postJSONHeld is postJSON for a long poll: the daemon may keep the request
// open for hold before it answers, so the request outlives the configured
// timeout by that much.
func (c *Client) postJSONHeld(ctx context.Context, path string, values url.Values, in any, out any, hold time.Duration) error {
	body := []byte("{}")
	if in != nil {
		encoded, err := json.Marshal(in)
		if err != nil {
			return fmt.Errorf("mcpd: request could not be encoded: %w", err)
		}
		body = encoded
	}
	raw, err := c.doRequest(ctx, http.MethodPost, path, values, body, "", hold)
	if err != nil {
		return err
	}
	if out == nil || len(raw) == 0 {
		return nil
	}
	if err := json.Unmarshal(raw, out); err != nil {
		return decodeError(err)
	}
	return nil
}

func decodeError(err error) error {
	return fmt.Errorf("mcpd: response could not be decoded: %w", err)
}

func (c *Client) projectValues(project string) url.Values {
	values := url.Values{}
	if project == "" && c != nil {
		project = c.cfg.project
	}
	if project != "" {
		values.Set("project", project)
	}
	return values
}

// encodeMetricFilter renders label filters the way the daemon reads them:
// key:value pairs joined by commas. JSON in this place matched no point, so a
// filtered query came back as an empty series with a 200. A key or value the
// format cannot carry is refused rather than sent, because the daemon would
// split it into a different filter and answer a different question.
func encodeMetricFilter(filter map[string]any) (string, error) {
	keys := make([]string, 0, len(filter))
	for key := range filter {
		keys = append(keys, key)
	}
	// Sorted, so the same filter is the same query string every time.
	sort.Strings(keys)
	pairs := make([]string, 0, len(keys))
	for _, key := range keys {
		value := fmt.Sprint(filter[key])
		if key == "" || strings.ContainsAny(key, ":,") {
			return "", fmt.Errorf("mcpd: metric filter key %q cannot be empty or contain ':' or ','", key)
		}
		if strings.Contains(value, ",") {
			return "", fmt.Errorf("mcpd: metric filter value %q for %q cannot contain ','", value, key)
		}
		pairs = append(pairs, key+":"+value)
	}
	return strings.Join(pairs, ","), nil
}

func setString(values url.Values, key, value string) {
	if value != "" {
		values.Set(key, value)
	}
}

func setTime(values url.Values, key string, at time.Time) {
	if !at.IsZero() {
		values.Set(key, at.UTC().Format(time.RFC3339Nano))
	}
}

func setInt(values url.Values, key string, value int) {
	if value > 0 {
		values.Set(key, strconv.Itoa(value))
	}
}

func setUint(values url.Values, key string, value uint64) {
	if value > 0 {
		values.Set(key, strconv.FormatUint(value, 10))
	}
}

func setFloat(values url.Values, key string, value float64) {
	if value != 0 {
		values.Set(key, formatFloat(value))
	}
}

func formatFloat(value float64) string {
	return strconv.FormatFloat(value, 'f', -1, 64)
}
