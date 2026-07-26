-- Target PostgreSQL schema (greenfield baseline = schema version 1).
-- Applied on every Store open via `CREATE … IF NOT EXISTS`, then the versioned
-- migrator in `src/store/postgres/migrate.rs` stamps/upgrades `schema_meta`.
-- Additive upgrades after v1 ship as numbered SQL steps — not by editing
-- CREATE TABLE bodies alone (existing databases would miss new columns).

-- Singleton row tracking the applied schema version (see migrate.rs).
CREATE TABLE IF NOT EXISTS schema_meta (
 singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
 version INTEGER NOT NULL,
 applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS source_state (
 source_id TEXT PRIMARY KEY,
 initialized BOOLEAN NOT NULL DEFAULT FALSE,
 last_polled_at TIMESTAMPTZ,
 etag TEXT,
 latest_release_tag TEXT
);

CREATE TABLE IF NOT EXISTS seen_release (
 source_id TEXT NOT NULL,
 identity TEXT NOT NULL,
 display_tag TEXT,
 first_seen_at TIMESTAMPTZ NOT NULL,
 content_digest TEXT,
 published_at TIMESTAMPTZ,
 url TEXT,
 PRIMARY KEY (source_id, identity)
);

CREATE TABLE IF NOT EXISTS webhook_delivery (
 delivery_id TEXT PRIMARY KEY,
 received_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS notification_outbox (
 id BIGSERIAL PRIMARY KEY,
 source_id TEXT NOT NULL,
 identity TEXT NOT NULL,
 content_digest TEXT,
 display_tag TEXT,
 published_at TIMESTAMPTZ,
 title TEXT NOT NULL,
 body TEXT NOT NULL,
 url TEXT,
 routing_tag TEXT,
 source_kind TEXT NOT NULL,
 status TEXT NOT NULL DEFAULT 'pending',
 attempts INTEGER NOT NULL DEFAULT 0,
 last_error TEXT,
 created_at TIMESTAMPTZ NOT NULL,
 sent_at TIMESTAMPTZ,
 -- Delivery lease: NULL or past = claimable; future = in flight.
 locked_until TIMESTAMPTZ,
 -- Cron-gated delivery (`notify_schedule`); NULL = deliver ASAP.
 deliver_after TIMESTAMPTZ,
 UNIQUE (source_id, identity)
);

CREATE INDEX IF NOT EXISTS idx_notification_outbox_status
 ON notification_outbox (status, created_at);

CREATE TABLE IF NOT EXISTS notification_sink_delivery (
 outbox_id BIGINT NOT NULL REFERENCES notification_outbox(id) ON DELETE CASCADE,
 sink_index INTEGER NOT NULL,
 sink_kind TEXT NOT NULL,
 status TEXT NOT NULL DEFAULT 'pending',
 attempts INTEGER NOT NULL DEFAULT 0,
 last_error TEXT,
 sent_at TIMESTAMPTZ,
 PRIMARY KEY (outbox_id, sink_index)
);

CREATE INDEX IF NOT EXISTS idx_notification_sink_delivery_status
 ON notification_sink_delivery (outbox_id, status);

-- Applied-configuration audit ledger. Desired documents store structure +
-- secret refs (`*_env`); values live in `app_secret`.
-- `organization_id`: NULL = single-document stream; slug = one org.
CREATE TABLE IF NOT EXISTS config_revision (
 id BIGSERIAL PRIMARY KEY,
 content TEXT NOT NULL,
 content_sha256 TEXT NOT NULL,
 revision_label TEXT,
 applied_at TIMESTAMPTZ NOT NULL,
 applied_by TEXT,
 source_addr TEXT,
 status TEXT NOT NULL,
 error TEXT,
 organization_id TEXT
);

CREATE INDEX IF NOT EXISTS idx_config_revision_applied_at
 ON config_revision (applied_at DESC);

CREATE INDEX IF NOT EXISTS idx_config_revision_status_applied_at
 ON config_revision (status, applied_at DESC);

CREATE INDEX IF NOT EXISTS idx_config_revision_org_status_applied_at
 ON config_revision (organization_id, status, applied_at DESC);

-- UI / API secret store: AES-256-GCM ciphertext keyed by env-var name.
CREATE TABLE IF NOT EXISTS app_secret (
 name TEXT PRIMARY KEY,
 ciphertext TEXT NOT NULL,
 value_sha256 TEXT NOT NULL,
 updated_at TIMESTAMPTZ NOT NULL
);

-- Local + OIDC UI users (first-boot admin seed; OIDC upsert on sync).
CREATE TABLE IF NOT EXISTS app_user (
 id BIGSERIAL PRIMARY KEY,
 username TEXT UNIQUE,
 password_hash TEXT,
 oidc_sub TEXT UNIQUE,
 email TEXT,
 display_name TEXT,
 role TEXT NOT NULL,
 auth_source TEXT NOT NULL,
 created_at TIMESTAMPTZ NOT NULL,
 updated_at TIMESTAMPTZ NOT NULL,
 last_login_at TIMESTAMPTZ,
 -- Bumped on logout / password / role change to invalidate live session JWTs.
 session_version BIGINT NOT NULL DEFAULT 0,
 CONSTRAINT app_user_role_check CHECK (role IN ('admin', 'operator', 'viewer')),
 CONSTRAINT app_user_auth_source_check CHECK (auth_source IN ('local', 'oidc')),
 CONSTRAINT app_user_identity_check CHECK (
 (auth_source = 'local' AND username IS NOT NULL AND password_hash IS NOT NULL)
 OR (auth_source = 'oidc' AND oidc_sub IS NOT NULL)
 )
);

CREATE INDEX IF NOT EXISTS idx_app_user_username
 ON app_user (username)
 WHERE username IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_app_user_oidc_sub
 ON app_user (oidc_sub)
 WHERE oidc_sub IS NOT NULL;

-- Retention / prune helpers (Store::prune).
CREATE INDEX IF NOT EXISTS idx_seen_release_first_seen_at
 ON seen_release (first_seen_at);

CREATE INDEX IF NOT EXISTS idx_webhook_delivery_received_at
 ON webhook_delivery (received_at);

CREATE INDEX IF NOT EXISTS idx_notification_outbox_sent_at
 ON notification_outbox (sent_at)
 WHERE status = 'sent';
