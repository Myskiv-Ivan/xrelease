//! HTTP client for a running `xrelease serve` — the transport behind `xrctl`.
//!
//! Deliberately depends on nothing but `reqwest`: these commands talk to a
//! *remote* instance and must not open a database, load a config file, or build
//! any server-side component. That is what makes them usable from a CI runner
//! or a laptop that has no PostgreSQL credentials at all — the same shape as
//! the NewReleases CLI.
//!
//! Config authoring stays whole-document: every mutating call
//! sends a complete desired-state document, so this client never needs
//! field-level endpoints and cannot invent a second config representation.

use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, ETAG, IF_MATCH};
use reqwest::{Client, RequestBuilder, Response, StatusCode};

/// Default management API address, matching `[api].listen`'s default
/// (`127.0.0.1:8080`). Docker Compose UI stack publishes nginx on
/// `http://127.0.0.1:3000` instead — pass `--api-url`.
pub const DEFAULT_API_URL: &str = "http://127.0.0.1:8080";

/// How `POST /config/apply` should set its `If-Match` precondition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IfMatch {
    /// Read the current `ETag` first and require it — safe against a
    /// concurrent editor. The default: a human at a terminal is exactly the
    /// case optimistic concurrency exists for.
    Auto,
    /// Apply unconditionally (CI pushing the committed state, which has no
    /// reason to have read the running config first).
    None,
    /// Require this exact ETag/sha.
    Exact(String),
}

impl IfMatch {
    /// Parse the `--if-match` flag: `auto` / `none` (case-insensitive) or an
    /// explicit ETag/sha. Infallible — any unrecognised value is a literal tag.
    #[must_use]
    pub fn from_flag(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "none" => Self::None,
            _ => Self::Exact(value.trim().to_owned()),
        }
    }
}

/// Connection settings for the management API.
#[derive(Debug, Clone)]
pub struct ApiClient {
    base_url: String,
    /// When set, config commands address this organization's stream
    /// (`/api/v1/organizations/{id}/config/…`).
    organization: Option<String>,
    http: Client,
}

impl ApiClient {
    /// Build a client for `base_url`, sending `api_key` as a bearer token and
    /// scoping config commands to `organization` when one is given.
    ///
    /// The bearer header is parsed once here and installed as a default header,
    /// so every request carries it without re-parsing (and no per-call `?`).
    ///
    /// # Errors
    /// Fails when the api key is not a valid header value, or the HTTP client
    /// cannot be constructed.
    pub fn new(
        base_url: &str,
        api_key: Option<String>,
        organization: Option<String>,
    ) -> anyhow::Result<Self> {
        let mut headers = HeaderMap::new();
        if let Some(key) = api_key.as_deref().filter(|key| !key.is_empty()) {
            let mut value = HeaderValue::from_str(&format!("Bearer {key}"))
                .context("api key is not a valid HTTP header value")?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("xrelease-cli/", env!("CARGO_PKG_VERSION")))
            .default_headers(headers)
            .build()
            .context("building HTTP client")?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            organization: organization
                .map(|org| org.trim().to_owned())
                .filter(|org| !org.is_empty()),
            http,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// Base path for config commands: the whole-document routes, or one
    /// organization's routes when `--organization` is set.
    ///
    /// The org id is percent-encoded like the dashboard client
    /// (`encodeURIComponent`) so path segments stay aligned with OpenAPI.
    fn config_base(&self) -> String {
        match &self.organization {
            Some(org) => format!(
                "/api/v1/organizations/{}/config",
                percent_encode_path_segment(org)
            ),
            None => "/api/v1/config".to_owned(),
        }
    }

    /// `GET /api/v1/organizations` — the organization catalogue.
    pub async fn organizations(&self) -> anyhow::Result<serde_json::Value> {
        self.get_json("/api/v1/organizations").await
    }

    /// `GET …/config` — effective (or per-org desired) config, secrets redacted.
    pub async fn config_show(&self) -> anyhow::Result<serde_json::Value> {
        self.get_json(&self.config_base()).await
    }

    /// `GET /api/v1/config/schema` — option lists for editors (global: the
    /// accepted document shape is identical for every organization).
    pub async fn config_schema(&self) -> anyhow::Result<serde_json::Value> {
        self.get_json("/api/v1/config/schema").await
    }

    /// `GET /api/v1/status` — process overview.
    pub async fn status(&self) -> anyhow::Result<serde_json::Value> {
        self.get_json("/api/v1/status").await
    }

    /// `GET /api/v1/sources` — configured sources with live runtime state.
    ///
    /// When `--organization` is set, passes `?organization=` (same authZ gate
    /// as the dashboard OrgSwitcher).
    pub async fn sources(&self) -> anyhow::Result<serde_json::Value> {
        self.get_json_scoped("/api/v1/sources").await
    }

    /// `GET /api/v1/outbox` — pending/failed notification outbox.
    ///
    /// When `--organization` is set, passes `?organization=`.
    pub async fn outbox(&self) -> anyhow::Result<serde_json::Value> {
        self.get_json_scoped("/api/v1/outbox").await
    }

    /// `GET …/config/revisions` — apply/reject audit history (global ledger,
    /// or one organization's stream).
    pub async fn config_history(&self, limit: i64) -> anyhow::Result<serde_json::Value> {
        self.get_json(&format!("{}/revisions?limit={limit}", self.config_base()))
            .await
    }

    /// Current desired-document ETag, when the server reports one.
    pub async fn current_etag(&self) -> anyhow::Result<Option<String>> {
        let path = self.config_base();
        let response = self
            .http
            .get(self.url(&path))
            .send()
            .await
            .with_context(|| format!("GET {}", self.url(&path)))?;
        let response = ensure_success(response).await?;
        Ok(response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned))
    }

    /// `POST …/config/validate` — dry-run a candidate document.
    pub async fn config_validate(
        &self,
        document: String,
        content_type: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let path = format!("{}/validate", self.config_base());
        let request = self
            .http
            .post(self.url(&path))
            .header(CONTENT_TYPE, content_type)
            .body(document);
        self.send_json(request, "POST", &path).await
    }

    /// `POST /api/v1/config/apply` — whole-document apply with optional
    /// optimistic concurrency and an audit label.
    pub async fn config_apply(
        &self,
        document: String,
        content_type: &str,
        if_match: &IfMatch,
        label: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let precondition = match if_match {
            IfMatch::None => None,
            IfMatch::Exact(tag) => Some(quote_etag(tag)),
            // The server sends its ETag already quoted, so it is used verbatim.
            // A missing ETag must NOT silently downgrade to an unconditional
            // apply — that is the exact lost-update `auto` promises to prevent
            // (a proxy that strips ETag, or an instance with no config yet) —
            // so fail loudly and point at the explicit escape hatch instead.
            IfMatch::Auto => {
                let read_path = self.config_base();
                Some(self.current_etag().await?.ok_or_else(|| {
                    anyhow!(
                        "--if-match auto needs the server's current ETag, but \
 GET {read_path} returned none (no config applied yet, \
 or a proxy stripped the header); re-run with \
 --if-match none to apply unconditionally"
                    )
                })?)
            }
        };

        let path = format!("{}/apply", self.config_base());
        let mut request = self
            .http
            .post(self.url(&path))
            .header(CONTENT_TYPE, content_type);
        if let Some(tag) = precondition {
            request = request.header(IF_MATCH, tag);
        }
        if let Some(label) = label.filter(|label| !label.is_empty()) {
            request = request.header("X-Config-Revision", label);
        }
        self.send_json(request.body(document), "POST", &path).await
    }

    /// `POST …/config/rollback` — re-apply the previous revision (per stream).
    pub async fn config_rollback(&self) -> anyhow::Result<serde_json::Value> {
        self.post_empty(&format!("{}/rollback", self.config_base()))
            .await
    }

    /// `POST /api/v1/reload` — re-read desired state from the server's own
    /// authority (single app file, or every `[[organizations]]` document).
    pub async fn config_reload(&self) -> anyhow::Result<serde_json::Value> {
        self.post_empty("/api/v1/reload").await
    }

    async fn get_json(&self, path: &str) -> anyhow::Result<serde_json::Value> {
        self.send_json(self.http.get(self.url(path)), "GET", path)
            .await
    }

    /// `GET` that optionally scopes with `?organization=` when set.
    async fn get_json_scoped(&self, path: &str) -> anyhow::Result<serde_json::Value> {
        let mut request = self.http.get(self.url(path));
        if let Some(org) = &self.organization {
            request = request.query(&[("organization", org.as_str())]);
        }
        self.send_json(request, "GET", path).await
    }

    /// `POST` with no body — the shape shared by rollback and reload.
    async fn post_empty(&self, path: &str) -> anyhow::Result<serde_json::Value> {
        self.send_json(self.http.post(self.url(path)), "POST", path)
            .await
    }

    /// Send a prepared request and decode its JSON body.
    ///
    /// The single funnel for every verb: it labels the error with the full URL
    /// (so a wrong `--api-url` is visible on any failure, GET or POST) and maps
    /// non-2xx through [`ensure_success`]. Adding a retry or header here reaches
    /// every command at once.
    async fn send_json(
        &self,
        request: RequestBuilder,
        method: &str,
        path: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let response = request
            .send()
            .await
            .with_context(|| format!("{method} {}", self.url(path)))?;
        json_body(ensure_success(response).await?).await
    }
}

/// Percent-encode a single URL path segment (RFC 3986 unreserved stay literal).
///
/// Mirrors the dashboard's `encodeURIComponent` usage on
/// `/api/v1/organizations/{id}/…` so CLI and UI hit the same routes.
#[must_use]
fn percent_encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                use std::fmt::Write as _;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// Wrap an ETag in quotes unless the caller already did.
fn quote_etag(tag: &str) -> String {
    let tag = tag.trim();
    if tag.starts_with('"') || tag.starts_with("W/") {
        tag.to_owned()
    } else {
        format!("\"{tag}\"")
    }
}

/// Turn a non-2xx into an actionable error instead of a bare status.
///
/// The server answers config mistakes with a JSON `error` (and, for a failed
/// validation, a `report`); surfacing those verbatim is the difference between
/// "422 Unprocessable Entity" and knowing which source is misrouted.
async fn ensure_success(response: Response) -> anyhow::Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    // Only hint where the status alone is genuinely ambiguous. 409 in
    // particular has several distinct causes (file-authoritative instance,
    // nothing to roll back to, ledger-vs-file reload conflict) and the server
    // already says which — a guessed hint there would contradict the real
    // message, which is worse than no hint at all.
    let hint = match status {
        StatusCode::UNAUTHORIZED => " — check --api-key (must match the server [api].api_key)",
        StatusCode::NOT_FOUND => {
            " — the config API may be disabled ([config_api].api_config), or the path is wrong"
        }
        StatusCode::PRECONDITION_FAILED => {
            " — config changed since it was read; re-run to pick up the new revision"
        }
        StatusCode::SERVICE_UNAVAILABLE => {
            " — dependency temporarily unavailable (database pool exhausted?); retry shortly"
        }
        _ => "",
    };

    let body = response.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|json| {
            json.get("error")
                .and_then(|error| error.as_str())
                .map(str::to_owned)
        })
        .unwrap_or(body);

    if detail.is_empty() {
        bail!("request failed: {status}{hint}");
    }
    bail!("request failed: {status}{hint}\n {detail}");
}

async fn json_body(response: Response) -> anyhow::Result<serde_json::Value> {
    // A successful response with an empty body is still valid JSON-less; treat
    // it as null rather than an error so callers can print something sane.
    let text = response.text().await.context("reading response body")?;
    if text.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&text).context("parsing response JSON")
}

/// Guess the `Content-Type` for a desired-state document from its path.
///
/// The server sniffs the format when no hint is given, but an explicit header
/// is always better: sniffing is advisory and a wrong guess would otherwise
/// have to be recovered by the parse fallback. Matched case-insensitively so a
/// `releases.YAML` is not mislabeled as TOML.
#[must_use]
pub fn content_type_for_path(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml") => {
            "application/yaml"
        }
        _ => "application/toml",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_etag_should_add_quotes_only_when_missing() {
        assert_eq!(quote_etag("abc123"), "\"abc123\"");
        assert_eq!(quote_etag("\"abc123\""), "\"abc123\"");
        assert_eq!(quote_etag("W/\"abc123\""), "W/\"abc123\"");
        assert_eq!(quote_etag(" abc123 "), "\"abc123\"");
    }

    #[test]
    fn content_type_should_follow_the_file_extension() {
        assert_eq!(
            content_type_for_path(std::path::Path::new("app/releases.yaml")),
            "application/yaml"
        );
        assert_eq!(
            content_type_for_path(std::path::Path::new("desired.yml")),
            "application/yaml"
        );
        assert_eq!(
            content_type_for_path(std::path::Path::new("config.toml")),
            "application/toml"
        );
    }

    #[test]
    fn content_type_should_ignore_extension_case() {
        assert_eq!(
            content_type_for_path(std::path::Path::new("releases.YAML")),
            "application/yaml"
        );
        assert_eq!(
            content_type_for_path(std::path::Path::new("releases.Yml")),
            "application/yaml"
        );
    }

    #[test]
    fn if_match_from_flag_should_map_keywords_case_insensitively() {
        assert_eq!(IfMatch::from_flag("auto"), IfMatch::Auto);
        assert_eq!(IfMatch::from_flag(" AUTO "), IfMatch::Auto);
        assert_eq!(IfMatch::from_flag("none"), IfMatch::None);
        assert_eq!(IfMatch::from_flag("None"), IfMatch::None);
        assert_eq!(
            IfMatch::from_flag("abc123"),
            IfMatch::Exact("abc123".to_owned())
        );
        assert_eq!(
            IfMatch::from_flag(" abc123 "),
            IfMatch::Exact("abc123".to_owned())
        );
    }

    #[test]
    fn base_url_should_lose_its_trailing_slash() {
        let client = ApiClient::new("http://example.test:8080/", None, None).expect("client");
        assert_eq!(
            client.url("/api/v1/config"),
            "http://example.test:8080/api/v1/config"
        );
    }

    #[test]
    fn config_base_should_scope_to_the_organization() {
        let global = ApiClient::new("http://example.test", None, None).expect("client");
        assert_eq!(global.config_base(), "/api/v1/config");

        let org =
            ApiClient::new("http://example.test", None, Some("platform".into())).expect("client");
        assert_eq!(org.config_base(), "/api/v1/organizations/platform/config");

        // Whitespace-only orgs mean "no org" rather than a `//config` path.
        let blank = ApiClient::new("http://example.test", None, Some(" ".into())).expect("client");
        assert_eq!(blank.config_base(), "/api/v1/config");
    }

    #[test]
    fn config_base_should_percent_encode_organization_id() {
        let client =
            ApiClient::new("http://example.test", None, Some("team/ops".into())).expect("client");
        assert_eq!(
            client.config_base(),
            "/api/v1/organizations/team%2Fops/config"
        );
    }

    #[test]
    fn default_api_url_should_match_server_listen_default() {
        // Keep in lockstep with `src/config/api.rs` `default_api_listen`.
        assert_eq!(DEFAULT_API_URL, "http://127.0.0.1:8080");
    }
}
