//! Local + OIDC application users (`app_user`).

use chrono::{DateTime, Utc};
use postgres::Row;

use super::PostgresStore;
use crate::error::StoreError;
use crate::store::{AppUser, AppUserInsert, AppUserUpsertOidc};

/// Shared projection for every `app_user` read path.
const USER_SELECT: &str =
    "SELECT id, username, password_hash, oidc_sub, email, display_name, role, \
     auth_source, created_at, updated_at, last_login_at, session_version \
     FROM app_user";

impl PostgresStore {
    pub(crate) fn count_users(&self) -> Result<i64, StoreError> {
        let mut client = self.conn()?;
        let count: i64 = client
            .query_one("SELECT COUNT(*) FROM app_user", &[])?
            .get(0);
        Ok(count)
    }

    pub(crate) fn list_users(&self) -> Result<Vec<AppUser>, StoreError> {
        let mut client = self.conn()?;
        let rows = client.query(
            &format!("{USER_SELECT} ORDER BY created_at ASC, id ASC"),
            &[],
        )?;
        Ok(rows.into_iter().map(row_to_user).collect())
    }

    pub(crate) fn get_user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<AppUser>, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_opt(&format!("{USER_SELECT} WHERE username = $1"), &[&username])?;
        Ok(row.map(row_to_user))
    }

    pub(crate) fn get_user_by_id(&self, id: i64) -> Result<Option<AppUser>, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_opt(&format!("{USER_SELECT} WHERE id = $1"), &[&id])?;
        Ok(row.map(row_to_user))
    }

    pub(crate) fn get_user_by_oidc_sub(
        &self,
        oidc_sub: &str,
    ) -> Result<Option<AppUser>, StoreError> {
        let mut client = self.conn()?;
        let row = client.query_opt(&format!("{USER_SELECT} WHERE oidc_sub = $1"), &[&oidc_sub])?;
        Ok(row.map(row_to_user))
    }

    /// Local user with matching email and no `oidc_sub` yet (candidate for SSO link).
    pub(crate) fn find_linkable_local_user_by_email(
        &self,
        email: &str,
    ) -> Result<Option<AppUser>, StoreError> {
        let email = email.trim();
        if email.is_empty() {
            return Ok(None);
        }
        let mut client = self.conn()?;
        let row = client.query_opt(
            &format!(
                "{USER_SELECT} WHERE auth_source = 'local' AND oidc_sub IS NULL \
                 AND lower(email) = lower($1) \
                 ORDER BY id ASC LIMIT 1"
            ),
            &[&email],
        )?;
        Ok(row.map(row_to_user))
    }

    pub(crate) fn insert_user(&self, user: &AppUserInsert<'_>) -> Result<AppUser, StoreError> {
        let mut client = self.conn()?;
        let now = Utc::now();
        // `session_version` uses the column DEFAULT (0) — do not list it here.
        let row = client.query_one(
            "INSERT INTO app_user (
                username, password_hash, oidc_sub, email, display_name,
                role, auth_source, created_at, updated_at, last_login_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8, NULL)
             RETURNING id, username, password_hash, oidc_sub, email, display_name, role,
                       auth_source, created_at, updated_at, last_login_at, session_version",
            &[
                &user.username,
                &user.password_hash,
                &user.oidc_sub,
                &user.email,
                &user.display_name,
                &user.role,
                &user.auth_source,
                &now,
            ],
        )?;
        Ok(row_to_user(row))
    }

    /// Attach (or clear) an IdP subject on an existing local user.
    ///
    /// Keeps `auth_source = 'local'` so password login still works; OIDC auth
    /// resolves the same row via `oidc_sub`.
    pub(crate) fn link_user_oidc_sub(
        &self,
        user_id: i64,
        oidc_sub: Option<&str>,
    ) -> Result<AppUser, StoreError> {
        let mut client = self.conn()?;
        let now = Utc::now();
        let sub = oidc_sub.map(str::trim).filter(|value| !value.is_empty());

        if let Some(sub) = sub {
            if let Some(owner) = self.get_user_by_oidc_sub(sub)? {
                if owner.id != user_id {
                    return Err(StoreError::Other(format!(
                        "oidc_sub `{sub}` is already linked to user {}",
                        owner.id
                    )));
                }
            }
        }

        let row = client.query_opt(
            "UPDATE app_user SET oidc_sub = $2, updated_at = $3 \
             WHERE id = $1 AND auth_source = 'local' \
             RETURNING id, username, password_hash, oidc_sub, email, display_name, role,
                       auth_source, created_at, updated_at, last_login_at, session_version",
            &[&user_id, &sub, &now],
        )?;
        row.map(row_to_user).ok_or_else(|| {
            StoreError::Other(format!(
                "no local app_user with id {user_id} (OIDC-only rows cannot be re-linked here)"
            ))
        })
    }

    /// Upsert an OIDC identity: existing `oidc_sub`, else link a local user by
    /// email, else insert a pure OIDC row.
    pub(crate) fn upsert_oidc_user(
        &self,
        user: &AppUserUpsertOidc<'_>,
    ) -> Result<AppUser, StoreError> {
        let mut client = self.conn()?;
        let now = Utc::now();

        if let Some(existing) = self.get_user_by_oidc_sub(user.oidc_sub)? {
            let row = client.query_one(
                "UPDATE app_user SET
                    email = COALESCE($2, email),
                    display_name = COALESCE($3, display_name),
                    role = $4,
                    updated_at = $5,
                    last_login_at = $5
                 WHERE id = $1
                 RETURNING id, username, password_hash, oidc_sub, email, display_name, role,
                           auth_source, created_at, updated_at, last_login_at, session_version",
                &[
                    &existing.id,
                    &user.email,
                    &user.display_name,
                    &user.role,
                    &now,
                ],
            )?;
            return Ok(row_to_user(row));
        }

        if let Some(email) = user.email.map(str::trim).filter(|value| !value.is_empty()) {
            if let Some(local) = self.find_linkable_local_user_by_email(email)? {
                let row = client.query_one(
                    "UPDATE app_user SET
                        oidc_sub = $2,
                        email = COALESCE($3, email),
                        display_name = COALESCE($4, display_name),
                        role = $5,
                        updated_at = $6,
                        last_login_at = $6
                     WHERE id = $1
                     RETURNING id, username, password_hash, oidc_sub, email, display_name, role,
                               auth_source, created_at, updated_at, last_login_at, session_version",
                    &[
                        &local.id,
                        &user.oidc_sub,
                        &user.email,
                        &user.display_name,
                        &user.role,
                        &now,
                    ],
                )?;
                return Ok(row_to_user(row));
            }
        }

        // New rows get `session_version` DEFAULT 0.
        let row = client.query_one(
            "INSERT INTO app_user (
                username, password_hash, oidc_sub, email, display_name,
                role, auth_source, created_at, updated_at, last_login_at
             ) VALUES (NULL, NULL, $1, $2, $3, $4, 'oidc', $5, $5, $5)
             RETURNING id, username, password_hash, oidc_sub, email, display_name, role,
                       auth_source, created_at, updated_at, last_login_at, session_version",
            &[
                &user.oidc_sub,
                &user.email,
                &user.display_name,
                &user.role,
                &now,
            ],
        )?;
        Ok(row_to_user(row))
    }

    pub(crate) fn touch_user_last_login(&self, id: i64) -> Result<(), StoreError> {
        let mut client = self.conn()?;
        let now = Utc::now();
        client.execute(
            "UPDATE app_user SET last_login_at = $2, updated_at = $2 WHERE id = $1",
            &[&id, &now],
        )?;
        Ok(())
    }

    /// Invalidate every live session for a user (logout / password / role
    /// change) by advancing its session epoch. Returns the new version.
    pub(crate) fn bump_session_version(&self, id: i64) -> Result<i64, StoreError> {
        let mut client = self.conn()?;
        let now = Utc::now();
        let row = client.query_opt(
            "UPDATE app_user SET session_version = session_version + 1, updated_at = $2 \
             WHERE id = $1 RETURNING session_version",
            &[&id, &now],
        )?;
        row.map(|row| row.get(0))
            .ok_or_else(|| StoreError::Other(format!("no app_user with id {id}")))
    }
}

fn row_to_user(row: Row) -> AppUser {
    AppUser {
        id: row.get(0),
        username: row.get(1),
        password_hash: row.get(2),
        oidc_sub: row.get(3),
        email: row.get(4),
        display_name: row.get(5),
        role: row.get(6),
        auth_source: row.get(7),
        created_at: row.get(8),
        updated_at: row.get(9),
        last_login_at: row.get::<_, Option<DateTime<Utc>>>(10),
        session_version: row.get(11),
    }
}
