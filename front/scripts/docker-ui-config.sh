#!/bin/sh
# Generate /ui-config.js from container env so GHCR users can change VITE_* without rebuild.
# Installed as /docker-entrypoint.d/40-xrelease-ui-config.sh (official nginx image).
set -eu

OUT="${UI_CONFIG_PATH:-/usr/share/nginx/html/ui-config.js}"

json_escape() {
	# Minimal JSON string escape for config values (no newlines expected).
	printf '%s' "$1" | sed \
		-e 's/\\/\\\\/g' \
		-e 's/"/\\"/g' \
		-e 's/	/\\t/g'
}

emit_kv() {
	key="$1"
	val="$2"
	printf '  "%s": "%s"' "$key" "$(json_escape "$val")"
}

# Defaults match public GHCR bake (local login). Override via container env.
AUTH_MODE="${VITE_AUTH_MODE:-local}"
API_URL="${VITE_API_URL-}"
API_KEY_ROLE="${VITE_API_KEY_DEFAULT_ROLE:-admin}"
OIDC_ISSUER="${VITE_OIDC_ISSUER-}"
OIDC_CLIENT_ID="${VITE_OIDC_CLIENT_ID-}"
OIDC_REDIRECT_URI="${VITE_OIDC_REDIRECT_URI-}"
OIDC_SCOPES="${VITE_OIDC_SCOPES:-openid,profile,email,groups}"
OIDC_ROLE_CLAIM="${VITE_OIDC_ROLE_CLAIM:-groups}"
OIDC_ROLE_ADMIN="${VITE_OIDC_ROLE_ADMIN:-xrelease-admin,admin}"
OIDC_ROLE_OPERATOR="${VITE_OIDC_ROLE_OPERATOR:-xrelease-operator,operator}"
OIDC_ROLE_VIEWER="${VITE_OIDC_ROLE_VIEWER:-xrelease-viewer,viewer}"
GRAFANA_EMBED="${VITE_GRAFANA_EMBED_URL-}"

{
	printf 'window.__XRELEASE_UI__ = {\n'
	emit_kv VITE_AUTH_MODE "$AUTH_MODE"
	printf ',\n'
	emit_kv VITE_API_URL "$API_URL"
	printf ',\n'
	emit_kv VITE_API_KEY_DEFAULT_ROLE "$API_KEY_ROLE"
	printf ',\n'
	emit_kv VITE_OIDC_ISSUER "$OIDC_ISSUER"
	printf ',\n'
	emit_kv VITE_OIDC_CLIENT_ID "$OIDC_CLIENT_ID"
	printf ',\n'
	emit_kv VITE_OIDC_REDIRECT_URI "$OIDC_REDIRECT_URI"
	printf ',\n'
	emit_kv VITE_OIDC_SCOPES "$OIDC_SCOPES"
	printf ',\n'
	emit_kv VITE_OIDC_ROLE_CLAIM "$OIDC_ROLE_CLAIM"
	printf ',\n'
	emit_kv VITE_OIDC_ROLE_ADMIN "$OIDC_ROLE_ADMIN"
	printf ',\n'
	emit_kv VITE_OIDC_ROLE_OPERATOR "$OIDC_ROLE_OPERATOR"
	printf ',\n'
	emit_kv VITE_OIDC_ROLE_VIEWER "$OIDC_ROLE_VIEWER"
	printf ',\n'
	emit_kv VITE_GRAFANA_EMBED_URL "$GRAFANA_EMBED"
	printf '\n};\n'
} >"$OUT"

echo "xrelease-ui: wrote $OUT (VITE_AUTH_MODE=$AUTH_MODE)"
