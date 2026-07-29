{{- define "xrelease.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "xrelease.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{- define "xrelease.labels" -}}
helm.sh/chart: {{ include "xrelease.chart" . }}
{{ include "xrelease.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "xrelease.selectorLabels" -}}
app.kubernetes.io/name: {{ include "xrelease.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- /* Pair with app.kubernetes.io/component — name+instance alone matches every workload. */ -}}
{{- define "xrelease.componentSelectorLabels" -}}
{{ include "xrelease.selectorLabels" .ctx }}
app.kubernetes.io/component: {{ .component }}
{{- end }}

{{- /* Always pin chart-built images to amd64. */ -}}
{{- define "xrelease.nodeSelector" -}}
{{- $selector := deepCopy (.Values.nodeSelector | default dict) -}}
{{- $_ := set $selector "kubernetes.io/arch" "amd64" -}}
nodeSelector:
  {{- toYaml $selector | nindent 2 }}
{{- end }}

{{- /* Public egress: carve RFC1918 out of 0.0.0.0/0 when blockPrivateEgress is on. */ -}}
{{- define "xrelease.internetIpBlock" -}}
cidr: 0.0.0.0/0
except:
  - 169.254.0.0/16
  {{- if .Values.networkPolicy.blockPrivateEgress }}
  - 10.0.0.0/8
  - 172.16.0.0/12
  - 192.168.0.0/16
  {{- end }}
{{- end }}

{{- define "xrelease.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "xrelease.secretsName" -}}
{{- if .Values.secrets.existingSecret -}}
{{- .Values.secrets.existingSecret -}}
{{- else -}}
{{- printf "%s-secrets" (include "xrelease.fullname" .) -}}
{{- end -}}
{{- end }}

{{- define "xrelease.createSecret" -}}
{{- if .Values.secrets.existingSecret -}}
{{- false -}}
{{- else -}}
{{- true -}}
{{- end -}}
{{- end }}

{{- /* value → existing Secret (lookup) → generated. ArgoCD / helm template must use existingSecret. */ -}}
{{- define "xrelease.machineSecret" -}}
{{- if .value -}}
{{- .value -}}
{{- else -}}
{{- $found := lookup "v1" "Secret" .ctx.Release.Namespace (.secret | default (include "xrelease.secretsName" .ctx)) -}}
{{- $prev := "" -}}
{{- if and $found $found.data -}}
{{- $prev = index $found.data .key | default "" -}}
{{- end -}}
{{- if $prev -}}
{{- b64dec $prev -}}
{{- else -}}
{{- .gen -}}
{{- end -}}
{{- end -}}
{{- end }}

{{- define "xrelease.postgresqlName" -}}
{{- printf "%s-postgresql" (include "xrelease.fullname" .) -}}
{{- end }}

{{- define "xrelease.cnpgAppSecret" -}}
{{- printf "%s-app" .Values.postgresql.cnpg.clusterName -}}
{{- end }}

{{- define "xrelease.cnpgRwService" -}}
{{- $host := printf "%s-rw" .Values.postgresql.cnpg.clusterName -}}
{{- with .Values.postgresql.cnpg.namespace -}}
{{- $host = printf "%s.%s.svc" $host . -}}
{{- end -}}
{{- $host -}}
{{- end }}

{{- define "xrelease.validate" -}}
{{- if gt (int .Values.replicaCount) 1 -}}
{{- fail "xrelease replicaCount must be 1 — one poller per PostgreSQL database (see docs/operations/scaling.md)" -}}
{{- end -}}
{{- if and .Values.ingress.enabled .Values.gateway.enabled -}}
{{- fail "set either ingress.enabled OR gateway.enabled — both would publish the same hostname twice (see docs/operations/gateway.md)" -}}
{{- end -}}
{{- if and .Values.gateway.enabled (not .Values.gateway.parentRef.name) -}}
{{- fail "gateway.enabled requires gateway.parentRef.name — the Gateway is a platform object, the chart only attaches an HTTPRoute to it" -}}
{{- end -}}
{{- if eq .Values.postgresql.mode "cnpg" -}}
{{- if .Values.postgresql.enabled -}}
{{- fail "postgresql.mode=cnpg requires postgresql.enabled=false — CloudNativePG owns the database, not the chart StatefulSet" -}}
{{- end -}}
{{- if not .Values.postgresql.cnpg.clusterName -}}
{{- fail "postgresql.mode=cnpg requires postgresql.cnpg.clusterName" -}}
{{- end -}}
{{- if and .Values.postgresql.cnpg.create .Values.postgresql.cnpg.namespace -}}
{{- fail "postgresql.cnpg.create=true with a non-empty cnpg.namespace puts the Cluster (and its -app Secret) outside the release namespace — leave namespace empty, or set create=false and copy the Secret in" -}}
{{- end -}}
{{- if .Values.postgresql.cnpg.backup.enabled -}}
{{- if or (not .Values.postgresql.cnpg.backup.destinationPath) (not .Values.postgresql.cnpg.backup.credentialsSecret) -}}
{{- fail "postgresql.cnpg.backup.enabled requires backup.destinationPath and backup.credentialsSecret" -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- if and (ne .Values.postgresql.mode "cnpg") (not .Values.postgresql.enabled) (not .Values.secrets.existingSecret) (not .Values.secrets.databaseUrl) (not .Values.database.url) -}}
{{- fail "no database configured: set postgresql.enabled=true, or postgresql.mode=cnpg, or provide database.url / secrets.databaseUrl / secrets.existingSecret" -}}
{{- end -}}
{{- if and .Values.secrets.existingSecret (or .Values.secrets.apiKey .Values.secrets.databaseUrl .Values.secrets.webhookSecret .Values.secrets.expressAccessToken .Values.secrets.configEncryptionKey .Values.secrets.sessionSecret .Values.secrets.adminPassword) -}}
{{- fail "set either secrets.existingSecret OR secrets.* values — not both (inline keys are ignored when existingSecret is set)" -}}
{{- end -}}
{{- if and .Values.ui.enabled (eq (default "local" .Values.ui.env.VITE_AUTH_MODE) "local") (not .Values.secrets.existingSecret) -}}
{{- if not .Values.secrets.adminPassword -}}
{{- fail "ui.enabled with VITE_AUTH_MODE=local requires secrets.adminPassword — set it in values.secrets.yaml (see deploy/k8s/values.secrets.example.yaml) or use secrets.existingSecret. Everything else is generated." -}}
{{- end -}}
{{- end -}}
{{- range $key, $value := .Values.secrets -}}
{{- if and (kindIs "string" $value) (hasPrefix "CHANGE_ME" $value) -}}
{{- fail (printf "secrets.%s is still the CHANGE_ME placeholder — set a real value or remove the key to let the chart generate one" $key) -}}
{{- end -}}
{{- end -}}
{{- end -}}
