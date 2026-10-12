#!/usr/bin/env bash
# mcpd from the command line, with no SDK at all.
#
# Every SDK is a convenience over these five requests. If your language has an
# HTTP client, it can talk to mcpd.
set -euo pipefail

MCPD="${MCPD_ENDPOINT:-http://localhost:4317}"

# #region init
# mcpd needs no registration and no project to be created up front. Check that
# the daemon is up and which spec version it implements.
curl -s "$MCPD/healthz"
# → {"status":"ok","version":"0.1.0","spec_version":"1.0.0", ...}
# #endregion init

# Ids are OpenTelemetry-shaped: 32 hex characters for a trace, 16 for a span.
TRACE=$(openssl rand -hex 16)
ROOT=$(openssl rand -hex 8)
GEN=$(openssl rand -hex 8)
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# #region trace
# One batch carries any mix of the five signals. Send the whole run at once.
curl -s -X POST "$MCPD/v1/ingest" \
  -H 'content-type: application/json' \
  -H "authorization: Bearer ${MCPD_API_KEY:-}" \
  -d @- <<JSON
{
  "ingest_key": "run-$TRACE",
  "spans": [
    {
      "trace_id": "$TRACE",
      "span_id": "$ROOT",
      "name": "support_agent",
      "kind": "agent_run",
      "status": "ok",
      "start_time": "$NOW", "end_time": "$NOW",
      "is_root": true, "is_event": false, "received_at": "$NOW",
      "input":  { "question": "Where is order 4417?" },
      "output": { "answer": "It is in the Leeds depot." },
      "correlation": { "project": "default", "environment": "production" }
    },
    {
      "trace_id": "$TRACE",
      "span_id": "$GEN",
      "parent_span_id": "$ROOT",
      "name": "chat",
      "kind": "model_generation",
      "status": "ok",
      "start_time": "$NOW", "end_time": "$NOW",
      "is_root": false, "is_event": false, "received_at": "$NOW",
      "model": "gpt-4o",
      "provider": "openai",
      "usage": { "input_tokens": 1284, "output_tokens": 210 },
      "correlation": { "project": "default" }
    }
  ]
}
JSON
# → {"accepted":2,"rejected":0,"duplicate":0}
#
# mcpd prices the model span from its own price book on the way in, so cost is
# stored with the run rather than recomputed later from rates that may have
# changed since.
# #endregion trace

# #region score
# Attach an evaluation. `min` and `max` are required: a bare number with no
# declared scale cannot be compared to anything.
curl -s -X POST "$MCPD/v1/api/scores" \
  -H 'content-type: application/json' \
  -d "{
    \"score_id\": \"\",
    \"trace_id\": \"$TRACE\",
    \"scorer_id\": \"answer_relevance\",
    \"source\": \"live\",
    \"value\": 0.94,
    \"min\": 0,
    \"max\": 1,
    \"reason\": \"Names the order and the depot.\",
    \"timestamp\": \"$NOW\",
    \"correlation\": { \"project\": \"default\" }
  }"
# #endregion score

# #region query
# Read it back. Any filter in the API works here.
curl -s "$MCPD/v1/api/traces?project=default&limit=10"

# The whole run, with its logs, scores and feedback in one request.
curl -s "$MCPD/v1/api/traces/$TRACE?include=all"

# Find the run by something the model actually said.
curl -s --get "$MCPD/v1/api/traces" \
  --data-urlencode 'project=default' \
  --data-urlencode 'q=Leeds depot'

# Find the runs a scorer marked down. This is the query that finds bad answers.
curl -s --get "$MCPD/v1/api/traces" \
  --data-urlencode 'project=default' \
  --data-urlencode 'has_score=answer_relevance' \
  --data-urlencode 'score_below=0.5'

# Where the money went, over the last week, by model.
curl -s --get "$MCPD/v1/api/analytics/cost" \
  --data-urlencode 'project=default' \
  --data-urlencode 'from=-7d' \
  --data-urlencode 'group_by=model'
# #endregion query
