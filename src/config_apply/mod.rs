//! Push-applied configuration — unified apply core for apply, rollback, and boot.
//!
//! All config mutations flow through [`apply_document`]: parse → merge bootstrap →
//! restore redacted → normalize secrets → validate → upsert/`app_secret` prune →
//! hot-swap → ledger append.
//!
//! **Ordering rationale:** secrets are sealed before hot-swap so runtime resolve
//! sees vault values immediately; hot-swap runs before the ledger append so a
//! failed append can restore the previous runtime snapshot (compensate). A crash
//! between swap and append leaves runtime ahead of the ledger; the next boot
//! reloads from the ledger (or app file) and converges. Concurrent applies are
//! serialized by the HTTP layer (`AppState::apply_lock`).

mod diff;
mod redact;

pub use diff::ConfigDiff;
pub use redact::{
    redact_config_toml, redact_desired_document, redact_desired_document_with_format,
    redact_desired_only_document, restore_redacted_secrets,
};

use anyhow::Context;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::config::{
    collect_secret_env_refs, compose_organizations, ensure_desired_only, has_desired_state,
    merge_pushed_document, normalize_document_for_storage, normalize_secrets_to_refs,
    organization_desired_raw, parse_desired_document, parse_desired_document_with_hint,
    stale_ui_managed_secrets, Config, ConfigPaths, SecretWrite, EMPTY_ORGANIZATION_DOCUMENT,
};
use crate::store::{ConfigRevisionInsert, ConfigRevisionStatus, Store};
use crate::validate::{validate_full, ValidateOptions, ValidationReport};

/// Which desired-state stream an apply / rollback addresses.
#[derive(Debug, Clone, Copy)]
pub enum ApplyScope<'a> {
    /// Legacy single-document instance: the pushed document IS the whole
    /// desired state, recorded in the `organization_id IS NULL` ledger stream.
    Whole,
    /// Multi-org instance: the pushed document replaces one organization's
    /// desired state; the runtime is recomposed from every organization's
    /// current authority (its ledger stream or its `[[organizations]]` file)
    /// with this candidate substituted in.
    Organization {
        /// Catalogue slug (`[[organizations]].id`).
        organization: &'a str,
        /// Bootstrap/app paths for resolving the other orgs' files.
        paths: &'a ConfigPaths,
    },
}

impl ApplyScope<'_> {
    /// The ledger stream this scope reads and writes.
    #[must_use]
    pub fn ledger_stream(&self) -> Option<&str> {
        match self {
            Self::Whole => None,
            Self::Organization { organization, .. } => Some(organization),
        }
    }
}

/// Validation failure — maps to HTTP 422.
///
/// `revision` is the ledger row recording the rejected attempt; it is `None`
/// for file-mode reloads, which never touch the ledger.
#[derive(Debug, Clone)]
pub struct ValidationRejected {
    pub revision: Option<i64>,
    pub report: ValidationReport,
}

impl std::fmt::Display for ValidationRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.revision {
            Some(revision) => write!(f, "config validation failed (revision {revision})"),
            None => write!(f, "config validation failed"),
        }
    }
}

impl std::error::Error for ValidationRejected {}

/// The submitted document could not be turned into a config at all — it does
/// not parse as TOML or YAML, or it carries infrastructure sections the
/// desired-state layer forbids (the bootstrap/desired boundary).
///
/// Distinct from [`ValidationRejected`]: that one means "this parsed into a
/// config, but the config is semantically wrong" (422, with a report). This
/// one means the request body was never a usable document (400). Both are the
/// **client's** fault — neither is a server error, which is what they used to
/// surface as, since a bare `anyhow` maps to 500.
#[derive(Debug, Clone)]
pub struct DocumentRejected(pub String);

impl std::fmt::Display for DocumentRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DocumentRejected {}

/// `POST /config/rollback` with no *earlier* applied revision than the
/// current one — a state conflict (409), not a server error.
#[derive(Debug, Clone, Copy)]
pub struct NothingToRollBackTo;

impl std::fmt::Display for NothingToRollBackTo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no earlier applied revision to roll back to")
    }
}

impl std::error::Error for NothingToRollBackTo {}

/// Parse + merge a pushed document, tagging failures as client errors.
fn merge_or_reject(
    bootstrap: &Config,
    raw: &str,
    format_hint: Option<crate::config::DesiredFormat>,
) -> anyhow::Result<Config> {
    merge_pushed_document(bootstrap, raw, format_hint)
        .map_err(|err| anyhow::Error::new(DocumentRejected(err.to_string())))
}

/// Provenance metadata carried on every apply attempt.
#[derive(Debug, Clone, Default)]
pub struct ApplyOrigin {
    pub revision_label: Option<String>,
    pub applied_by: Option<String>,
    pub source_addr: Option<String>,
}

/// Outcome of a successful or idempotent apply.
#[derive(Debug, Clone, Serialize)]
pub struct ApplyResponse {
    pub applied: bool,
    pub content_sha256: String,
    pub revision: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources_added: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources_removed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources_changed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Result of validation-only dry-run (`POST /api/v1/config/validate`).
#[derive(Debug, Clone, Serialize)]
pub struct ValidateResponse {
    pub valid: bool,
    pub content_sha256: String,
    pub report: ValidationReport,
}

/// Compute the SHA-256 hex digest of raw config bytes.
#[must_use]
pub fn content_sha256(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    hex::encode(digest)
}

/// The subset of `writes` that would actually change a sealed secret.
///
/// Every apply normalizes inline secrets to `*_env` refs, so an unchanged
/// document yields a full set of `writes` on each call even though none of the
/// values moved. Only those differing from the currently-resolved value are real
/// rotations. An unresolvable name counts as changed, so the comparison can only
/// ever cause an extra reseal — never skip a genuine rotation.
fn effective_secret_rotations(writes: &[SecretWrite]) -> Vec<&SecretWrite> {
    writes
        .iter()
        .filter(|write| {
            crate::config::vault_get(&write.name).as_deref() != Some(write.value.as_str())
        })
        .collect()
}

/// A scope-resolved candidate ready to validate and hot-swap.
struct ScopedCandidate {
    /// Desired-only body persisted in the ledger (secrets normalized to refs).
    ledger_raw: String,
    /// `content_sha256(ledger_raw)` — the idempotency key for this scope.
    ledger_sha: String,
    /// Full runtime view (bootstrap + composition) to validate and swap.
    merged: Config,
    /// Secrets to seal into `app_secret` before hot-swap.
    secret_writes: Vec<SecretWrite>,
    /// `*_env` names referenced by this scope's desired document (orphan GC).
    secret_env_refs: std::collections::HashSet<String>,
}

/// Parse + guard an organization candidate and restore redacted secrets from
/// the org's OWN authority document, BEFORE composition. Returns the ledger
/// body and its sha.
///
/// Restore must not run on the composed runtime: the composition contains
/// every organization, so serializing it back for the ledger would record
/// other tenants' sources — and their restored secrets — into this org's
/// stream (cross-tenant leak), and the next recompose would namespace those
/// already-namespaced entries again (`platform::security::github:…`),
/// duplicating watches and forking seen-release state. Org-local restore also
/// keeps a placeholder from ever matching another organization's notifier.
fn restore_org_candidate(
    bootstrap: &Config,
    paths: &ConfigPaths,
    store: &Store,
    organization: &str,
    raw: &str,
    format_hint: Option<crate::config::DesiredFormat>,
) -> anyhow::Result<(Config, String)> {
    // Same candidate guards an org file gets at boot; all client-side faults.
    let mut candidate = parse_desired_document_with_hint(raw, format_hint)
        .map_err(|err| anyhow::Error::new(DocumentRejected(err.to_string())))?;
    ensure_desired_only(&candidate)
        .map_err(|err| anyhow::Error::new(DocumentRejected(err.to_string())))?;
    if !has_desired_state(&candidate) {
        return Err(anyhow::Error::new(DocumentRejected(
            "document defines no desired-state sections (sources / notifiers)".into(),
        )));
    }

    let org = bootstrap
        .organizations
        .iter()
        .find(|org| org.id == organization)
        .ok_or_else(|| {
            anyhow::anyhow!("unknown organization `{organization}` (not in [[organizations]])")
        })?;
    // The org's current authority (ledger stream or file) carries the real
    // secrets the UI redacted on GET — the org-local counterpart of restoring
    // from the live runtime config in whole-document mode.
    let (previous_raw, _) = organization_desired_raw(bootstrap, paths, Some(store), org)?;
    if let Ok(previous) = parse_desired_document(&previous_raw) {
        let _ = restore_redacted_secrets(&previous, &mut candidate);
    }
    let format = format_hint.unwrap_or_else(|| crate::config::detect_desired_format(raw));
    let body = redact::serialize_desired_only_strict(format, &candidate)?;
    Ok((candidate, body))
}

/// Resolve a candidate document into the ledger body + merged runtime view for
/// its scope.
///
/// Whole-instance: bootstrap + the document, then secrets restored from
/// `previous` (the live runtime config). Organization: org-local restore first
/// (see [`restore_org_candidate`]), then every org's current authority with
/// the restored candidate substituted — composed by the same
/// [`compose_organizations`] path boot and reload use, so validation sees
/// exactly what would run (including cross-org id collisions).
///
/// Inline secrets are normalized to `*_env` refs + [`SecretWrite`]s so the
/// ledger never stores plaintext credentials.
///
/// Failures of the CANDIDATE (parse, infra sections, empty desired state) are
/// tagged [`DocumentRejected`] → 400. Failures loading OTHER orgs' authority
/// stay bare `anyhow` → 500: they are the server's state, not the client's.
fn resolve_scoped_candidate(
    bootstrap: &Config,
    raw: &str,
    format_hint: Option<crate::config::DesiredFormat>,
    store: &Store,
    scope: &ApplyScope<'_>,
    previous: Option<&Config>,
) -> anyhow::Result<ScopedCandidate> {
    let organization = scope.ledger_stream().map(str::to_owned);
    let format = format_hint.unwrap_or_else(|| crate::config::detect_desired_format(raw));

    match scope {
        ApplyScope::Whole => {
            let mut merged = merge_or_reject(bootstrap, raw, format_hint)?;
            // UI drops redacted secrets on Apply — restore from the live
            // config so a BotX access_token (etc.) is not wiped by an
            // unrelated edit.
            let _ = previous
                .map(|previous| restore_redacted_secrets(previous, &mut merged))
                .unwrap_or(false);

            let mut desired = desired_state_for_ledger(&merged);
            let secret_writes = normalize_secrets_to_refs(&mut desired, organization.as_deref());
            // Keep runtime view in sync with normalized refs (vault upsert is later).
            merged.defaults = desired.defaults.clone();
            merged.notifiers = desired.notifiers.clone();
            merged.teams = desired.teams.clone();
            merged.presets = desired.presets.clone();
            merged.sources = desired.sources.clone();

            let ledger_raw = redact::serialize_desired_only_strict(format, &desired)?;
            let ledger_sha = content_sha256(&ledger_raw);
            let secret_env_refs = collect_secret_env_refs(&desired);
            Ok(ScopedCandidate {
                ledger_raw,
                ledger_sha,
                merged,
                secret_writes,
                secret_env_refs,
            })
        }
        ApplyScope::Organization {
            organization: org_id,
            paths,
        } => {
            let (mut org_desired, _) =
                restore_org_candidate(bootstrap, paths, store, org_id, raw, format_hint)?;
            let secret_writes = normalize_secrets_to_refs(&mut org_desired, Some(org_id));
            let ledger_raw = redact::serialize_desired_only_strict(format, &org_desired)?;
            let ledger_sha = content_sha256(&ledger_raw);
            let secret_env_refs = collect_secret_env_refs(&org_desired);
            let composed =
                compose_organizations(bootstrap, paths, Some(store), Some((org_id, &ledger_raw)))?;
            Ok(ScopedCandidate {
                ledger_raw,
                ledger_sha,
                merged: composed.merged,
                secret_writes,
                secret_env_refs,
            })
        }
    }
}

/// Dry-run a candidate desired-state document without persisting or hot-swapping.
///
/// Redacted secrets omitted by the UI (e.g. Apprise `urls`) are restored
/// before validation — same as [`apply_document`]: whole-document mode from
/// `previous` (the live runtime config), organization mode from the org's own
/// authority document. Without that, Validate falsely reports routing orphans
/// while Apply would succeed after restore.
pub fn validate_document(
    bootstrap: &Config,
    raw: &str,
    strict: bool,
    format_hint: Option<crate::config::DesiredFormat>,
    store: &Store,
    scope: &ApplyScope<'_>,
    previous: Option<&Config>,
) -> anyhow::Result<ValidateResponse> {
    let candidate = resolve_scoped_candidate(bootstrap, raw, format_hint, store, scope, previous)?;
    let mut report = validate_full(&candidate.merged, &ValidateOptions::default());
    if strict {
        report.apply_strict(true);
    }
    Ok(ValidateResponse {
        valid: report.valid,
        content_sha256: content_sha256(raw),
        report,
    })
}

/// Apply a desired-state document: validate, hot-swap, append to the ledger.
///
/// Idempotency is checked against the latest applied revision **in this
/// scope's ledger stream**, twice: on the client raw's sha (exact repush) and
/// on the sha of the ledger body the apply would record after secret restore
/// (a GET→apply round-trip of unchanged content). Both return `applied =
/// false` with the current revision metadata.
pub async fn apply_document(
    bootstrap: &Config,
    raw: &str,
    format_hint: Option<crate::config::DesiredFormat>,
    origin: &ApplyOrigin,
    store: &Store,
    swap: &impl ConfigSwapper,
    scope: &ApplyScope<'_>,
) -> anyhow::Result<ApplyResponse> {
    let sha = content_sha256(raw);
    let stream = scope.ledger_stream();
    let current = store.latest_applied_config_revision(stream)?;

    if let Some(current) = &current {
        if current.content_sha256 == sha {
            return Ok(ApplyResponse {
                applied: false,
                content_sha256: sha,
                revision: current.id,
                sources_added: Vec::new(),
                sources_removed: Vec::new(),
                sources_changed: Vec::new(),
                warnings: Vec::new(),
            });
        }
    }

    let previous = swap.current_config().await;
    let candidate = match resolve_scoped_candidate(
        bootstrap,
        raw,
        format_hint,
        store,
        scope,
        Some(&previous),
    ) {
        Ok(candidate) => candidate,
        Err(err) => {
            // Only the candidate's own faults are auditable rejections; a
            // server-side compose failure (another org's file vanished) is
            // not this document's fault and must surface as the 500 it is.
            if err.downcast_ref::<DocumentRejected>().is_none() {
                return Err(err);
            }
            let (reject_body, reject_sha) = rejected_ledger_body(raw, stream);
            let revision = record_attempt(
                store,
                &reject_body,
                &reject_sha,
                origin,
                ConfigRevisionStatus::Rejected,
                Some(&err.to_string()),
                stream,
            )?;
            // Keep the typed `DocumentRejected` at the head of the chain so
            // the HTTP layer can still map it to 400; `with_context` would
            // bury it behind a plain string and it would fall back to 500.
            return Err(anyhow::Error::new(DocumentRejected(format!(
                "{err} (revision {revision} recorded as rejected)"
            ))));
        }
    };

    // Round-trip no-op: after a GET, the client raw differs from the stored
    // revision only by redacted secrets — compare the ledger body this apply
    // would record, so an unchanged UI save does not append a twin revision
    // and restart every poll loop for nothing.
    //
    // Exception: rotating an inline secret keeps the same refs-only ledger body
    // (same `*_env` names) but MUST still upsert `app_secret`. Skipping that
    // silently drops credential rotations.
    if let Some(current) = &current {
        if current.content_sha256 == candidate.ledger_sha {
            // …but only a write whose value actually *differs* from what is
            // already sealed is a rotation. Re-sending the same inline secret —
            // what a UI save or a CI re-apply of unchanged config does on every
            // call, since an inline value is normalized to a ref on each apply —
            // is not, and must still report `applied = false`. Comparing against
            // the process vault fails safe: a name that is not loaded looks
            // changed, so a real rotation is never mistaken for a no-op.
            if effective_secret_rotations(&candidate.secret_writes).is_empty() {
                return Ok(ApplyResponse {
                    applied: false,
                    content_sha256: candidate.ledger_sha,
                    revision: current.id,
                    sources_added: Vec::new(),
                    sources_removed: Vec::new(),
                    sources_changed: Vec::new(),
                    warnings: Vec::new(),
                });
            }

            let report = validate_full(&candidate.merged, &ValidateOptions::default());
            if !report.valid {
                let summary = report
                    .errors
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "validation failed".into());
                let revision = record_attempt(
                    store,
                    &candidate.ledger_raw,
                    &candidate.ledger_sha,
                    origin,
                    ConfigRevisionStatus::Rejected,
                    Some(&summary),
                    stream,
                )?;
                return Err(anyhow::Error::new(ValidationRejected {
                    revision: Some(revision),
                    report,
                }));
            }

            store
                .upsert_app_secrets(&candidate.secret_writes)
                .context("persisting app secrets")?;
            prune_stale_app_secrets(
                bootstrap,
                store,
                scope,
                &previous,
                &candidate.secret_env_refs,
            )?;
            // Shape unchanged — vault_upsert already refreshed process memory;
            // skip hot-swap / ledger append to avoid pointless poll restarts.
            return Ok(ApplyResponse {
                applied: true,
                content_sha256: candidate.ledger_sha,
                revision: current.id,
                sources_added: Vec::new(),
                sources_removed: Vec::new(),
                sources_changed: Vec::new(),
                warnings: report.warnings,
            });
        }
    }

    let report = validate_full(&candidate.merged, &ValidateOptions::default());
    if !report.valid {
        let summary = report
            .errors
            .first()
            .cloned()
            .unwrap_or_else(|| "validation failed".into());
        let revision = record_attempt(
            store,
            &candidate.ledger_raw,
            &candidate.ledger_sha,
            origin,
            ConfigRevisionStatus::Rejected,
            Some(&summary),
            stream,
        )?;
        return Err(anyhow::Error::new(ValidationRejected {
            revision: Some(revision),
            report,
        }));
    }

    // Snapshot before swap so a ledger failure can restore in-memory state —
    // and so the diff compares against the real previous config (truthful
    // `changed`), not just the id set.
    let diff = ConfigDiff::compute(&previous, &candidate.merged);
    store
        .upsert_app_secrets(&candidate.secret_writes)
        .context("persisting app secrets")?;
    prune_stale_app_secrets(
        bootstrap,
        store,
        scope,
        &previous,
        &candidate.secret_env_refs,
    )?;
    swap.apply_runtime(&candidate.merged).await?;

    let revision = match record_attempt(
        store,
        &candidate.ledger_raw,
        &candidate.ledger_sha,
        origin,
        ConfigRevisionStatus::Applied,
        None,
        stream,
    ) {
        Ok(id) => id,
        Err(err) => {
            tracing::error!(
            error = %err,
            "ledger append failed after hot-swap; restoring previous runtime config"
            );
            if let Err(restore_err) = swap.apply_runtime(&previous).await {
                tracing::error!(
                    error = %restore_err,
                    "CRITICAL: failed to restore previous config after ledger failure — \
                     runtime is ahead of the ledger; restart this process (or re-apply the \
                     last known-good document) before accepting further applies"
                );
            }
            return Err(err);
        }
    };

    Ok(ApplyResponse {
        applied: true,
        content_sha256: candidate.ledger_sha,
        revision,
        sources_added: diff.added,
        sources_removed: diff.removed,
        sources_changed: diff.changed,
        warnings: report.warnings,
    })
}

/// Outcome of a file-mode reload (`POST /api/v1/reload`, `SIGHUP`).
#[derive(Debug, Clone, Serialize)]
pub struct ReloadResponse {
    pub applied: bool,
    pub content_sha256: String,
    /// App config file the desired state was read from.
    pub source_path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources_added: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources_removed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources_changed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Re-read desired state from the app config file and hot-swap it.
///
/// Deliberately **does not touch the ledger**: [`crate::config::resolve`]
/// prefers a ledger revision over the app file, so recording a reload would
/// silently promote the ledger to boot authority and make later edits to the
/// file stop taking effect. Whichever source booted the process stays the
/// source it boots from — reload only refreshes what is running.
///
/// `current_sha` short-circuits an unchanged file: re-swapping identical
/// config would restart every poll loop for nothing, which matters when CD
/// calls this on a schedule.
pub async fn reload_document(
    bootstrap: &Config,
    current_sha: Option<&str>,
    source_path: &str,
    raw: &str,
    format_hint: Option<crate::config::DesiredFormat>,
    swap: &impl ConfigSwapper,
) -> anyhow::Result<ReloadResponse> {
    let sha = content_sha256(raw);
    let unchanged = ReloadResponse {
        applied: false,
        content_sha256: sha.clone(),
        source_path: source_path.to_owned(),
        sources_added: Vec::new(),
        sources_removed: Vec::new(),
        sources_changed: Vec::new(),
        warnings: Vec::new(),
    };
    if current_sha == Some(sha.as_str()) {
        return Ok(unchanged);
    }

    // Same guard as a pushed document: the app file may not carry infra
    // sections, so a reload can never change `[database]` / `[api]` either.
    let merged = merge_or_reject(bootstrap, raw, format_hint)?;

    let report = validate_full(&merged, &ValidateOptions::default());
    if !report.valid {
        return Err(anyhow::Error::new(ValidationRejected {
            revision: None,
            report,
        }));
    }

    let previous = swap.current_config().await;
    let diff = ConfigDiff::compute(&previous, &merged);
    swap.apply_runtime(&merged).await?;

    Ok(ReloadResponse {
        applied: true,
        sources_added: diff.added,
        sources_removed: diff.removed,
        sources_changed: diff.changed,
        warnings: report.warnings,
        ..unchanged
    })
}

/// Re-resolve every `[[organizations]]` document from its authority and
/// hot-swap the recomposed runtime (`POST /api/v1/reload`, SIGHUP).
///
/// The multi-org counterpart of [`reload_document`]: it converges the runtime
/// to exactly what a restart would produce — each org's ledger stream when
/// `source = "api"` and a row exists, its `[[organizations]]` file otherwise.
/// Ledger state is never written. `current_identity` short-circuits when no
/// org's document changed (same skip semantics as the single-file sha check).
pub async fn reload_organizations(
    bootstrap: &Config,
    paths: &ConfigPaths,
    store: &Store,
    current_identity: Option<&str>,
    swap: &impl ConfigSwapper,
) -> anyhow::Result<ReloadResponse> {
    let composed = compose_organizations(bootstrap, paths, Some(store), None)?;
    let unchanged = ReloadResponse {
        applied: false,
        content_sha256: composed.identity_sha256.clone(),
        source_path: "[[organizations]]".to_owned(),
        sources_added: Vec::new(),
        sources_removed: Vec::new(),
        sources_changed: Vec::new(),
        warnings: Vec::new(),
    };
    if current_identity == Some(composed.identity_sha256.as_str()) {
        return Ok(unchanged);
    }

    let report = validate_full(&composed.merged, &ValidateOptions::default());
    if !report.valid {
        return Err(anyhow::Error::new(ValidationRejected {
            revision: None,
            report,
        }));
    }

    let previous = swap.current_config().await;
    let diff = ConfigDiff::compute(&previous, &composed.merged);
    swap.apply_runtime(&composed.merged).await?;

    Ok(ReloadResponse {
        applied: true,
        sources_added: diff.added,
        sources_removed: diff.removed,
        sources_changed: diff.changed,
        warnings: report.warnings,
        ..unchanged
    })
}

/// Re-apply the second-newest applied revision (Talos-style rollback).
///
/// This is a **one-step undo, not a history walk**: it re-applies the
/// content of the revision *before* the current one as a brand-new applied
/// revision (append-only ledger — nothing is ever rewound in place). Because
/// [`apply_document`] is idempotent by content sha, two consecutive applied
/// rows always differ, so repeated `rollback` calls **toggle** between the two
/// most recent distinct states (B→A→B→A…) rather than stepping further back
/// through older revisions. To reach an arbitrary earlier revision, `apply`
/// its document explicitly (its content is in the history ledger).
pub async fn rollback(
    bootstrap: &Config,
    origin: &ApplyOrigin,
    store: &Store,
    swap: &impl ConfigSwapper,
    scope: &ApplyScope<'_>,
) -> anyhow::Result<ApplyResponse> {
    // "There is nothing to roll back to" is a statement about current state,
    // not a server malfunction — a bare `anyhow` here surfaced as 500, which
    // told an operator their instance was broken when it was merely on its
    // first revision.
    let previous = store
        .previous_applied_config_revision(scope.ledger_stream())?
        .ok_or_else(|| anyhow::Error::new(NothingToRollBackTo))?;
    apply_document(
        bootstrap,
        &previous.content,
        None,
        origin,
        store,
        swap,
        scope,
    )
    .await
}

// A "restart required" list used to ride on both responses, computed by diffing
// the bootstrap against the merged config. It was structurally always empty:
// `merge_pushed_document` rejects infra sections outright and then *clones*
// `[database]` / `[api]` / `[log]` / `[config_api]` from the running bootstrap,
// so those fields can never differ. Rejecting the document is the honest
// signal (and the privilege boundary — a config-apply credential must not
// reach infra at all); an always-empty array next to it was dead contract.

fn record_attempt(
    store: &Store,
    raw: &str,
    sha: &str,
    origin: &ApplyOrigin,
    status: ConfigRevisionStatus,
    error: Option<&str>,
    organization: Option<&str>,
) -> anyhow::Result<i64> {
    store
        .insert_config_revision(&ConfigRevisionInsert {
            content: raw,
            content_sha256: sha,
            revision_label: origin.revision_label.as_deref(),
            applied_by: origin.applied_by.as_deref(),
            source_addr: origin.source_addr.as_deref(),
            status,
            error,
            organization_id: organization,
        })
        .context("recording config revision")
}

/// Best-effort refs-only body for rejected ledger rows (never store inline secrets).
fn rejected_ledger_body(raw: &str, organization: Option<&str>) -> (String, String) {
    match normalize_document_for_storage(raw, organization) {
        Ok((content, _writes)) => {
            let sha = content_sha256(&content);
            (content, sha)
        }
        // Unparseable body may contain pasted credentials — do not persist it.
        Err(_) => {
            let safe = EMPTY_ORGANIZATION_DOCUMENT;
            (safe.to_owned(), content_sha256(safe))
        }
    }
}

/// Drop UI-managed `app_secret` rows that this scope no longer references.
fn prune_stale_app_secrets(
    bootstrap: &Config,
    store: &Store,
    scope: &ApplyScope<'_>,
    previous_runtime: &Config,
    next_refs: &std::collections::HashSet<String>,
) -> anyhow::Result<()> {
    let previous_refs = previous_secret_env_refs(bootstrap, store, scope, previous_runtime)?;
    let stale = stale_ui_managed_secrets(&previous_refs, next_refs, scope.ledger_stream());
    if stale.is_empty() {
        return Ok(());
    }
    let deleted = store
        .delete_app_secrets(&stale)
        .context("pruning stale app secrets")?;
    if deleted > 0 {
        tracing::info!(
            deleted,
            organization = scope.ledger_stream().unwrap_or("-"),
            "pruned unreferenced UI-managed app_secret rows"
        );
    }
    Ok(())
}

fn previous_secret_env_refs(
    bootstrap: &Config,
    store: &Store,
    scope: &ApplyScope<'_>,
    previous_runtime: &Config,
) -> anyhow::Result<std::collections::HashSet<String>> {
    match scope {
        ApplyScope::Whole => Ok(collect_secret_env_refs(&desired_state_for_ledger(
            previous_runtime,
        ))),
        ApplyScope::Organization {
            organization: org_id,
            paths,
        } => {
            let org = bootstrap
                .organizations
                .iter()
                .find(|org| org.id == *org_id)
                .ok_or_else(|| {
                    anyhow::anyhow!("unknown organization `{org_id}` (not in [[organizations]])")
                })?;
            let (raw, _) = organization_desired_raw(bootstrap, paths, Some(store), org)?;
            match parse_desired_document(&raw) {
                Ok(cfg) => Ok(collect_secret_env_refs(&cfg)),
                Err(_) => Ok(std::collections::HashSet::new()),
            }
        }
    }
}

/// App-layer fields only — safe to store in the config ledger after secret restore.
///
/// `merged` is bootstrap overlayed with desired; serializing it wholesale would
/// embed `[database]` / `[api]` into the ledger and break the next UI apply
/// (`ensure_desired_only`).
fn desired_state_for_ledger(merged: &Config) -> Config {
    Config {
        defaults: merged.defaults.clone(),
        notifiers: merged.notifiers.clone(),
        teams: merged.teams.clone(),
        presets: merged.presets.clone(),
        sources: merged.sources.clone(),
        ..Config::default()
    }
}

/// Hot-swap hook implemented by the HTTP runtime (`AppState` / `WatchSupervisor`).
pub trait ConfigSwapper: Send + Sync {
    /// Effective merged config currently driving the runtime (for compensate).
    fn current_config(&self) -> impl std::future::Future<Output = Config> + Send;

    /// Replace watches, notifier, and runtime tuning from `config`.
    fn apply_runtime(
        &self,
        config: &Config,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}
