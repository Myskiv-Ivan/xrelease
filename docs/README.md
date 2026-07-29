# xrelease

**Self-hosted release notifier** — watch upstream releases on your own
infrastructure and notify the right teams, without a SaaS account.

Published docs: **https://myskiv-ivan.github.io/xrelease/**

## Start here

| Path | When |
|---|---|
| [Quick start](getting-started/quickstart.md) | First run (Docker / Helm) |
| [Configuration overview](configuration/overview.md) | Local vs API vs API + UI authoring |
| [CLI reference](api/cli.md) | `xrelease` / `xrctl` flags and commands |
| [Architecture](concepts/architecture.md) | How `serve`, ledger, UI, and `xrctl` fit |
| [Runtime deployment](operations/deployment.md) | Install matrix, Compose / Helm |

## What it is for

Teams pin many third-party components (GitHub releases, container images, PyPI,
npm, …). xrelease watches those sources, remembers what was already seen in
**PostgreSQL**, and notifies via Apprise, Novu, Slack, Telegram, SMTP,
webhooks, or optional brokers — routed by team tags.

| Binary | Role |
|---|---|
| **`xrelease`** | Backend — poller, outbox, sinks, HTTP API + webhooks (`serve`) |
| **`xrctl`** | Lean management CLI over that API ([CLI reference](api/cli.md)) |

Config split: **`bootstrap.toml`** (infra + optional `[[organizations]]`) vs
**desired state** (Git YAML, API ledger, and/or dashboard Apply). Secrets live
in env (`.env` / K8s Secret).

## Default labs

| | Docker Compose | Kubernetes (Helm) |
|---|---|---|
| **Authoring** | **API + UI** + multi-org | **API + UI** + single-doc ledger |
| **First config** | Idle until UI / `xrctl` Apply | Idle until UI / `xrctl` Apply |
| **Stack** | UI + PG + Apprise | CNPG + Gateway (or Ingress) + Apprise |
| **URL** | http://127.0.0.1:3000 | Gateway / Ingress hostname |
| **Guide** | [Docker](getting-started/docker.md) | [Kubernetes](getting-started/kubernetes.md) · [variants](operations/deployment-variants.md) |

Authoring flags: [Local / API / API + UI](configuration/overview.md#authoring-variants).
Samples: [`deploy/examples/`](https://github.com/Myskiv-Ivan/xrelease/tree/main/deploy/examples).

## Auth (defaults)

| Path | Credential |
|---|---|
| Dashboard | Local username / password ([Authentication](operations/authentication.md)) |
| CLI / CI | `xrctl --api-key` (same value as server `XRELEASE_API_KEY`) |
| SSO | Optional [OIDC](operations/oidc.md) |

## Doc map

| Section | Contents |
|---|---|
| **Getting started** | Quick start, Docker, Kubernetes |
| **Concepts** | Architecture, pipeline, provider categories |
| **Configuration** | Authoring, sources (21), notifications, `[api]` / `[config_api]`, optional `[advisories]` |
| **API** | HTTP endpoints, CLI (`xrelease` / `xrctl`), webhooks, OpenAPI |
| **Operations** | Auth, OIDC, CI apply, deploy, variants, TLS, Gateway, Postgres, CloudNativePG, scaling |
| **Project** | Comparison with alternatives, changelog |

OpenAPI at runtime: Compose UI `http://127.0.0.1:3000/openapi.json` · native
`http://127.0.0.1:8080/openapi.json` — [OpenAPI page](api/openapi.md).

## License

Apache License 2.0 —
[LICENSE](https://github.com/Myskiv-Ivan/xrelease/blob/main/LICENSE) ·
[https://github.com/Myskiv-Ivan/xrelease](https://github.com/Myskiv-Ivan/xrelease)
