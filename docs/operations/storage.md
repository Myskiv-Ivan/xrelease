# Storage backends

xrelease persists release state, the notification outbox, webhook idempotency keys,
per-sink delivery tracking, the config ledger, and local/OIDC users in **PostgreSQL**.

## Connection

```toml
[database]
postgres_url = "postgres://xrelease:@postgres:5432/xrelease"
```

Password and full URL should come from **`XRELEASE_DATABASE_URL`** (Docker `.env`
or Kubernetes Secret) — do not commit credentials to Git.

| Property | Behaviour |
|---|---|
| Deployment | Single xrelease replica per database (one poller) |
| Concurrency | Connection pool; single writer for mutations |
| TLS | `ssl_mode` / URL `sslmode` / `XRELEASE_DATABASE_SSL_MODE` — see [TLS](tls.md) |
| Backup | `pg_dump` / managed service snapshots |
| HA | Managed Postgres (RDS, Cloud SQL) or CloudNativePG |
| Schema | Applied automatically on startup |

See [Scaling](scaling.md) for retention (`prune_*`) and growth expectations.

### Tables

| Table | Purpose | Growth |
|---|---|---|
| `source_state` | Baseline flag, etag, last poll, latest tag | 1 row per source |
| `seen_release` | Identity-set diff / deduplication | Grows with unique tags; prune via `prune_seen_*` |
| `notification_outbox` | Transactional delivery queue | Pending/failed/dead; sent rows pruneable |
| `notification_sink_delivery` | Per-sink retry state (team routing fan-out) | Child of outbox |
| `webhook_delivery` | Inbound webhook idempotency | Short-lived keys; pruneable |
| `config_revision` | Applied app config history (`source = api`) | Append-only; structure + `*_env` refs; stream via `organization_id` |
| `app_secret` | UI/API secret values keyed by env-var name | Encrypted at rest when a key is set |
| `app_user` | Local + OIDC UI principals / session version | Small |
| `release_advisory` | Cached OSV findings per package coordinate | When `[advisories]` enabled; prune via `prune_advisories_*` |
| `advisory_check` | “Looked up, nothing found” markers | Same retention as `release_advisory` |
| `schema_meta` | Applied schema version | 1 row |

Desired-state authority and how CLI/API/UI interact with `config_revision`:
[Architecture — config lifecycle](../concepts/architecture.md#config-lifecycle).

### Config secrets (at rest)

Desired documents store **refs** (`*_env`), not secret values. On API apply the
store:

1. Moves inline secrets into `app_secret` (AES-256-GCM when
   `XRELEASE_CONFIG_ENCRYPTION_KEY` is set) and sets matching `*_env` names.
2. Writes the refs-only document to `config_revision.content`.
3. Loads `app_secret` into the process vault on boot so runtime resolve matches
   process env.

| Property | Behaviour |
|---|---|
| Key source | Env / K8s Secret only (`openssl rand -base64 32`) |
| `content_sha256` / ETag | Hash of the **refs-only** ledger body |
| Secret rotation | Same `*_env` + new inline value → ledger sha unchanged; `app_secret` still upserts |
| Orphan GC | Removed UI-managed `XRELEASE_UI_*` refs are deleted from `app_secret` on apply |
| Document column | Plaintext structure + env names — readable in SQL |
| `app_secret` | Encrypted values; require the key to open |
| API mode without key | `xrelease validate` / serve **error** unless `XRELEASE_ALLOW_PLAINTEXT_CONFIG_LEDGER=1` |

Prefer committing only `*_env` (GitOps) so values never enter the apply body.

### Schema upgrades

On startup the backend applies the current schema and records the version in
`schema_meta`. Newer releases may upgrade the database automatically. If the
database version is **newer** than the running binary, startup fails — upgrade
xrelease first.

## Docker storage

Postgres data lives in the `postgres-data` Compose volume, mounted at
`/var/lib/postgresql` with the image's default `PGDATA`
(`/var/lib/postgresql/18/docker`). The xrelease container is **stateless** —
its root filesystem is read-only with `tmpfs` for `/data` and `/tmp`.

Lab reset (wipes all state):

```bash
docker compose down -v
docker compose up -d
```

## Kubernetes storage

- **PostgreSQL:** CloudNativePG (`values.yaml`), managed service (GitOps variant), or chart StatefulSet (lab)
- **xrelease pod:** ConfigMap for config files, `emptyDir` for `/data` + `/tmp`
- **Apprise:** stateless in the chart; add a PVC for persistent channel registration

The chart's PVC (`{release}-postgresql`) carries
`helm.sh/resource-policy: keep`, so `helm uninstall` leaves the data behind.
Reclaim it deliberately:

```bash
kubectl -n xrelease delete pvc xrelease-postgresql
```

Set `postgresql.persistence.retain=false` for throwaway clusters.

### Volumes are not interchangeable between Compose and Helm

The chart pins `PGDATA=/var/lib/postgresql/pgdata` while Compose keeps the
image default. A volume written by one will not be picked up by the other —
move data with `pg_dump` / `psql`, not by copying the filesystem.

## Per-sink delivery

Fan-out notifications write one parent row to `notification_outbox` and one child
row per matching sink in `notification_sink_delivery`. Retries only hit sinks
still in `pending` / `failed`.

## Backup & restore

```bash
# Docker
docker compose exec postgres pg_dump -U xrelease xrelease > backup.sql

# Restore (stop xrelease first)
docker compose exec -T postgres psql -U xrelease xrelease < backup.sql
```

Inspect outbox:

```bash
docker compose exec postgres psql -U xrelease xrelease -c \
  "SELECT status, COUNT(*) FROM notification_outbox GROUP BY status"
```

See [`deploy/README.md`](../../deploy/README.md) for Docker and Kubernetes layout.
