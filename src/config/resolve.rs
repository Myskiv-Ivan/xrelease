//! Boot-time config resolution — infra bootstrap + app desired state (file or ledger).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Serialize;

use super::desired_format::{
    load_desired_file, parse_desired_document, parse_desired_document_with_hint,
};
use super::paths::ConfigPaths;
use super::{apply_env_overrides, Config, Defaults};
use crate::store::{ConfigRevisionRecord, Store};

/// Where the running desired-state sections came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredSource {
    /// Latest `applied` row in `config_revision`.
    Ledger,
    /// On-disk app config file (`--app` / `app/releases.yaml` / `[[organizations]].app`).
    AppFile,
    /// API/UI org with no seed file and no ledger revision yet (empty placeholder).
    Empty,
}

/// Minimal desired document for an API-authored organization before the first apply.
///
/// Parses as desired-only with no sources/notifiers — compose skips the
/// "must have desired state" check for [`DesiredSource::Empty`].
pub const EMPTY_ORGANIZATION_DOCUMENT: &str = "{}\n";

/// Derive desired-state provenance from revision metadata (HTTP handlers).
#[must_use]
pub fn desired_source_from_revision(
    effective_revision: Option<&EffectiveRevision>,
) -> DesiredSource {
    if effective_revision.is_some() {
        DesiredSource::Ledger
    } else {
        DesiredSource::AppFile
    }
}

/// Whether the config carries application-layer sections.
#[must_use]
pub fn contains_app_sections(config: &Config) -> bool {
    !config.sources.is_empty()
        || !config.notifiers.is_empty()
        || !config.teams.is_empty()
        || !config.presets.is_empty()
        || config.defaults != Defaults::default()
}

/// Whether the config carries infrastructure-layer sections (bootstrap-only).
#[must_use]
pub fn contains_infra_sections(config: &Config) -> bool {
    let default = Config::default();
    config.database != default.database
        || config.api != default.api
        || config.log != default.log
        || config.config_api != default.config_api
        // `[advisories]` is an outbound endpoint + credentials-free network
        // toggle, not desired state: an operator enabling third-party lookups
        // is an infrastructure decision, so it must not arrive via a pushed
        // document.
        || config.advisories != default.advisories
}

/// Reject infrastructure sections in a desired-state document (app YAML / POST body).
pub fn ensure_desired_only(config: &Config) -> anyhow::Result<()> {
    if contains_infra_sections(config) {
        bail!(
            "desired-state document must not contain infrastructure sections \
 ([database], [api], [log], [config_api], [advisories]); configure those in \
 bootstrap.toml only"
        );
    }
    if !config.organizations.is_empty() {
        bail!(
            "desired-state document must not contain [[organizations]]; \
 declare the organization catalogue in bootstrap.toml only"
        );
    }
    if config.apprise != crate::config::AppriseConfig::default() {
        bail!(
            "top-level `[apprise]` / `apprise:` is removed; move delivery settings to \
 `[[notifiers]]` / `notifiers:` with `type: apprise` \
 (Compose/Helm may still set XRELEASE_APPRISE_ENDPOINT on those sinks)"
        );
    }
    Ok(())
}

/// Whether the config carries application sections (sources / notifiers / …).
#[must_use]
pub fn has_desired_state(config: &Config) -> bool {
    contains_app_sections(config)
}

/// Clear application-layer sections, keeping infra fields intact.
pub fn strip_desired_sections(config: &mut Config) {
    config.defaults = Defaults::default();
    config.apprise = Default::default();
    config.notifiers.clear();
    config.teams.clear();
    config.presets.clear();
    config.sources.clear();
}

/// Clear bootstrap-only sections so a desired document can be serialized without
/// `Config::default()` infra pollution (`[config_api] source = local`, …).
pub fn strip_infra_sections(config: &mut Config) {
    config.database = Default::default();
    config.log = Default::default();
    config.api = Default::default();
    config.config_api = Default::default();
    config.advisories = Default::default();
    config.organizations.clear();
}

/// Overlay bootstrap-only sections from `bootstrap` onto `desired`.
#[must_use]
pub fn merge_bootstrap_over_desired(bootstrap: &Config, mut desired: Config) -> Config {
    desired.database = bootstrap.database.clone();
    desired.log = bootstrap.log.clone();
    desired.api = bootstrap.api.clone();
    desired.config_api = bootstrap.config_api.clone();
    desired.advisories = bootstrap.advisories.clone();
    desired.organizations = bootstrap.organizations.clone();
    desired
}

/// Parse bootstrap TOML from disk without env overrides.
pub fn parse_bootstrap_file(path: &Path) -> anyhow::Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading bootstrap config {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing bootstrap config {}", path.display()))
}

/// Reject application sections in a bootstrap file — infra TOML only.
pub fn ensure_infra_only(config: &Config, path: &Path) -> anyhow::Result<()> {
    if contains_app_sections(config) {
        bail!(
            "{} must contain only infrastructure sections \
 ([database], [api], [log], [config_api], [[organizations]]); \
 move [defaults], [[teams]], [presets], [[notifiers]], and [[sources]] to \
 app/releases.yaml or app/<org>/releases.yaml (or apply via POST /api/v1/config/apply)",
            path.display()
        );
    }
    Ok(())
}

/// Load bootstrap (infra) config from a local file (or defaults when missing).
pub fn load_bootstrap(path: &Path) -> anyhow::Result<Config> {
    let mut config = if path.exists() {
        let parsed = parse_bootstrap_file(path)?;
        ensure_infra_only(&parsed, path)?;
        parsed
    } else {
        Config::default()
    };
    apply_env_overrides(&mut config);
    strip_desired_sections(&mut config);
    Ok(config)
}

/// Load infra-only bootstrap for `paths.bootstrap`.
pub fn load_infra_bootstrap(paths: &ConfigPaths) -> anyhow::Result<Config> {
    load_bootstrap(&paths.bootstrap)
}

/// Resolve the effective runtime config:
///
/// 1. Infra bootstrap from `paths.bootstrap` + env (TOML, infra sections only).
/// 2. When `[[organizations]]` is non-empty: compose every org from its own
///    authority — its ledger stream when `source = "api"` holds a row, its
///    app file otherwise ([`compose_organizations`]).
/// 3. Else latest `applied` revision from PostgreSQL when available.
/// 4. Else on-disk app file (`paths.app` or auto-discovered `app/releases.yaml`).
/// 5. Fail loudly when neither ledger nor app file provides desired state.
pub fn resolve(paths: &ConfigPaths, store: Option<&Store>) -> anyhow::Result<Config> {
    let bootstrap = load_infra_bootstrap(paths)?;

    if !bootstrap.organizations.is_empty() {
        return Ok(compose_organizations(&bootstrap, paths, store, None)?.merged);
    }

    // `[config_api].source = "local"` pins authority to the app file, so a
    // ledger revision left over from an earlier api-mode run must not
    // silently win at boot.
    if bootstrap.config_api.ledger_is_bootable() {
        if let Some(store) = store {
            if let Some(revision) = store.latest_applied_config_revision(None)? {
                let mut desired = parse_desired_document(&revision.content)?;
                ensure_desired_only(&desired)?;
                apply_env_overrides(&mut desired);
                tracing::info!(
                revision = revision.id,
                sha = %revision.content_sha256,
                "desired config resolved from the ledger"
                );
                return Ok(merge_bootstrap_over_desired(&bootstrap, desired));
            }
        }
    }

    let app_path = paths.app.as_ref();
    match app_path {
        Some(app_path) if app_path.exists() => {
            let mut desired = load_desired_file(app_path)?;
            ensure_desired_only(&desired)?;
            apply_env_overrides(&mut desired);
            if !has_desired_state(&desired) {
                if bootstrap.config_api.ledger_is_bootable() {
                    tracing::warn!(
                    path = %app_path.display(),
                    "app config defines no desired-state sections — idle until UI/CI apply"
                    );
                    return Ok(merge_bootstrap_over_desired(&bootstrap, Config::default()));
                }
                bail!(
 "app config {} defines no desired-state sections (sources / notifiers / apprise)",
 app_path.display()
 );
            }
            tracing::info!(
            path = %app_path.display(),
            "desired config resolved from the app file"
            );
            Ok(merge_bootstrap_over_desired(&bootstrap, desired))
        }
        _ if bootstrap.config_api.ledger_is_bootable() => {
            // UI-first single document: no seed file and no ledger row yet.
            tracing::warn!(
                "API/UI mode with no ledger revision and no app file — idle until first apply"
            );
            Ok(merge_bootstrap_over_desired(&bootstrap, Config::default()))
        }
        None => bail!(
            "no applied config revision in PostgreSQL and no app config file — \
 set --app app/releases.yaml, declare [[organizations]] in bootstrap.toml, \
 or apply desired state via POST /api/v1/config/apply"
        ),
        Some(app_path) => bail!(
            "app config file {} does not exist (no applied revision in PostgreSQL)",
            app_path.display()
        ),
    }
}

/// Resolve one organization's desired-state document from its authority.
///
/// Authority order mirrors the single-document `resolve` exactly, but scoped
/// to the organization's own ledger stream: when
/// `[config_api].source` permits the ledger and a store is reachable, the
/// newest `applied` row for this org wins; otherwise the `[[organizations]]`
/// app file is read. `source = "local"` pins every org to Git and requires
/// `app`. With `source = "api"`, `app` is optional — missing seed and ledger
/// yields [`DesiredSource::Empty`] so the process boots idle for UI apply.
pub fn organization_desired_raw(
    bootstrap: &Config,
    paths: &ConfigPaths,
    store: Option<&Store>,
    org: &super::OrganizationConfig,
) -> anyhow::Result<(String, DesiredSource)> {
    let id = &org.id.clone();
    if bootstrap.config_api.ledger_is_bootable() {
        if let Some(store) = store {
            if let Some(revision) = store.latest_applied_config_revision(Some(id))? {
                return Ok((revision.content, DesiredSource::Ledger));
            }
        }
    }

    match organization_app_path(paths, org) {
        Some(app_path) => {
            if !app_path.exists() {
                bail!(
                    "organization `{id}` app file {} does not exist",
                    app_path.display()
                );
            }
            let raw = std::fs::read_to_string(&app_path).with_context(|| {
                format!(
                    "reading organization `{id}` app file {}",
                    app_path.display()
                )
            })?;
            Ok((raw, DesiredSource::AppFile))
        }
        None if bootstrap.config_api.ledger_is_bootable() => {
            Ok((EMPTY_ORGANIZATION_DOCUMENT.to_owned(), DesiredSource::Empty))
        }
        None => bail!(
            "organization `{id}` has no `app` path; set `app = \"…\"` when \
 [config_api].source = \"local\", or use source = \"api\" and \
 author via UI / apply into the ledger"
        ),
    }
}

/// Absolute path of one organization's app file (relative paths are resolved
/// against the bootstrap file's directory). `None` when `app` is omitted.
#[must_use]
pub fn organization_app_path(
    paths: &ConfigPaths,
    org: &super::OrganizationConfig,
) -> Option<PathBuf> {
    let raw = Path::new(org.app_path()?);
    if raw.is_absolute() {
        Some(raw.to_path_buf())
    } else {
        let bootstrap_dir = paths
            .bootstrap
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        Some(bootstrap_dir.join(raw))
    }
}

/// Result of composing every organization's desired state into one runtime config.
#[derive(Debug, Clone)]
pub struct ComposedOrganizations {
    /// Bootstrap + all orgs, namespaced and merged — what the runtime runs.
    pub merged: Config,
    /// Identity of the composed inputs: SHA-256 over each org's raw document
    /// sha in catalogue order. Two compositions are equal iff every org's
    /// desired document (from whatever authority) is byte-identical, so reload
    /// can skip an unchanged multi-org state exactly like single-file mode.
    pub identity_sha256: String,
    /// Where each organization's document came from (catalogue order).
    pub sources: Vec<(String, DesiredSource)>,
}

/// Load every `[[organizations]]` document, namespace tags/ids, merge into one
/// runtime [`Config`] — the ONE composition path shared by boot, reload, and
/// per-organization apply (which passes `override_document` for the org being
/// changed so the candidate is composed exactly as it would boot).
pub fn compose_organizations(
    bootstrap: &Config,
    paths: &ConfigPaths,
    store: Option<&Store>,
    override_document: Option<(&str, &str)>,
) -> anyhow::Result<ComposedOrganizations> {
    use sha2::{Digest, Sha256};

    use super::organizations::namespace_organization_desired;

    let mut seen_ids = std::collections::HashSet::new();
    let mut merged = Config::default();
    let mut sources = Vec::with_capacity(bootstrap.organizations.len());
    let mut identity = Sha256::new();
    let mut override_used = override_document.is_none();
    // The upstream limiter is PROCESS-wide (one politeness cap for all polls),
    // but each org declares `[defaults].upstream_requests_per_minute` in its
    // own document. Taking the first org's value silently ignored the rest —
    // honor the strictest (lowest non-zero) cap any org asks for instead.
    let mut strictest_rpm: Option<u32> = None;

    for org in &bootstrap.organizations {
        let id = &org.id.clone();
        if id.is_empty() {
            bail!("[[organizations]] entry has an empty id");
        }
        if !seen_ids.insert(id.to_owned()) {
            bail!("duplicate [[organizations]] id `{id}`");
        }

        let (raw, desired_source) = match override_document {
            Some((override_id, document)) if override_id == id => {
                override_used = true;
                (document.to_owned(), DesiredSource::Ledger)
            }
            _ => organization_desired_raw(bootstrap, paths, store, org)?,
        };

        // Per-org identity input: the org id and its document bytes. Feeding
        // the id keeps two orgs with identical documents from cancelling a
        // swap between them.
        identity.update(id.as_bytes());
        identity.update([0u8]);
        identity.update(raw.as_bytes());
        identity.update([0u8]);

        let mut desired = parse_desired_document(&raw)
            .with_context(|| format!("parsing organization `{id}` desired document"))?;
        ensure_desired_only(&desired)?;
        apply_env_overrides(&mut desired);
        // Empty API-pending orgs contribute nothing until the first apply.
        if desired_source != DesiredSource::Empty && !has_desired_state(&desired) {
            bail!("organization `{id}` desired document defines no desired-state sections");
        }
        namespace_organization_desired(id, &mut desired);
        let source_count = desired.sources.len();

        // Prefer the first org's defaults for any source that somehow skipped
        // materialization; sources already carry explicit schedule fields.
        if sources.is_empty() && has_desired_state(&desired) {
            merged.defaults = desired.defaults.clone();
        }
        if has_desired_state(&desired) {
            let rpm = desired.defaults.upstream_requests_per_minute;
            if rpm > 0 {
                strictest_rpm = Some(strictest_rpm.map_or(rpm, |current| current.min(rpm)));
            }
        }
        merged.teams.append(&mut desired.teams);
        merged.notifiers.append(&mut desired.notifiers);
        merged.sources.append(&mut desired.sources);
        for (name, preset) in desired.presets {
            if merged.presets.contains_key(&name) {
                tracing::debug!(
                organization = id,
                preset = %name,
                "preset name collides across organizations; later entry wins"
                );
            }
            merged.presets.insert(name, preset);
        }
        if desired.apprise != super::AppriseConfig::default() {
            bail!(
                "organization `{id}`: top-level `apprise:` is removed; use \
 `notifiers:` with `type: apprise`"
            );
        }

        tracing::debug!(
        organization = id,
        source = ?desired_source,
        sources = source_count,
        "organization desired config loaded"
        );
        sources.push((id.to_owned(), desired_source));
    }

    if !override_used {
        // A typo'd org id must fail the apply, not silently compose the
        // current state and record the document in a stream nothing reads.
        let (override_id, _) = override_document.unwrap_or_default();
        bail!("unknown organization `{override_id}` (not in [[organizations]])");
    }

    if let Some(rpm) = strictest_rpm {
        if merged.defaults.upstream_requests_per_minute != rpm {
            tracing::debug!(
                rpm,
                "multi-org upstream cap: strictest declared value wins (process-wide limiter)"
            );
        }
        merged.defaults.upstream_requests_per_minute = rpm;
    }

    let has_workload = !merged.sources.is_empty() || !merged.notifiers.is_empty();
    if !has_workload {
        if bootstrap.config_api.ledger_is_bootable() {
            tracing::warn!(
                organizations = sources.len(),
                "multi-organization catalogue loaded with no sources/notifiers yet \
 (API/UI orgs awaiting first apply)"
            );
        } else {
            bail!("[[organizations]] loaded but no sources or notifiers were found");
        }
    }

    tracing::info!(
        organizations = sources.len(),
        "multi-organization config resolved"
    );
    Ok(ComposedOrganizations {
        merged: merge_bootstrap_over_desired(bootstrap, merged),
        identity_sha256: hex::encode(identity.finalize()),
        sources,
    })
}

/// Merge a pushed desired-state document with the live bootstrap for validation
/// and hot-swap. Environment overrides are applied after merge so secrets
/// (`XRELEASE_*`) continue to work for apprise/database fields in the merged view.
pub fn merge_pushed_document(
    bootstrap: &Config,
    raw: &str,
    format_hint: Option<super::desired_format::DesiredFormat>,
) -> anyhow::Result<Config> {
    let desired = parse_desired_document_with_hint(raw, format_hint)?;
    ensure_desired_only(&desired)?;
    let mut merged = merge_bootstrap_over_desired(bootstrap, desired);
    apply_env_overrides(&mut merged);
    Ok(merged)
}

/// Metadata about the currently effective applied revision, if any.
#[derive(Debug, Clone)]
pub struct EffectiveRevision {
    pub id: i64,
    pub content_sha256: String,
    pub revision_label: Option<String>,
    pub applied_at: String,
}

impl From<ConfigRevisionRecord> for EffectiveRevision {
    fn from(record: ConfigRevisionRecord) -> Self {
        Self {
            id: record.id,
            content_sha256: record.content_sha256,
            revision_label: record.revision_label,
            applied_at: record.applied_at,
        }
    }
}

/// Load effective revision metadata when booting from the ledger.
///
/// Single-document instances only: multi-org state has one revision per
/// organization stream, not one global identity.
pub fn effective_revision(store: &Store) -> anyhow::Result<Option<EffectiveRevision>> {
    Ok(store
        .latest_applied_config_revision(None)?
        .map(EffectiveRevision::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `[advisories]` enables outbound third-party lookups, so it must not be
    /// reachable through a pushed desired-state document.
    #[test]
    fn pushed_document_with_advisories_should_fail_desired_only() {
        let config: Config = toml::from_str("[advisories]\nenabled = true\n").expect("parse");
        assert!(contains_infra_sections(&config));
        assert!(ensure_desired_only(&config).is_err());
    }

    #[test]
    fn strip_infra_sections_should_clear_advisories() {
        // Must mirror `contains_infra_sections`: a stripped document is fed back
        // through `ensure_desired_only`, so anything counted as infra there and
        // left behind here would reject a legitimate document.
        let mut config: Config = toml::from_str("[advisories]\nenabled = true\n").expect("parse");
        strip_infra_sections(&mut config);
        assert!(!contains_infra_sections(&config));
        assert!(ensure_desired_only(&config).is_ok());
    }

    #[test]
    fn merge_bootstrap_over_desired_should_carry_advisories() {
        // Regression: omitting this silently discarded bootstrap enrichment
        // settings the moment any desired document was applied.
        let bootstrap: Config =
            toml::from_str("[advisories]\nenabled = true\nendpoint = \"https://osv.example\"\n")
                .expect("parse bootstrap");
        let desired = Config::default();
        let merged = merge_bootstrap_over_desired(&bootstrap, desired);
        assert!(merged.advisories.enabled);
        assert_eq!(merged.advisories.endpoint, "https://osv.example");
    }

    #[test]
    fn pushed_document_with_database_should_fail_desired_only() {
        let bootstrap: Config = toml::from_str(
            r#"
 [database]
 postgres_url = "postgres://bootstrap/db"
 [api]
 listen = "127.0.0.1:8080"
 "#,
        )
        .expect("bootstrap");

        let err = merge_pushed_document(
            &bootstrap,
            r#"
 [database]
 postgres_url = "postgres://evil/db"
 [[sources]]
 type = "github"
 repo = "org/app"
 "#,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("infrastructure sections"));
    }

    #[test]
    fn merge_should_keep_bootstrap_database_and_api() {
        let mut bootstrap: Config = toml::from_str(
            r#"
 [database]
 postgres_url = "postgres://bootstrap/db"

 [api]
 listen = "127.0.0.1:9090"

 [config_api]
 api_config = true
 source = "api"
 "#,
        )
        .expect("bootstrap");
        bootstrap.log.level = "debug".into();

        let desired: Config = toml::from_str(
            r#"
 [[notifiers]]
 type = "apprise"
 urls = ["mailto://a@b.c"]

 [[sources]]
 type = "github"
 repo = "org/app"
 "#,
        )
        .expect("desired");

        let merged = merge_bootstrap_over_desired(&bootstrap, desired);
        assert_eq!(merged.database.postgres_url, "postgres://bootstrap/db");
        assert_eq!(merged.api.listen, "127.0.0.1:9090");
        assert!(merged.config_api.api_config);
        assert_eq!(merged.log.level, "debug");
        assert_eq!(merged.sources.len(), 1);
    }

    #[test]
    fn bootstrap_with_sources_should_fail_infra_only_check() {
        let base = std::env::temp_dir().join(format!("xrelease-infra-only-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("mkdir");
        let bootstrap_path = base.join("bootstrap.toml");
        std::fs::write(
            &bootstrap_path,
            r#"
 [database]
 postgres_url = "postgres://local/db"
 [[sources]]
 type = "github"
 repo = "org/forbidden"
 "#,
        )
        .expect("bootstrap");

        let err = load_bootstrap(&bootstrap_path).unwrap_err();
        assert!(err.to_string().contains("infrastructure sections"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn app_file_with_infra_sections_should_fail_desired_only() {
        let base = std::env::temp_dir().join(format!("xrelease-app-infra-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("mkdir");
        let bootstrap_path = base.join("bootstrap.toml");
        std::fs::write(
            &bootstrap_path,
            r#"
 [database]
 postgres_url = "postgres://local/db"
 "#,
        )
        .expect("bootstrap");

        let app_path = base.join("releases.yaml");
        std::fs::write(
            &app_path,
            r#"
database:
  postgres_url: postgres://evil/db
sources:
  - type: github
    repo: org/app
"#,
        )
        .expect("app");

        let paths = ConfigPaths::new(bootstrap_path, Some(app_path));
        let err = resolve(&paths, None).unwrap_err();
        assert!(err.to_string().contains("desired-state document"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn split_resolve_should_load_yaml_app_file() {
        let base = std::env::temp_dir().join(format!("xrelease-split-test-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("mkdir");
        let bootstrap_path = base.join("bootstrap.toml");
        std::fs::write(
            &bootstrap_path,
            r#"
 [database]
 postgres_url = "postgres://local/db"
 [api]
 listen = "127.0.0.1:8080"
 "#,
        )
        .expect("bootstrap");

        let app_dir = base.join("app");
        std::fs::create_dir_all(&app_dir).expect("app dir");
        let app_path = app_dir.join("releases.yaml");
        std::fs::write(
            &app_path,
            r#"
sources:
  - type: github
    repo: org/from-yaml
"#,
        )
        .expect("app");

        let paths = ConfigPaths::new(bootstrap_path, Some(app_path));
        let effective = resolve(&paths, None).expect("resolve");
        assert_eq!(effective.sources.len(), 1);
        let watches = effective.into_watches().expect("watches");
        assert_eq!(watches[0].provider.id(), "github:org/from-yaml");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_without_app_or_ledger_should_fail() {
        let base = std::env::temp_dir().join(format!("xrelease-no-app-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("mkdir");
        let bootstrap_path = base.join("bootstrap.toml");
        std::fs::write(
            &bootstrap_path,
            r#"
 [database]
 postgres_url = "postgres://local/db"
 "#,
        )
        .expect("bootstrap");

        let paths = ConfigPaths::new(bootstrap_path, None);
        assert!(resolve(&paths, None).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_organizations_should_merge_and_namespace() {
        let base = std::env::temp_dir().join(format!("xrelease-multi-org-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("app/platform")).expect("platform dir");
        std::fs::create_dir_all(base.join("app/security")).expect("security dir");

        std::fs::write(
            base.join("bootstrap.toml"),
            r#"
 [database]
 postgres_url = "postgres://local/db"

 [[organizations]]
 id = "platform"
 name = "Platform"
 app = "app/platform/releases.yaml"

 [[organizations]]
 id = "security"
 app = "app/security/releases.yaml"
 "#,
        )
        .expect("bootstrap");

        std::fs::write(
            base.join("app/platform/releases.yaml"),
            r#"
teams:
  - tag: core
sources:
  - type: github
    repo: org/platform-app
    routing_tag: core
"#,
        )
        .expect("platform");

        std::fs::write(
            base.join("app/security/releases.yaml"),
            r#"
notifiers:
  - type: apprise
    endpoint: http://127.0.0.1:9
    urls: ["mailto://a@b.c"]
  - type: apprise
    urls: ["mailto://platform@example.com"]
  - type: webhook
    url: https://example.test/hook
    tags: [ops]
sources:
  - type: github
    repo: org/security-app
    routing_tag: ops
"#,
        )
        .expect("security");

        let paths = ConfigPaths::new(base.join("bootstrap.toml"), None);
        let effective = resolve(&paths, None).expect("resolve orgs");
        assert_eq!(effective.organizations.len(), 2);
        assert_eq!(effective.sources.len(), 2);
        assert_eq!(effective.teams.len(), 1); // security has no teams block
        let watches = effective.into_watches().expect("watches");
        let ids: Vec<_> = watches.iter().map(|w| w.provider.id().to_owned()).collect();
        assert!(ids
            .iter()
            .any(|id| id == "platform::github:org/platform-app"));
        assert!(ids
            .iter()
            .any(|id| id == "security::github:org/security-app"));
        assert_eq!(
            watches
                .iter()
                .find(|w| w.provider.id().starts_with("platform::"))
                .and_then(|w| w.organization_id.as_deref()),
            Some("platform")
        );
        assert_eq!(
            watches
                .iter()
                .find(|w| w.provider.id().starts_with("platform::"))
                .and_then(|w| w.routing_tag.as_deref()),
            Some("platform::core")
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Two-org fixture on disk with caller-supplied desired documents.
    fn two_org_fixture(
        tag: &str,
        platform_yaml: &str,
        security_yaml: &str,
    ) -> (std::path::PathBuf, ConfigPaths) {
        let base =
            std::env::temp_dir().join(format!("xrelease-compose-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("app/platform")).expect("platform dir");
        std::fs::create_dir_all(base.join("app/security")).expect("security dir");

        std::fs::write(
            base.join("bootstrap.toml"),
            r#"
 [database]
 postgres_url = "postgres://local/db"

 [[organizations]]
 id = "platform"
 app = "app/platform/releases.yaml"

 [[organizations]]
 id = "security"
 app = "app/security/releases.yaml"
 "#,
        )
        .expect("bootstrap");
        std::fs::write(base.join("app/platform/releases.yaml"), platform_yaml).expect("platform");
        std::fs::write(base.join("app/security/releases.yaml"), security_yaml).expect("security");

        let paths = ConfigPaths::new(base.join("bootstrap.toml"), None);
        (base, paths)
    }

    /// Two-org fixture on disk for the compose-override tests.
    fn org_fixture(tag: &str) -> (std::path::PathBuf, ConfigPaths) {
        two_org_fixture(
 tag,
 "notifiers:\n  - type: apprise\n    urls: [\"mailto://p@example.com\"]\nsources:\n  - type: github\n    repo: org/platform-app\n",
 "notifiers:\n  - type: apprise\n    urls: [\"mailto://s@example.com\"]\nsources:\n  - type: github\n    repo: org/security-app\n")
    }

    #[test]
    fn cross_org_same_named_presets_should_stay_org_scoped() {
        // `team-pattern` is deliberately NOT a built-in: resolution can only
        // succeed through each org's own (namespaced) entry, so a passing
        // `into_watches` proves the references were rewritten per-org.
        let (base, paths) = two_org_fixture(
 "presets",
 "notifiers:\n  - type: apprise\n    urls: [\"mailto://p@example.com\"]\npresets:\n  team-pattern:\n    pattern: \"^platform-\"\nsources:\n  - type: github\n    repo: org/platform-app\n    preset: team-pattern\n",
 "notifiers:\n  - type: apprise\n    urls: [\"mailto://s@example.com\"]\npresets:\n  team-pattern:\n    pattern: \"^security-\"\nsources:\n  - type: github\n    repo: org/security-app\n    preset: team-pattern\n");
        let bootstrap = load_infra_bootstrap(&paths).expect("bootstrap");

        let composed = compose_organizations(&bootstrap, &paths, None, None).expect("compose");

        // Both entries survive under org-scoped keys (no later-org-wins merge),
        // and every source resolves against its OWN org's entry.
        assert!(composed
            .merged
            .presets
            .contains_key("platform::team-pattern"));
        assert!(composed
            .merged
            .presets
            .contains_key("security::team-pattern"));
        assert_eq!(
            composed.merged.into_watches().expect("watches").len(),
            2,
            "both org sources must resolve their own preset"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn preset_reference_should_not_resolve_across_organizations() {
        // Only platform declares `team-pattern` (not a built-in); security
        // references it. Standalone, security's document is invalid — the
        // merge must fail the same way instead of quietly lending it
        // platform's definition. (A built-in name would resolve to the shared
        // built-in for security, which is likewise standalone semantics.)
        let (base, paths) = two_org_fixture(
 "preset-x",
 "notifiers:\n  - type: apprise\n    urls: [\"mailto://p@example.com\"]\npresets:\n  team-pattern:\n    pattern: \"^platform-\"\nsources:\n  - type: github\n    repo: org/platform-app\n    preset: team-pattern\n",
 "notifiers:\n  - type: apprise\n    urls: [\"mailto://s@example.com\"]\nsources:\n  - type: github\n    repo: org/security-app\n    preset: team-pattern\n");
        let bootstrap = load_infra_bootstrap(&paths).expect("bootstrap");

        let composed = compose_organizations(&bootstrap, &paths, None, None).expect("compose");
        let err = composed
            .merged
            .into_watches()
            .expect_err("cross-org preset reference must not resolve");
        assert!(err.to_string().contains("unknown preset"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn multi_org_upstream_cap_should_take_the_strictest_value() {
        let (base, paths) = two_org_fixture(
 "rpm",
 "defaults:\n  upstream_requests_per_minute: 120\nnotifiers:\n  - type: apprise\n    urls: [\"mailto://p@example.com\"]\nsources:\n  - type: github\n    repo: org/platform-app\n",
 "defaults:\n  upstream_requests_per_minute: 30\nnotifiers:\n  - type: apprise\n    urls: [\"mailto://s@example.com\"]\nsources:\n  - type: github\n    repo: org/security-app\n");
        let bootstrap = load_infra_bootstrap(&paths).expect("bootstrap");

        let composed = compose_organizations(&bootstrap, &paths, None, None).expect("compose");

        assert_eq!(composed.merged.defaults.upstream_requests_per_minute, 30);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn unlimited_first_org_should_not_erase_a_later_org_cap() {
        // First org leaves rpm at 0 (unlimited); its defaults seed `merged`,
        // but the second org's explicit cap must still take effect.
        let (base, paths) = two_org_fixture(
 "rpm-zero",
 "notifiers:\n  - type: apprise\n    urls: [\"mailto://p@example.com\"]\nsources:\n  - type: github\n    repo: org/platform-app\n",
 "defaults:\n  upstream_requests_per_minute: 60\nnotifiers:\n  - type: apprise\n    urls: [\"mailto://s@example.com\"]\nsources:\n  - type: github\n    repo: org/security-app\n");
        let bootstrap = load_infra_bootstrap(&paths).expect("bootstrap");

        let composed = compose_organizations(&bootstrap, &paths, None, None).expect("compose");

        assert_eq!(composed.merged.defaults.upstream_requests_per_minute, 60);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn compose_override_should_substitute_only_the_addressed_org() {
        let (base, paths) = org_fixture("substitute");
        let bootstrap = load_infra_bootstrap(&paths).expect("bootstrap");

        let candidate =
 "notifiers:\n  - type: apprise\n    urls: [\"mailto://p@example.com\"]\nsources:\n  - type: github\n    repo: org/replaced\n";
        let composed =
            compose_organizations(&bootstrap, &paths, None, Some(("platform", candidate)))
                .expect("compose with override");

        let watches = composed.merged.into_watches().expect("watches");
        let ids: Vec<_> = watches.iter().map(|w| w.provider.id().to_owned()).collect();
        assert!(ids.iter().any(|id| id == "platform::github:org/replaced"));
        assert!(!ids
            .iter()
            .any(|id| id == "platform::github:org/platform-app"));
        // The other org's file remains the authority.
        assert!(ids
            .iter()
            .any(|id| id == "security::github:org/security-app"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn compose_override_should_reject_an_unknown_org() {
        let (base, paths) = org_fixture("unknown");
        let bootstrap = load_infra_bootstrap(&paths).expect("bootstrap");

        let err = compose_organizations(&bootstrap, &paths, None, Some(("nope", "sources: []")))
            .expect_err("unknown org must fail, not silently compose current state");
        assert!(err.to_string().contains("unknown organization"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn compose_identity_should_change_only_when_a_document_changes() {
        let (base, paths) = org_fixture("identity");
        let bootstrap = load_infra_bootstrap(&paths).expect("bootstrap");

        let first = compose_organizations(&bootstrap, &paths, None, None).expect("compose");
        let second = compose_organizations(&bootstrap, &paths, None, None).expect("compose");
        assert_eq!(first.identity_sha256, second.identity_sha256);

        std::fs::write(
 base.join("app/security/releases.yaml"),
 "notifiers:\n  - type: apprise\n    urls: [\"mailto://s@example.com\"]\nsources:\n  - type: github\n    repo: org/security-v2\n")
.expect("rewrite security");
        let third = compose_organizations(&bootstrap, &paths, None, None).expect("compose");
        assert_ne!(first.identity_sha256, third.identity_sha256);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn api_org_without_app_should_boot_empty() {
        let base =
            std::env::temp_dir().join(format!("xrelease-api-org-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("dir");
        std::fs::write(
            base.join("bootstrap.toml"),
            r#"
 [database]
 postgres_url = "postgres://local/db"

 [config_api]
 api_config = true
 source = "api"
 ui_config = true

 [[organizations]]
 id = "platform"
 name = "Platform"
 "#,
        )
        .expect("bootstrap");

        let paths = ConfigPaths::new(base.join("bootstrap.toml"), None);
        let bootstrap = load_infra_bootstrap(&paths).expect("bootstrap");
        assert!(bootstrap.organizations[0].app_path().is_none());

        let composed =
            compose_organizations(&bootstrap, &paths, None, None).expect("empty API org boots");
        assert_eq!(composed.sources.len(), 1);
        assert_eq!(composed.sources[0].1, DesiredSource::Empty);
        assert!(composed.merged.sources.is_empty());

        let (raw, source) =
            organization_desired_raw(&bootstrap, &paths, None, &bootstrap.organizations[0])
                .expect("raw");
        assert_eq!(source, DesiredSource::Empty);
        assert_eq!(raw, EMPTY_ORGANIZATION_DOCUMENT);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn local_org_without_app_should_fail() {
        let base =
            std::env::temp_dir().join(format!("xrelease-local-org-no-app-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("dir");
        std::fs::write(
            base.join("bootstrap.toml"),
            r#"
 [database]
 postgres_url = "postgres://local/db"

 [config_api]
 source = "local"

 [[organizations]]
 id = "platform"
 "#,
        )
        .expect("bootstrap");

        let paths = ConfigPaths::new(base.join("bootstrap.toml"), None);
        let err = resolve(&paths, None).expect_err("local requires app");
        assert!(
            err.to_string().contains("no `app` path"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
