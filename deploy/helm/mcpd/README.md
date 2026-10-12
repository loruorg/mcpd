# mcpd Helm chart

One Deployment, one Service, one Secret for the keys, and a
PersistentVolumeClaim for the embedded database. Everything else in
`templates/` is off until you turn it on. `values.yaml` explains every setting
and the question it answers.

## Install

No release has been cut yet, so the chart is not in a registry: install it
from a clone of the repository. Generate the three keys first, and keep them:
the ingest key goes to every agent, the read key to whoever reads the
interface, and the admin key to whoever may delete data.

```shell
INGEST_KEY=$(openssl rand -hex 32)
READ_KEY=$(openssl rand -hex 32)
ADMIN_KEY=$(openssl rand -hex 32)

helm install mcpd ./deploy/helm/mcpd -n mcpd --create-namespace \
  --set keys.ingest=$INGEST_KEY \
  --set keys.read=$READ_KEY \
  --set keys.admin=$ADMIN_KEY
```

`keys.admin` is optional to the chart and not optional in practice. Left empty,
the daemon accepts the read keys for admin work, so every read key can delete
traces, erase subjects and read the audit log. The chart keeps the empty default
so an upgrade of an existing release changes nothing, and it says so in the
notes it prints after every install until the key is set.

The release workflow pushes the chart to `oci://ghcr.io/mcpd/charts/mcpd`
beside the image, with the same version. Once a release exists, the same
install is `helm install mcpd oci://ghcr.io/mcpd/charts/mcpd --version <version>`
with the same `--set` flags.

## An external store, and the run surface

`externalStore.type` (`postgres` or `clickhouse`) sends the signals to a
database. The run store is still an embedded SQLite file, so with
`runs.enabled` true (the default) the pod keeps the run store on the volume and
runs as one replica:

```shell
helm install mcpd ./deploy/helm/mcpd -n mcpd --create-namespace \
  --set keys.ingest=$INGEST_KEY --set keys.read=$READ_KEY \
  --set externalStore.type=postgres \
  --set externalStore.postgresUrl=postgres://mcpd:$PGPASSWORD@db:5432/mcpd
```

To run several pods, turn the run surface off. Nothing is then kept on disk,
the chart drops the claim, and the rolling update comes back:

```shell
  --set runs.enabled=false --set replicaCount=3
```

To keep the connection string out of your values, put it in a Secret you
manage under `postgres-url` (or `clickhouse-url` and `clickhouse-password`),
beside the keys, and name it in `keys.existingSecret`. Set
`externalStore.type` all the same: the type, not the URL, selects the backend.

## The config file

Monitors, SLOs, the judge, per-project keys and the run store's timings are
read from the daemon's `--config-file`. Write the JSON object under
`configFile.content`; the chart mounts it read-only and sets
`MCPD_CONFIG_FILE`. Content that carries a credential goes in a Secret:
set `configFile.kind=Secret`, or name a Secret you manage in
`configFile.existingSecret`.

## What the chart refuses to render

Each refusal names the setting and the fix, so it is a message at install time
rather than a crash loop after it. `bash deploy/helm/test-chart.sh` checks that
every one of them still fires.

Keys and exposure:

- no `keys.ingest` or no `keys.read`, unless `keys.existingSecret` names a
  Secret you manage,
- `ingress.enabled` with either of those keys missing, because an Ingress puts
  an open prompt store on a hostname,
- `identity.header` without `identity.trustedProxyCidrs`, because a header is a
  string anyone who can reach the port may send,
- `networkPolicy.enabled` with no ingress source, because the policy would
  refuse every connection, the agents' included.

Stores and replicas:

- an `externalStore.type` other than `none`, `postgres` or `clickhouse`,
- `externalStore.postgresUrl` or `clickhouseUrl` set while `externalStore.type`
  names another backend or none, because the URL would be ignored,
- `externalStore.type` `postgres` or `clickhouse` with no URL and no
  `keys.existingSecret` to carry one,
- a URL or the ClickHouse password set beside `keys.existingSecret`, because
  the chart then creates no Secret to hold it,
- `replicaCount` below 1,
- `replicaCount` above 1 on the embedded store, because an SQLite file has one
  writer,
- `replicaCount` above 1 with `runs.enabled`, because each pod would hold its
  own run store,
- `config.runsPath` outside the directory of `config.dataPath`, the only
  writable mount,
- `config.listen` with no port number after the last colon.

The config file:

- `configFile.content` and `configFile.existingSecret` both set,
- a `configFile.kind` other than `ConfigMap` or `Secret`,
- `configFile.content` that carries per-project keys, `judge.api_key` or
  `forward.headers` while `configFile.kind` is `ConfigMap`.

The disruption budget:

- `podDisruptionBudget.enabled` over one replica, which is any release that
  keeps an SQLite file in the pod or sets `replicaCount` 1, where a budget
  either blocks every drain or permits every one,
- `podDisruptionBudget.minAvailable` and `maxUnavailable` both set,
- `podDisruptionBudget.enabled` with neither of them set.

## Check it

```shell
helm lint ./deploy/helm/mcpd --set keys.ingest=a --set keys.read=b
helm template mcpd ./deploy/helm/mcpd --set keys.ingest=a --set keys.read=b
bash deploy/helm/test-chart.sh
```

`helm lint` alone does not prove a refusal: Helm 4 logs a template's `fail` at
INFO level and passes. The test script asks `helm template`, which does fail.

The deploy page at <https://mcpd.sh/deploy> covers the external stores, the
network policy, the identity header and the upgrade path.
