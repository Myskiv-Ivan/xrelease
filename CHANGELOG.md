# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
with the **0.x** caveat below.

## Versioning (0.x)

While the major version is `0`, the public HTTP API, desired-document shape, and
PostgreSQL schema may change in minor releases when required for correctness.
Breaking changes are called out under **Changed** / **Removed**. Prefer pinning
GHCR tags and reading this file before upgrades.

Schema upgrades run automatically on startup. Lab resets remain
`docker compose down -v`.

## [Unreleased]

### Changed

- `Runtime` keeps one PostgreSQL pool (`Arc<Store>`) for the process lifetime.
  Config resolve, `health`, outbox requeue, and `serve` share it via
  `Engine::open_with_store` instead of opening a second pool (and re-applying
  schema DDL) on every call.
- Online `validate` reuses the Runtime HTTP client (no second `reqwest::Client`).
- Kafka, NATS, and RabbitMQ notifier sinks are always compiled into the
  `xrelease` binary (no cargo features / `all-notifiers`). Local builds, GHCR
  images, and GitHub Release archives share one notifier surface.
- Helm defaults are CloudNativePG + Gateway API; site overlay is
  [`deploy/k8s/values.yaml`](deploy/k8s/values.yaml) (+ gitignored
  `values.secrets.yaml`). Optional Ingress / builtin PG / OIDC are local
  value overrides — [deployment variants](docs/operations/deployment-variants.md).
- TLS guides: Compose PEM on UI nginx; Kubernetes terminates on the Gateway
  listener (or Ingress via a local overlay) —
  [tls.md](docs/operations/tls.md) · [gateway.md](docs/operations/gateway.md).
- Dependency lockfile refresh within semver (tokio, reqwest, clap, toml, …);
  frontend `package-lock.json` updated. `feed-rs` stays on 2.x.

### Fixed

- UI CrashLoopBackOff on Kubernetes: Helm nginx ConfigMap used literal
  `proxy_pass http://xrelease:…`, which nginx resolves **once at start**.
  If Service DNS is briefly missing, nginx exits with
  `host not found in upstream` and never recovers. Upstream is now a
  variable + `kube-dns` resolver (runtime DNS); Compose uses `127.0.0.11`.
- UI worker count: enable `NGINX_ENTRYPOINT_WORKER_PROCESSES_AUTOTUNE` so
  workers follow the pod CPU quota instead of `worker_processes auto` ×
  node CPUs (OOM risk under the previous 128Mi limit). Memory limit raised
  to 256Mi.
- CLI / container `health` no longer opens a fresh database pool on every
  probe (Docker/Kubernetes liveness intervals).
- UI OIDC role mapping calls `toLowerCase()` / `trim()` correctly, so IdP
  group claims resolve to viewer/operator/admin (and `alias:org` grants).
- UI nginx preserves edge `X-Forwarded-Proto` when proxying to the API
  (HTTPS behind Gateway / Ingress / reverse proxy).

### Security

- Bump `rustls` (0.23.43 → 0.23.46) and `h2` (0.4.15 → 0.4.20) for published
  advisories; replace yanked `chacha20` 0.10.1 (`rand` 0.10.3).
  `cargo deny check advisories` is clean.

### Added

- Automatic PostgreSQL schema versioning on startup (`schema_meta`).
- Docker HTTPS on UI nginx with **cert.pem / key.pem** via `.env`
  (`XRELEASE_TLS_CERT`, `XRELEASE_TLS_KEY`, ports) —
  [`docker/compose.tls.yaml`](docker/compose.tls.yaml).
- Optional security-advisory enrichment (`[advisories]`) via OSV — opt-in;
  Compose lab enables it in [`bootstrap.toml`](bootstrap.toml).
