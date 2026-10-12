#!/usr/bin/env bash
# The canonical run over the raw HTTP API, with no SDK at all. This is the probe
# that proves the wire format is usable by a language mcpd ships no SDK for.
set -euo pipefail
E="${MCPD_ENDPOINT:?}"; P="${MCPD_PROJECT:?}"
hex() { head -c "$1" /dev/urandom | od -An -tx1 | tr -d ' \n'; }
T=$(hex 16); R=$(hex 8); S1=$(hex 8); S2=$(hex 8); S3=$(hex 8)
now() { python3 -c "import datetime;print(datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%f000Z'))"; }
EPOCH="1970-01-01T00:00:00.000000000Z"
START=$(now); END=$(now)
# An SDK builds this from its configuration and its span context. Written out
# here, it is the whole of what mcpd asks a client to carry.
corr=$(cat <<J
{"project":"$P","environment":"${MCPD_ENVIRONMENT:-}","service_name":"${MCPD_SERVICE_NAME:-}",
 "service_version":"${MCPD_SERVICE_VERSION:-}","session_id":"sess-conformance","thread_id":"thread-7",
 "user_id":"user-41","request_id":"req-9001","release":"2026.08.1"}
J
)
span() { # name span_id parent kind status is_root extra
  cat <<J
{"trace_id":"$T","span_id":"$2","parent_span_id":$3,"name":"$1","kind":"$4",
 "status":"$5","start_time":"$START","end_time":"$END","duration_ns":1000000,
 "is_root":$6,"is_event":false,"received_at":"$EPOCH","correlation":$corr $7}
J
}
BODY=$(cat <<J
{"spans":[
 $(span support_agent "$R" null agent_run ok true ',"input":"where is my order 41?"'),
 $(span plan_answer "$S1" "\"$R\"" model_generation ok false ',"model":"gpt-4o","provider":"openai","usage":{"input_tokens":1180,"output_tokens":96}'),
 $(span lookup_order "$S2" "\"$R\"" tool_call ok false ''),
 $(span estimate_delivery "$S3" "\"$R\"" tool_call error false ',"error":{"message":"shipping service returned 503"}')
],
"logs":[{"log_id":"$(hex 16)","trace_id":"$T","span_id":"$R","timestamp":"$START","level":"warn",
         "message":"estimate_delivery failed, answering from the cached estimate",
         "fields":{"step":"estimate_delivery"},"received_at":"$EPOCH","correlation":$corr}],
"metrics":[{"metric_id":"$(hex 16)","name":"conformance.run_steps","kind":"counter","value":3,
            "unit":"{step}","labels":{"outcome":"degraded","lang":"curl"},"trace_id":"$T","span_id":"$R",
            "timestamp":"$START","received_at":"$EPOCH","correlation":$corr}],
"scores":[{"score_id":"$(hex 16)","trace_id":"$T","scorer_id":"answer_relevance","source":"live",
           "value":0.86,"min":0,"max":1,"timestamp":"$START","correlation":$corr},
          {"score_id":"$(hex 16)","trace_id":"$T","scorer_id":"scale_check","source":"live",
           "value":3,"min":1,"max":5,"timestamp":"$START","correlation":$corr}],
"feedback":[{"feedback_id":"$(hex 16)","trace_id":"$T","kind":"thumbs","value":"up",
             "timestamp":"$START","correlation":$corr}],
"ingest_key":"$(hex 16)"}
J
)
curl -sf -X POST "$E/v1/ingest" -H 'content-type: application/json' -d "$BODY" > /dev/null
echo "$T"
