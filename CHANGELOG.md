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

### Fixed

- UI OIDC role mapping calls `toLowerCase()` / `trim()` correctly, so IdP
  group claims resolve to viewer/operator/admin (and `alias:org` grants).
- UI nginx preserves edge `X-Forwarded-Proto` when proxying to the API
  (HTTPS behind Gateway / Ingress / reverse proxy).

### Added

- Automatic PostgreSQL schema versioning on startup (`schema_meta`).
- Docker HTTPS on UI nginx with **cert.pem / key.pem** via `.env`
  (`XRELEASE_TLS_CERT`, `XRELEASE_TLS_KEY`, ports) —
  [`docker/compose.tls.yaml`](docker/compose.tls.yaml).
- Optional security-advisory enrichment (`[advisories]`) via OSV — opt-in;
  Compose lab enables it in [`bootstrap.toml`](bootstrap.toml).
