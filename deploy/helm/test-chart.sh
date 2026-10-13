#!/usr/bin/env bash
# Renders the chart under every combination of values that changes its shape,
# and asserts what each one renders. Then asserts that every refusal still
# refuses, with a message that names the setting to change.
#
# A refusal that stops firing is invisible: the chart renders, the install
# succeeds, and the pod crash-loops or quietly does the wrong thing. So the
# failure cases are tested as carefully as the passing ones.
#
# Run from anywhere: bash deploy/helm/test-chart.sh
set -uo pipefail

CHART="$(cd "$(dirname "$0")/mcpd" && pwd)"
KEYS=(--set keys.ingest=test-ingest --set keys.read=test-read)
failures=0
cases=0
out=""

bad() {
  echo "  FAIL  $*"
  failures=$((failures + 1))
}

# render NAME ARGS...: lint and template must both pass. The output is kept in
# $out for the assertions that follow.
render() {
  local name="$1"
  shift
  cases=$((cases + 1))
  current="$name"
  if ! helm lint --quiet "$CHART" "$@" >/dev/null 2>&1; then
    bad "$name: helm lint failed"
    helm lint --quiet "$CHART" "$@" 2>&1 | sed 's/^/        /'
  fi
  if ! out="$(helm template mcpd "$CHART" "$@" 2>&1)"; then
    bad "$name: helm template failed"
    echo "$out" | sed 's/^/        /'
    out=""
    return
  fi
  echo "  ok    $name"
}

# has PATTERN / lacks PATTERN: an extended regular expression over the last
# rendered output.
has() {
  grep -Eq -- "$1" <<<"$out" || bad "$current: expected to render /$1/"
}
lacks() {
  if grep -Eq -- "$1" <<<"$out"; then bad "$current: expected not to render /$1/"; fi
}

# refuse NAME MESSAGE ARGS...: the template must fail, and its error must
# contain MESSAGE. Only `helm template` is asked: Helm 4's lint logs a `fail`
# at INFO level and passes, so a lint result here would prove nothing.
refuse() {
  local name="$1" message="$2"
  shift 2
  cases=$((cases + 1))
  local err
  if err="$(helm template mcpd "$CHART" "$@" 2>&1)"; then
    bad "$name: rendered, expected a refusal naming \"$message\""
    return
  fi
  if ! grep -Fq -- "$message" <<<"$err"; then
    bad "$name: refused, but the message does not contain \"$message\""
    echo "$err" | sed 's/^/        /'
    return
  fi
  echo "  ok    refuses: $name"
}

PG=(--set externalStore.type=postgres --set externalStore.postgresUrl=postgres://u:p@db:5432/mcpd)

echo "renders"

render "embedded store, the defaults" "${KEYS[@]}"
has '^  replicas: 1$'
has 'type: Recreate'
has 'rollingUpdate: null'
has '^kind: PersistentVolumeClaim'

# A number from a values file is a float to Helm, and `quote` printed a large
# one in exponent form: maxBodyBytes 33554432 became "3.3554432e+07", which the
# daemon refuses at start. `--set` parses integers, so only a file shows it.
BIG="$(mktemp)"
trap 'rm -f "$BIG"' EXIT
printf 'maxBodyBytes: 33554432\nmaxBatchSignals: 2000000\nquotaPerMinute: 1000000\nconfig:\n  redactMaxString: 1048576\n  retentionDays: 36500\n' >"$BIG"
render "large numbers from a values file" "${KEYS[@]}" -f "$BIG"
has 'value: "33554432"'
has 'value: "2000000"'
has 'value: "1000000"'
has 'value: "1048576"'
lacks 'e\+0[0-9]'
has 'claimName: mcpd-data'
has 'name: MCPD_DATA'
has 'value: "/data/mcpd.runs.db"'
has 'containerPort: 4317'
has 'terminationGracePeriodSeconds: 45'
has 'runAsUser: 10001'
has 'fsGroup: 10001'
has 'type: RuntimeDefault'
has 'name: MCPD_RETENTION_DAYS'
lacks 'MCPD_CONFIG_FILE'
lacks 'MCPD_POSTGRES_URL'

render "embedded store, persistence off" "${KEYS[@]}" --set persistence.enabled=false
lacks '^kind: PersistentVolumeClaim'
has 'emptyDir: \{\}'
has 'mountPath: /data$'

render "embedded store, an existing claim" "${KEYS[@]}" --set persistence.existingClaim=mine
lacks '^kind: PersistentVolumeClaim'
has 'claimName: mine'

render "embedded store, runs off" "${KEYS[@]}" --set runs.enabled=false
has 'name: MCPD_RUNS_DATA'
has 'value: "off"'

render "the older runsPath off spelling" "${KEYS[@]}" --set config.runsPath=off
has 'value: "off"'

render "a run store path inside the data directory" "${KEYS[@]}" --set config.runsPath=/data/runs.db
has 'value: "/data/runs.db"'

render "retention 0 keeps everything" "${KEYS[@]}" --set config.retentionDays=0
lacks 'MCPD_RETENTION_DAYS'

render "retention empty keeps everything" "${KEYS[@]}" --set-string config.retentionDays=
lacks 'MCPD_RETENTION_DAYS'

render "postgres with runs on" "${KEYS[@]}" "${PG[@]}"
has '^  replicas: 1$'
has 'type: Recreate'
has '^kind: PersistentVolumeClaim'
has 'name: MCPD_POSTGRES_URL'
has 'postgres-url: "postgres://u:p@db:5432/mcpd"'
has 'value: "/data/mcpd.runs.db"'
lacks 'name: MCPD_DATA$'

render "postgres with runs off, three replicas" "${KEYS[@]}" "${PG[@]}" \
  --set runs.enabled=false --set replicaCount=3
has '^  replicas: 3$'
has 'type: RollingUpdate'
lacks 'rollingUpdate: null'
lacks '^kind: PersistentVolumeClaim'
has 'emptyDir: \{\}'
has 'value: "off"'

render "postgres with the URL only in your own Secret" \
  --set keys.existingSecret=mine --set externalStore.type=postgres
lacks '^kind: Secret'
has 'name: MCPD_POSTGRES_URL'
has 'name: mine'

RUNS_PG=(--set runs.store=postgres --set runs.postgresUrl=postgres://u:p@db:5432/mcpd)

render "runs on postgres too, three replicas" "${KEYS[@]}" "${PG[@]}" "${RUNS_PG[@]}" \
  --set replicaCount=3
has '^  replicas: 3$'
has 'type: RollingUpdate'
lacks '^kind: PersistentVolumeClaim'
has 'key: runs-postgres-url'
has 'runs-postgres-url: "postgres://u:p@db:5432/mcpd"'
lacks 'mcpd.runs.db'

render "runs on postgres beside the embedded signal store" "${KEYS[@]}" "${RUNS_PG[@]}"
has '^  replicas: 1$'
has '^kind: PersistentVolumeClaim'
has 'key: runs-postgres-url'
has 'name: MCPD_DATA$'

render "runs on postgres with the URL only in your own Secret" \
  --set keys.existingSecret=mine --set externalStore.type=postgres --set runs.store=postgres
lacks '^kind: Secret'
has 'key: runs-postgres-url'

render "runs off ignore runs.store" "${KEYS[@]}" "${PG[@]}" --set runs.enabled=false \
  --set runs.store=postgres
has 'value: "off"'
lacks 'runs-postgres-url'

render "clickhouse with a user and password" "${KEYS[@]}" \
  --set externalStore.type=clickhouse --set externalStore.clickhouseUrl=http://ch:8123 \
  --set externalStore.clickhouseUser=mcpd --set externalStore.clickhousePassword=pw
has 'name: MCPD_CLICKHOUSE_URL'
has 'name: MCPD_CLICKHOUSE_PASSWORD'
has 'clickhouse-url: "http://ch:8123"'
has 'clickhouse-password: "pw"'

render "clickhouse with the endpoint only in your own Secret" \
  --set keys.existingSecret=mine --set externalStore.type=clickhouse
has 'key: clickhouse-url'
has 'key: clickhouse-password'

render "a listener on another port" "${KEYS[@]}" --set config.listen=0.0.0.0:8080 \
  --set networkPolicy.enabled=true --set 'networkPolicy.ingress.ipBlocks[0].cidr=10.0.0.0/8'
has 'containerPort: 8080'
has '^          port: 8080$'
lacks 'containerPort: 4317'

render "a config file in a ConfigMap" "${KEYS[@]}" \
  --set configFile.content.monitor_interval_seconds=30 \
  --set 'configFile.content.projects.checkout.retention_days=90'
has '^kind: ConfigMap'
has 'name: MCPD_CONFIG_FILE'
has 'value: /etc/mcpd/mcpd.json'
has 'checksum/config:'
has 'configMap:'
has 'readOnly: true'

render "a config file with project keys, in a Secret" "${KEYS[@]}" --set configFile.kind=Secret \
  --set 'configFile.content.projects.checkout.ingest_keys[0]=k1'
has 'name: mcpd-config'
has 'secretName: mcpd-config'
lacks '^kind: ConfigMap'

render "a config file from your own Secret" "${KEYS[@]}" \
  --set configFile.existingSecret=mcpd-file --set configFile.existingSecretKey=config.json
has 'secretName: mcpd-file'
has 'key: config.json'
has 'name: MCPD_CONFIG_FILE'
lacks 'checksum/config:'

render "extra env, volumes and mounts" "${KEYS[@]}" \
  --set 'extraEnv[0].name=MCPD_LOG' --set 'extraEnv[0].value=mcpd=debug' \
  --set 'extraVolumes[0].name=ca' --set 'extraVolumes[0].configMap.name=ca-bundle' \
  --set 'extraVolumeMounts[0].name=ca' --set 'extraVolumeMounts[0].mountPath=/etc/ssl/certs'
has 'name: MCPD_LOG'
has 'name: ca-bundle'
has 'mountPath: /etc/ssl/certs'

render "a budget over several replicas" "${KEYS[@]}" "${PG[@]}" --set runs.enabled=false \
  --set replicaCount=2 --set podDisruptionBudget.enabled=true
has '^kind: PodDisruptionBudget'
has 'minAvailable: 1'

render "every optional object on" "${KEYS[@]}" --set ingress.enabled=true \
  --set serviceAccount.create=true --set serviceMonitor.enabled=true \
  --set networkPolicy.enabled=true --set networkPolicy.egress.enabled=true \
  --set 'networkPolicy.ingress.namespaceSelectors[0].matchLabels.team=agents' \
  --set identity.header=x-forwarded-user --set 'identity.trustedProxyCidrs[0]=10.42.0.0/16'
has '^kind: Ingress'
has '^kind: ServiceAccount'
has '^kind: ServiceMonitor'
has '^kind: NetworkPolicy'
has 'name: MCPD_TRUSTED_IDENTITY_HEADER'

render "open sign-up behind a proxy" "${KEYS[@]}" \
  --set signup.enabled=true --set identity.clientIpHeader=cf-connecting-ip \
  --set 'identity.trustedProxyCidrs[0]=172.16.0.0/12'
has 'name: MCPD_SIGNUP'
has 'name: MCPD_CLIENT_IP_HEADER'
has 'name: MCPD_TRUSTED_PROXY_CIDRS'
lacks 'name: MCPD_TRUSTED_IDENTITY_HEADER'

render "accounts with mail and single sign-on" "${KEYS[@]}" \
  --set accounts.enabled=true --set accounts.publicUrl=https://mcpd.example.com \
  --set smtp.host=smtp.example.com --set smtp.from=no-reply@example.com \
  --set smtp.password=s3cret --set oidc.clientSecret=abc
has 'name: MCPD_ACCOUNTS'
has 'name: MCPD_SMTP_HOST'
has 'key: smtp-password'
has 'name: OIDC_CLIENT_SECRET'
lacks 'name: MCPD_SIGNUP'

render "a vendor destination from the flags" "${KEYS[@]}" \
  --set forward.vendor=honeycomb --set 'forward.headers[0]=x-honeycomb-team: k' \
  --set spool.dir=/data/spool --set spool.maxBytes=1048576
has 'name: MCPD_FORWARD_VENDOR'
has 'value: "honeycomb"'
has 'name: MCPD_SPOOL_DIR'
has 'value: "1048576"'
has 'forward-headers: "x-honeycomb-team: k"'

render "several destinations in a Secret config file" "${KEYS[@]}" --set configFile.kind=Secret \
  --set 'configFile.content.forward[0].vendor=langfuse' \
  --set 'configFile.content.forward[0].secret_key=sk-lf-1'
has 'secretName: mcpd-config'

render "destinations reading their keys from the environment, in a ConfigMap" "${KEYS[@]}" \
  --set 'configFile.content.forward[0].vendor=datadog' \
  --set 'configFile.content.forward[0].api_key=${DD_API_KEY}'
has '^kind: ConfigMap'

EXAMPLES="$(cd "$(dirname "$0")/examples" && pwd)"
render "example: the core region" --set keys.existingSecret=mcpd-keys -f "$EXAMPLES/region-core.yaml"
has '^  replicas: 3$'
has 'type: RollingUpdate'
has 'name: MCPD_POSTGRES_URL'
has '^kind: PodDisruptionBudget'
lacks 'args: \["relay"\]'

render "example: the standby region" --set keys.existingSecret=mcpd-keys -f "$EXAMPLES/region-standby.yaml"
has '^  replicas: 2$'
has 'name: MCPD_POSTGRES_URL'

render "example: an edge relay" --set keys.ingest=edge --set relay.upstreamKey=core \
  -f "$EXAMPLES/region-edge-relay.yaml"
has 'args: \["relay"\]'
has 'name: MCPD_UPSTREAMS'
has 'value: "https://mcpd.eu-west.example:4317,https://mcpd.us-east.example:4317"'
has 'upstream-key: "core"'
has 'value: "4294967296"'
has '^kind: PersistentVolumeClaim'
has 'type: Recreate'
lacks 'read-keys'
lacks 'MCPD_POSTGRES_URL'

render "relays without a volume may scale out" --set mode=relay --set keys.ingest=edge \
  --set 'relay.upstreams[0]=https://core:4317' --set persistence.enabled=false --set replicaCount=3
has '^  replicas: 3$'
lacks '^kind: PersistentVolumeClaim'
has 'emptyDir: \{\}'

echo
echo "refuses"

refuse "smtp with no from address" "smtp.from" "${KEYS[@]}" --set smtp.host=smtp.example.com

refuse "no keys" "keys.ingest is required"
refuse "no read key" "keys.read is required" --set keys.ingest=a
refuse "an ingress with no keys" "keys.read" --set ingress.enabled=true --set keys.ingest=a
refuse "an identity header with no trusted ranges" "identity.trustedProxyCidrs is empty" \
  "${KEYS[@]}" --set identity.header=x-forwarded-user
refuse "a network policy with no source" "no ingress source is set" \
  "${KEYS[@]}" --set networkPolicy.enabled=true
refuse "a budget on the embedded store" "podDisruptionBudget.enabled is true, but the Deployment runs one replica" \
  "${KEYS[@]}" --set podDisruptionBudget.enabled=true
refuse "a budget over one pod with runs on" "runs.enabled=false" \
  "${KEYS[@]}" "${PG[@]}" --set podDisruptionBudget.enabled=true
refuse "a budget with both bounds" "are both set" \
  "${KEYS[@]}" "${PG[@]}" --set runs.enabled=false --set replicaCount=2 \
  --set podDisruptionBudget.enabled=true --set podDisruptionBudget.maxUnavailable=1
refuse "a budget with neither bound" "neither minAvailable nor maxUnavailable" \
  "${KEYS[@]}" "${PG[@]}" --set runs.enabled=false --set replicaCount=2 \
  --set podDisruptionBudget.enabled=true --set-string podDisruptionBudget.minAvailable=
refuse "two replicas on the embedded store" "replicaCount is 2, but the signals are stored in an embedded SQLite file" \
  "${KEYS[@]}" --set replicaCount=2
refuse "two replicas with runs on" "Set runs.store=postgres with runs.postgresUrl" \
  "${KEYS[@]}" "${PG[@]}" --set replicaCount=2
refuse "an unknown run store" "Set it to sqlite or postgres" \
  "${KEYS[@]}" --set runs.store=mysql
refuse "a run store URL with runs.store sqlite" "Set runs.store=postgres, or clear runs.postgresUrl" \
  "${KEYS[@]}" --set runs.postgresUrl=postgres://db/mcpd
refuse "runs.store postgres with no URL" "Set runs.postgresUrl" \
  "${KEYS[@]}" --set runs.store=postgres
refuse "a run store URL beside an existing Secret" "under runs-postgres-url" \
  --set keys.existingSecret=mine "${RUNS_PG[@]}"
refuse "no replicas" "Set it to 1 or more" "${KEYS[@]}" --set replicaCount=0
refuse "a Postgres URL with type none" "Set externalStore.type=postgres" \
  "${KEYS[@]}" --set externalStore.postgresUrl=postgres://db/mcpd
refuse "a ClickHouse URL with type postgres" "Set externalStore.type=clickhouse" \
  "${KEYS[@]}" "${PG[@]}" --set externalStore.clickhouseUrl=http://ch:8123
refuse "type postgres with no URL" "Set externalStore.postgresUrl" \
  "${KEYS[@]}" --set externalStore.type=postgres
refuse "type clickhouse with no URL" "Set externalStore.clickhouseUrl" \
  "${KEYS[@]}" --set externalStore.type=clickhouse
refuse "an unknown store type" "Set it to none, postgres or clickhouse" \
  "${KEYS[@]}" --set externalStore.type=mysql
refuse "a URL beside an existing Secret" "Put the value in your Secret under postgres-url" \
  --set keys.existingSecret=mine "${PG[@]}"
refuse "a run store outside the data volume" "config.runsPath is /var/runs.db, outside /data" \
  "${KEYS[@]}" --set config.runsPath=/var/runs.db
refuse "a listener with no port" "Write it as an address and a port" \
  "${KEYS[@]}" --set config.listen=localhost
refuse "credentials in a ConfigMap" "configFile.content carries credentials (projects.checkout.read_keys)" \
  "${KEYS[@]}" --set 'configFile.content.projects.checkout.read_keys[0]=k1'
refuse "a judge key in a ConfigMap" "judge.api_key" \
  "${KEYS[@]}" --set configFile.content.judge.api_key=sk
refuse "content beside an existing config Secret" "the daemon reads one file" \
  "${KEYS[@]}" --set configFile.existingSecret=mine --set configFile.content.monitor_interval_seconds=30
refuse "an unknown config file kind" "Set it to ConfigMap or Secret" \
  "${KEYS[@]}" --set configFile.kind=File --set configFile.content.monitor_interval_seconds=30
refuse "a webhook URL in a ConfigMap" "monitors[0].webhook" "${KEYS[@]}" \
  --set 'configFile.content.monitors[0].id=errors' \
  --set 'configFile.content.monitors[0].webhook=https://hooks.slack.com/services/T0/B0/SECRET'
refuse "a forward endpoint with a user in a ConfigMap" "forward.endpoint" "${KEYS[@]}" \
  --set 'configFile.content.forward.endpoint=https://user:pass@collector.example:4318'
refuse "a judge endpoint with a query in a ConfigMap" "judge.endpoint" "${KEYS[@]}" \
  --set 'configFile.content.judge.endpoint=https://judge.example/v1?key=abc'
render "a plain forward endpoint in a ConfigMap" "${KEYS[@]}" \
  --set 'configFile.content.forward.endpoint=https://collector.example:4318'
has '^kind: ConfigMap'
render "a webhook in a Secret" "${KEYS[@]}" --set configFile.kind=Secret \
  --set 'configFile.content.monitors[0].id=errors' \
  --set 'configFile.content.monitors[0].webhook=https://hooks.slack.com/services/T0/B0/SECRET'
refuse "forward headers beside an existing key Secret" "forward.headers would be ignored" \
  --set keys.existingSecret=mine --set 'forward.endpoint=https://collector.example:4318' \
  --set 'forward.headers[0]=Authorization: Bearer x'

refuse "a relay with no upstream" "relay.upstreams is empty" \
  --set mode=relay --set keys.ingest=a
refuse "a relay with a store" "A relay opens no store" \
  --set mode=relay --set keys.ingest=a --set 'relay.upstreams[0]=https://core' "${PG[@]}"
refuse "relay pods sharing one spool volume" "share one spool volume" \
  --set mode=relay --set keys.ingest=a --set 'relay.upstreams[0]=https://core' --set replicaCount=2
refuse "upstreams on a daemon" "relay.upstreams is set but mode is daemon" \
  "${KEYS[@]}" --set 'relay.upstreams[0]=https://core'
refuse "an unknown mode" "Set it to daemon or relay" "${KEYS[@]}" --set mode=edge
refuse "a destination key in a ConfigMap" "forward[0].api_key" "${KEYS[@]}" \
  --set 'configFile.content.forward[0].vendor=honeycomb' \
  --set 'configFile.content.forward[0].api_key=hc-plain-key'

echo
if [ "$failures" -gt 0 ]; then
  echo "chart: $failures of the assertions over $cases cases failed"
  exit 1
fi
echo "chart: $cases cases, every rendering and every refusal as expected"
