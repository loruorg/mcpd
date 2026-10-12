{{- define "mcpd.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "mcpd.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "mcpd.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{ include "mcpd.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "mcpd.selectorLabels" -}}
app.kubernetes.io/name: {{ include "mcpd.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{/* The Secret holding the keys, whether this chart made it or not. */}}
{{- define "mcpd.secretName" -}}
{{- if .Values.keys.existingSecret -}}
{{- .Values.keys.existingSecret -}}
{{- else -}}
{{- printf "%s-keys" (include "mcpd.fullname" .) -}}
{{- end -}}
{{- end -}}

{{- define "mcpd.claimName" -}}
{{- if .Values.persistence.existingClaim -}}
{{- .Values.persistence.existingClaim -}}
{{- else -}}
{{- printf "%s-data" (include "mcpd.fullname" .) -}}
{{- end -}}
{{- end -}}

{{/* The ServiceAccount the pod runs as. Empty when the chart creates none and
     none is named, so the Deployment leaves the field out and the pod keeps
     using the namespace's `default` account, exactly as it did before this
     block existed. */}}
{{- define "mcpd.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "mcpd.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/* The signal store: none, postgres or clickhouse. Read from
     externalStore.type and never from the URLs, so a URL that lives only in a
     Secret you manage still selects the backend. Every template that needs the
     mode asks this one helper, so no two of them can disagree. */}}
{{- define "mcpd.storeType" -}}
{{- $t := lower (toString (default "none" .Values.externalStore.type)) -}}
{{- if not (has $t (list "none" "postgres" "clickhouse")) -}}
{{- fail (printf "externalStore.type is %q. Set it to none, postgres or clickhouse." (toString .Values.externalStore.type)) -}}
{{- end -}}
{{- $t -}}
{{- end -}}

{{/* True when the signals go to an external database. */}}
{{- define "mcpd.externalStore" -}}
{{- if ne (include "mcpd.storeType" .) "none" -}}
true
{{- end -}}
{{- end -}}

{{/* True when the daemon serves the run surface. `config.runsPath: off` is
     the older spelling of `runs.enabled: false` and means the same. */}}
{{- define "mcpd.runsEnabled" -}}
{{- if and .Values.runs.enabled (ne (lower (trim (toString .Values.config.runsPath))) "off") -}}
true
{{- end -}}
{{- end -}}

{{/* The run store's path. Always written out rather than left for the daemon
     to derive: with an external store the daemon would derive it from a data
     path that nothing mounts. Derived the way the daemon does it, with `.runs`
     before the extension. */}}
{{- define "mcpd.runsPath" -}}
{{- if .Values.config.runsPath -}}
{{- .Values.config.runsPath -}}
{{- else -}}
{{- $data := .Values.config.dataPath -}}
{{- $ext := ext $data -}}
{{- $stem := trimSuffix $ext (base $data) -}}
{{- printf "%s/%s.runs%s" (dir $data) $stem (default ".db" $ext) -}}
{{- end -}}
{{- end -}}

{{/* True when the pod keeps an SQLite file: the signal store on the embedded
     store, and the run store whenever runs are on. A file has one writer, so
     this is what holds the Deployment to one replica and to Recreate. */}}
{{- define "mcpd.localState" -}}
{{- if eq .Values.mode "relay" -}}
{{- if .Values.persistence.enabled -}}
true
{{- end -}}
{{- else if or (not (include "mcpd.externalStore" .)) (include "mcpd.runsEnabled" .) -}}
true
{{- end -}}
{{- end -}}

{{/* The port the daemon listens on: the number after the last colon of
     config.listen. The containerPort and the NetworkPolicy both use it, so
     moving the listener cannot leave either one on the old port. */}}
{{- define "mcpd.port" -}}
{{- $port := last (splitList ":" (toString .Values.config.listen)) -}}
{{- if not (regexMatch "^[0-9]+$" $port) -}}
{{- fail (printf "config.listen is %q, which ends in no port number. Write it as an address and a port, for example 0.0.0.0:4317." (toString .Values.config.listen)) -}}
{{- end -}}
{{- $port -}}
{{- end -}}

{{/* The object the config file is mounted from: Secret, ConfigMap, or
     nothing when no config file is set. */}}
{{- define "mcpd.configFileSource" -}}
{{- if .Values.configFile.existingSecret -}}
Secret
{{- else if .Values.configFile.content -}}
{{- .Values.configFile.kind -}}
{{- end -}}
{{- end -}}

{{/* Every refusal that is about a combination of values rather than about one
     object. Included once, from the Deployment, so each message prints once.
     Each one names the setting and the fix: a message at install time instead
     of a crash loop after it. */}}
{{- define "mcpd.validate" -}}
{{- if not (has .Values.mode (list "daemon" "relay")) -}}
{{- fail (printf "mode is %q. Set it to daemon or relay." (toString .Values.mode)) -}}
{{- end -}}
{{- if eq .Values.mode "relay" -}}
{{- if not .Values.relay.upstreams -}}
{{- fail "mode is relay but relay.upstreams is empty, so the relay would have nowhere to ship. List the daemons in order of preference, for example [\"https://mcpd.eu-west.example:4317\", \"https://mcpd.us-east.example:4317\"]." -}}
{{- end -}}
{{- if ne (include "mcpd.storeType" .) "none" -}}
{{- fail "mode is relay and externalStore.type is set. A relay opens no store; set externalStore.type on the daemons it ships to, and none here." -}}
{{- end -}}
{{- if and (gt (int .Values.replicaCount) 1) .Values.persistence.enabled -}}
{{- fail (printf "mode is relay with replicaCount %d and persistence.enabled true, so every pod would share one spool volume. Run one release per relay pod, or set persistence.enabled=false to give each pod a spool that lasts as long as the pod." (int .Values.replicaCount)) -}}
{{- end -}}
{{- end -}}
{{- if and .Values.relay.upstreams (ne .Values.mode "relay") -}}
{{- fail "relay.upstreams is set but mode is daemon, so nothing would read it. Set mode=relay, or clear relay.upstreams." -}}
{{- end -}}
{{- $type := include "mcpd.storeType" . -}}
{{- $ext := .Values.externalStore -}}
{{- if and (ne $type "postgres") $ext.postgresUrl -}}
{{- fail (printf "externalStore.postgresUrl is set but externalStore.type is %s, so the chart would not use it. Set externalStore.type=postgres, or clear externalStore.postgresUrl." $type) -}}
{{- end -}}
{{- if and (ne $type "clickhouse") $ext.clickhouseUrl -}}
{{- fail (printf "externalStore.clickhouseUrl is set but externalStore.type is %s, so the chart would not use it. Set externalStore.type=clickhouse, or clear externalStore.clickhouseUrl." $type) -}}
{{- end -}}
{{- if .Values.keys.existingSecret -}}
{{- if or $ext.postgresUrl $ext.clickhouseUrl $ext.clickhousePassword -}}
{{- fail "keys.existingSecret is set, so the chart creates no Secret, and externalStore.postgresUrl, clickhouseUrl or clickhousePassword would be ignored. Put the value in your Secret under postgres-url, clickhouse-url or clickhouse-password, and clear it here." -}}
{{- end -}}
{{- if .Values.forward.headers -}}
{{- fail "keys.existingSecret is set, so the chart creates no Secret, and forward.headers would be ignored. Put the headers in your Secret under forward-headers, one `Name: value` per comma, and clear forward.headers here." -}}
{{- end -}}
{{- else -}}
{{- if and (eq $type "postgres") (not $ext.postgresUrl) -}}
{{- fail "externalStore.type is postgres but no connection string is set. Set externalStore.postgresUrl, or point keys.existingSecret at a Secret that carries it under postgres-url." -}}
{{- end -}}
{{- if and (eq $type "clickhouse") (not $ext.clickhouseUrl) -}}
{{- fail "externalStore.type is clickhouse but no endpoint is set. Set externalStore.clickhouseUrl, or point keys.existingSecret at a Secret that carries it under clickhouse-url." -}}
{{- end -}}
{{- end -}}
{{- $replicas := int .Values.replicaCount -}}
{{- if lt $replicas 1 -}}
{{- fail (printf "replicaCount is %v. Set it to 1 or more." .Values.replicaCount) -}}
{{- end -}}
{{- if and (gt $replicas 1) (ne .Values.mode "relay") (not (include "mcpd.externalStore" .)) -}}
{{- fail (printf "replicaCount is %d, but the signals are stored in an embedded SQLite file, which has one writer: two pods on it corrupt the database. Set replicaCount=1, or set externalStore.type (postgres or clickhouse) and runs.enabled=false to run more than one pod." $replicas) -}}
{{- end -}}
{{- if and (gt $replicas 1) (ne .Values.mode "relay") (include "mcpd.runsEnabled" .) -}}
{{- fail (printf "replicaCount is %d, but runs.enabled is true, and the run store is an embedded SQLite file whatever the signal store is: each pod would hold its own run store, and a run started on one pod would be invisible to the others. Set runs.enabled=false to scale out with no run surface, or replicaCount=1 to keep it." $replicas) -}}
{{- end -}}
{{- $runsPath := toString .Values.config.runsPath -}}
{{- if and (include "mcpd.runsEnabled" .) $runsPath (ne $runsPath ":memory:") -}}
{{- $dataDir := dir .Values.config.dataPath -}}
{{- if not (hasPrefix (printf "%s/" $dataDir) $runsPath) -}}
{{- fail (printf "config.runsPath is %s, outside %s, which is the directory of config.dataPath and the only writable mount, so the daemon could not create it. Put it inside %s, or leave it empty to derive it." $runsPath $dataDir $dataDir) -}}
{{- end -}}
{{- end -}}
{{- $cf := .Values.configFile -}}
{{- if and $cf.content $cf.existingSecret -}}
{{- fail "configFile.content and configFile.existingSecret are both set, and the daemon reads one file. Keep the content in your Secret and clear configFile.content, or clear configFile.existingSecret." -}}
{{- end -}}
{{- if not (has $cf.kind (list "ConfigMap" "Secret")) -}}
{{- fail (printf "configFile.kind is %q. Set it to ConfigMap or Secret." (toString $cf.kind)) -}}
{{- end -}}
{{- if and $cf.content (eq $cf.kind "ConfigMap") -}}
{{- $found := list -}}
{{- range $name, $p := ($cf.content.projects | default dict) -}}
{{- range $k := list "ingest_keys" "read_keys" "write_keys" "admin_keys" "execute_keys" "worker_keys" -}}
{{- if get ($p | default dict) $k -}}
{{- $found = append $found (printf "projects.%s.%s" $name $k) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- if get ($cf.content.judge | default dict) "api_key" -}}
{{- $found = append $found "judge.api_key" -}}
{{- end -}}
{{- $forwards := $cf.content.forward | default list -}}
{{- if kindIs "map" $forwards -}}
{{- $forwards = list $forwards -}}
{{- end -}}
{{- range $i, $f := $forwards -}}
{{- range $k := list "headers" "api_key" "token" "secret_key" "public_key" "dsn" -}}
{{- $v := toString (get ($f | default dict) $k | default "") -}}
{{- if and $v (not (hasPrefix "${" $v)) (ne $v "[]") (ne $v "map[]") -}}
{{- $found = append $found (ternary (printf "forward.%s" $k) (printf "forward[%d].%s" $i $k) (kindIs "map" $cf.content.forward)) -}}
{{- end -}}
{{- end -}}
{{- $url := toString (get ($f | default dict) "endpoint" | default "") -}}
{{- if or (contains "@" $url) (contains "?" $url) -}}
{{- $found = append $found (ternary "forward.endpoint" (printf "forward[%d].endpoint" $i) (kindIs "map" $cf.content.forward)) -}}
{{- end -}}
{{- end -}}
{{- /* A webhook URL is often its own credential, which is why the API returns
       only its host; a URL carrying a user, a password or a query parameter
       usually holds a token. */ -}}
{{- range $i, $m := ($cf.content.monitors | default list) -}}
{{- if get ($m | default dict) "webhook" -}}
{{- $found = append $found (printf "monitors[%d].webhook" $i) -}}
{{- end -}}
{{- end -}}
{{- range $i, $o := ($cf.content.slos | default list) -}}
{{- if get ($o | default dict) "webhook" -}}
{{- $found = append $found (printf "slos[%d].webhook" $i) -}}
{{- end -}}
{{- end -}}
{{- range $block := list "judge" -}}
{{- $url := toString (get ((get $cf.content $block) | default dict) "endpoint" | default "") -}}
{{- if or (contains "@" $url) (contains "?" $url) -}}
{{- $found = append $found (printf "%s.endpoint" $block) -}}
{{- end -}}
{{- end -}}
{{- if $found -}}
{{- fail (printf "configFile.content carries credentials (%s), and configFile.kind is ConfigMap, which anything allowed to read ConfigMaps in the namespace can read. Set configFile.kind=Secret, or keep the file in a Secret you manage and name it in configFile.existingSecret." (join ", " $found)) -}}
{{- end -}}
{{- end -}}
{{- end -}}
