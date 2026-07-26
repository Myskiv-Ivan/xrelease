//! Config ledger (`config_revision`) — apply history per stream.
//!
//! Desired documents store structure + secret *refs* (`*_env`). Values live in
//! `app_secret` (see).

use chrono::{DateTime, Utc};

use super::PostgresStore;
use crate::crypto::LedgerCipher;
use crate::store::{
    ConfigRevisionInsert, ConfigRevisionRecord, ConfigRevisionStatus, ConfigRevisionSummary,
    StoreError,
};

impl PostgresStore {
    /// Newest revision with `status` at `offset` within one ledger stream.
    ///
    /// The stream key is `organization_id` (`None` = the legacy single-document
    /// stream): idempotency, rollback, and boot resolution must never
    /// cross streams, so every single-row lookup filters
    /// `organization_id IS NOT DISTINCT FROM $2`.
    fn stream_config_revision(
        &self,
        status: &str,
        offset: i64,
        organization: Option<&str>,
    ) -> Result<Option<ConfigRevisionRecord>, StoreError> {
        let mut client = self.conn()?;
        let row = client
            .query_opt(
                "SELECT id, content, content_sha256, revision_label, applied_at, applied_by, \
 source_addr, status, error, organization_id \
 FROM config_revision \
 WHERE status = $1 AND organization_id IS NOT DISTINCT FROM $2 \
 ORDER BY applied_at DESC, id DESC OFFSET $3 LIMIT 1",
                &[&status, &organization, &offset],
            )?
            .map(map_config_revision_row)
            .transpose()?;
        Ok(row)
    }

    /// Latest successfully applied config revision in one stream.
    pub fn latest_applied_config_revision(
        &self,
        organization: Option<&str>,
    ) -> Result<Option<ConfigRevisionRecord>, StoreError> {
        self.stream_config_revision("applied", 0, organization)
    }

    /// Latest rejected config apply attempt in one stream.
    pub fn latest_rejected_config_revision(
        &self,
        organization: Option<&str>,
    ) -> Result<Option<ConfigRevisionRecord>, StoreError> {
        self.stream_config_revision("rejected", 0, organization)
    }

    /// Latest rejected apply attempt across EVERY stream (status banner: a
    /// multi-org admin wants to see any org's failed push, not just the
    /// legacy stream's).
    pub fn latest_rejected_config_revision_any(
        &self,
    ) -> Result<Option<ConfigRevisionRecord>, StoreError> {
        let mut client = self.conn()?;
        let row = client
            .query_opt(
                "SELECT id, content, content_sha256, revision_label, applied_at, applied_by, \
 source_addr, status, error, organization_id \
 FROM config_revision WHERE status = 'rejected' \
 ORDER BY applied_at DESC, id DESC LIMIT 1",
                &[],
            )?
            .map(map_config_revision_row)
            .transpose()?;
        Ok(row)
    }

    /// Second-newest successfully applied revision (rollback target) in one stream.
    pub fn previous_applied_config_revision(
        &self,
        organization: Option<&str>,
    ) -> Result<Option<ConfigRevisionRecord>, StoreError> {
        self.stream_config_revision("applied", 1, organization)
    }

    /// Page through the whole ledger (every stream), newest first.
    ///
    /// Returns metadata only ([`ConfigRevisionSummary`] omits `content`).
    pub fn list_config_revisions(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ConfigRevisionSummary>, StoreError> {
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT id, content_sha256, revision_label, applied_at, applied_by, \
 source_addr, status, error, organization_id \
 FROM config_revision \
 ORDER BY applied_at DESC, id DESC \
 LIMIT $1 OFFSET $2",
            &[&limit, &offset],
        )?;
        rows.into_iter().map(map_config_revision_summary).collect()
    }

    /// Total ledger rows across every stream (for history pagination).
    pub fn count_config_revisions(&self) -> Result<i64, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_one("SELECT COUNT(*)::BIGINT FROM config_revision", &[])?;
        Ok(row.get(0))
    }

    /// Page through one organization's ledger stream, newest first.
    pub fn list_config_revisions_for(
        &self,
        organization: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ConfigRevisionSummary>, StoreError> {
        let mut client = self.conn()?;
        let rows = client.query(
            "SELECT id, content_sha256, revision_label, applied_at, applied_by, \
 source_addr, status, error, organization_id \
 FROM config_revision \
 WHERE organization_id = $1 \
 ORDER BY applied_at DESC, id DESC \
 LIMIT $2 OFFSET $3",
            &[&organization, &limit, &offset],
        )?;
        rows.into_iter().map(map_config_revision_summary).collect()
    }

    /// Ledger rows in one organization's stream (for history pagination).
    pub fn count_config_revisions_for(&self, organization: &str) -> Result<i64, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_one(
            "SELECT COUNT(*)::BIGINT FROM config_revision WHERE organization_id = $1",
            &[&organization],
        )?;
        Ok(row.get(0))
    }

    /// Append one config apply attempt (applied or rejected).
    ///
    /// `content` is stored as provided (caller normalizes secrets to refs).
    /// `content_sha256` is the caller's hash of that ledger body.
    pub fn insert_config_revision(
        &self,
        row: &ConfigRevisionInsert<'_>,
    ) -> Result<i64, StoreError> {
        let mut client = self.conn()?;
        let id: i64 = client
            .query_one(
                "INSERT INTO config_revision \
 (content, content_sha256, revision_label, applied_at, applied_by, source_addr, \
 status, error, organization_id) \
 VALUES ($1, $2, $3, now(), $4, $5, $6, $7, $8) \
 RETURNING id",
                &[
                    &row.content,
                    &row.content_sha256,
                    &row.revision_label,
                    &row.applied_by,
                    &row.source_addr,
                    &row.status.as_str(),
                    &row.error,
                    &row.organization_id,
                ],
            )?
            .get(0);
        Ok(id)
    }
}

fn map_config_revision_row(row: postgres::Row) -> Result<ConfigRevisionRecord, StoreError> {
    let status_raw: &str = row.get(7);
    let status = ConfigRevisionStatus::parse(status_raw).ok_or_else(|| {
        StoreError::Other(format!("invalid config_revision.status `{status_raw}`"))
    })?;
    let applied_at: DateTime<Utc> = row.get(4);
    let content: String = row.get(1);

    if LedgerCipher::is_encrypted(&content) {
        return Err(StoreError::Other(
            "config_revision.content must be plaintext desired-state (refs only); \
 encrypted blobs are not supported"
                .into(),
        ));
    }

    Ok(ConfigRevisionRecord {
        id: row.get(0),
        content,
        content_sha256: row.get(2),
        revision_label: row.get(3),
        applied_at: applied_at.to_rfc3339(),
        applied_by: row.get(5),
        source_addr: row.get(6),
        status,
        error: row.get(8),
        organization_id: row.get(9),
    })
}

fn map_config_revision_summary(row: postgres::Row) -> Result<ConfigRevisionSummary, StoreError> {
    let status_raw: &str = row.get(6);
    let status = ConfigRevisionStatus::parse(status_raw).ok_or_else(|| {
        StoreError::Other(format!("invalid config_revision.status `{status_raw}`"))
    })?;
    let applied_at: DateTime<Utc> = row.get(3);
    Ok(ConfigRevisionSummary {
        id: row.get(0),
        content_sha256: row.get(1),
        revision_label: row.get(2),
        applied_at: applied_at.to_rfc3339(),
        applied_by: row.get(4),
        source_addr: row.get(5),
        status,
        error: row.get(7),
        organization_id: row.get(8),
    })
}
