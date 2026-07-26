{{- /*
  Shared helpers for secret wiring (chart-managed vs external).
*/ -}}

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

{{- define "xrelease.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- /* Chart-managed Opaque Secret name (when secrets.existingSecret is empty). */ -}}
{{- define "xrelease.secretsName" -}}
{{- if .Values.secrets.existingSecret -}}
{{- .Values.secrets.existingSecret -}}
{{- else -}}
{{- printf "%s-secrets" (include "xrelease.fullname" .) -}}
{{- end -}}
{{- end }}

{{- /* True when the chart should render an Opaque Secret from values.secrets.*. */ -}}
{{- define "xrelease.createSecret" -}}
{{- if .Values.secrets.existingSecret -}}
{{- false -}}
{{- else if or .Values.secrets.apiKey .Values.secrets.databaseUrl .Values.secrets.webhookSecret .Values.secrets.expressAccessToken .Values.secrets.configEncryptionKey .Values.secrets.sessionSecret .Values.secrets.adminUser .Values.secrets.adminPassword -}}
{{- true -}}
{{- else -}}
{{- false -}}
{{- end -}}
{{- end }}

{{- /* PostgreSQL Service / Secret name. */ -}}
{{- define "xrelease.postgresqlName" -}}
{{- printf "%s-postgresql" (include "xrelease.fullname" .) -}}
{{- end }}

{{- /*
  Fail closed on common from-scratch mistakes. Include from any rendered template:
    {{- include "xrelease.validate" . }}
*/ -}}
{{- define "xrelease.validate" -}}
{{- if gt (int .Values.replicaCount) 1 -}}
{{- fail "xrelease replicaCount must be 1 — one poller per PostgreSQL database (see docs/operations/scaling.md)" -}}
{{- end -}}
{{- if and .Values.secrets.existingSecret (or .Values.secrets.apiKey .Values.secrets.databaseUrl .Values.secrets.webhookSecret .Values.secrets.sessionSecret .Values.secrets.adminPassword) -}}
{{- fail "set either secrets.existingSecret OR secrets.* values — not both (inline keys are ignored when existingSecret is set)" -}}
{{- end -}}
{{- if and .Values.postgresql.enabled (not .Values.postgresql.auth.password) (not .Values.postgresql.existingSecret) -}}
{{- fail "postgresql.enabled requires postgresql.auth.password or postgresql.existingSecret" -}}
{{- end -}}
{{- if and .Values.ui.enabled (eq (default "local" .Values.ui.env.VITE_AUTH_MODE) "local") (not .Values.secrets.existingSecret) -}}
{{- if or (not .Values.secrets.sessionSecret) (eq .Values.secrets.sessionSecret "") (hasPrefix "CHANGE_ME" (.Values.secrets.sessionSecret | toString)) -}}
{{- fail "ui.enabled with VITE_AUTH_MODE=local requires secrets.sessionSecret (values.secrets.yaml or secrets.existingSecret)" -}}
{{- end -}}
{{- if or (not .Values.secrets.adminPassword) (eq .Values.secrets.adminPassword "") (hasPrefix "CHANGE_ME" (.Values.secrets.adminPassword | toString)) -}}
{{- fail "ui.enabled with VITE_AUTH_MODE=local requires secrets.adminPassword (values.secrets.yaml or secrets.existingSecret)" -}}
{{- end -}}
{{- end -}}
{{- /* Lab/API ledger: bootstrapInline often sets source=api — require encryption key when chart-managed. */ -}}
{{- if and (not .Values.secrets.existingSecret) (contains "source = \"api\"" (.Values.config.bootstrapInline | default "")) -}}
{{- if or (not .Values.secrets.configEncryptionKey) (eq .Values.secrets.configEncryptionKey "") (hasPrefix "CHANGE_ME" (.Values.secrets.configEncryptionKey | toString)) -}}
{{- fail "bootstrap source=api requires secrets.configEncryptionKey (openssl rand -base64 32) or secrets.existingSecret" -}}
{{- end -}}
{{- end -}}
{{- end -}}
