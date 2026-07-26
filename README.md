# xrelease

[![CI](https://github.com/Myskiv-Ivan/xrelease/actions/workflows/ci.yml/badge.svg)](https://github.com/Myskiv-Ivan/xrelease/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-GitHub%20Pages-blue)](https://myskiv-ivan.github.io/xrelease/)
[![CodeQL](https://github.com/Myskiv-Ivan/xrelease/actions/workflows/codeql.yml/badge.svg)](https://github.com/Myskiv-Ivan/xrelease/actions/workflows/codeql.yml)
[![License](https://img.shields.io/github/license/Myskiv-Ivan/xrelease)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.96-orange?logo=rust)](https://www.rust-lang.org/)

## What it is for

**xrelease** is a **self-hosted release notifier**. You run it on your own
infrastructure so teams get notified when upstream software they depend on
ships a new version — without sending config, tokens, or notification secrets
to a SaaS.

Typical jobs:

- Watch **Git forges**, **container registries**, **package indexes**, and
  **RSS/Atom** feeds (21 source kinds)
- Diff new tags/releases against **PostgreSQL** state (silent baseline on first
  poll — no history flood)
- Deliver via **[Apprise](https://github.com/caronc/apprise)**, webhooks, SMTP,
  eXpress BotX, or optional Kafka / NATS / RabbitMQ sinks
- Route by **team tags** so platform, security, and product each get the right
  channels

Two binaries:

| Binary | Role |
|---|---|
| **`xrelease`** | Backend: poller, outbox, sinks, HTTP API + webhooks (`serve`) |
| **`xrctl`** | Lean remote CLI over that API — no Postgres, no local config authority |

Optional **dashboard** for observability; config editing is opt-in (see below).

## Deployment variants

Two independent choices: **how you author** desired state, and **how the
process runs**. Pick one authoring variant in `[config_api]` of
[`bootstrap.toml`](bootstrap.toml).

### 1 — How you author config (Local / API / API + UI)

| Variant | When to use | Who changes sources / notifiers | Where it lives |
|---|---|---|---|
| **Local** | GitOps, ConfigMaps, audited YAML in Git | Edit files → reload / restart | Disk |
| **API** | CI / GitOps-via-API, no browser editing | `xrctl apply` / curl | PostgreSQL ledger |
| **API + UI** | Lab or operators who prefer forms | Dashboard **Config** (same apply API) | PostgreSQL ledger |

#### Local

```toml
[config_api]
api_config = false
source = "local"
ui_config = false
```

Edit `app/releases.yaml` (or per-org `app/<id>/releases.yaml`), then
`xrctl reload` / `SIGHUP` / restart. Default if you omit `[config_api]`.

#### API

```toml
[config_api]
api_config = true
source = "api"
ui_config = false
```

```sh
xrctl validate app/releases.yaml
xrctl apply app/releases.yaml --if-match none --label "$GIT_SHA"
# Multi-org:
xrctl --organization platform apply app/platform/releases.yaml
```

Dashboard stays read-only (status / sources / outbox). Needs `admin` (or API key).

#### API + UI

```toml
[config_api]
api_config = true
source = "api"
ui_config = true
```

Same ledger as **API**, plus **Config → Edit → Apply** in the UI. The UI never
writes YAML to disk. Both Docker and Helm labs use this profile by default
(Compose: multi-org; Helm: single-doc).

| Sample | Variant |
|---|---|
| Docker Compose ([`bootstrap.toml`](bootstrap.toml)) | **API + UI** + multi-org (idle until Apply) |
| Helm ([`deploy/helm/xrelease`](deploy/helm/xrelease)) | **API + UI** + single-doc ledger (idle until Apply) |
| [`deploy/examples/infra-app/`](deploy/examples/infra-app/) | **API** (CI / `xrctl`, no UI editor) |
| [`deploy/examples/multi-team/`](deploy/examples/multi-team/), [`multi-org/`](deploy/examples/multi-org/) | **Local** |

Invalid: `api_config=false` + `source=api`, or `ui_config=true` without API apply.
Full tables: [authoring variants](docs/configuration/overview.md#authoring-variants).
Quick start: [docs/getting-started/quickstart.md](docs/getting-started/quickstart.md).

### 2 — How the backend runs

| Mode | Command | Use when |
|---|---|---|
| **Backend** | `xrelease serve` (default) | API + poller + webhooks (Compose / Helm) |

Only one poller may run per database (advisory lease). Details:
[runtime deployment](docs/operations/deployment.md).

## Integrations

### Sources (what to watch) — 21 kinds

| Category | `type` | Watches |
|---|---|---|
| Git | `github` | GitHub releases |
| Git | `codeberg` | Codeberg releases |
| Git | `gitea` | Self-hosted Gitea / Forgejo |
| Git | `gitlab` | GitLab releases |
| Git | `bitbucket` | Bitbucket Cloud / Server tags |
| Containers | `docker` | Any OCI registry (custom URL) |
| Containers | `ghcr` | GitHub Container Registry |
| Containers | `quay` | Quay.io |
| Containers | `ecr` | AWS ECR Public gallery |
| Packages | `pypi` | Python (PyPI) |
| Packages | `npm` | npm packages |
| Packages | `yarn` | Yarn registry |
| Packages | `cargo` | Rust crates (crates.io) |
| Packages | `maven` | Maven Central |
| Packages | `nuget` | NuGet.org |
| Packages | `hex` | Hex.pm (Elixir) |
| Packages | `rubygems` | RubyGems.org |
| Packages | `packagist` | Packagist (PHP) |
| Packages | `cpan` | Perl CPAN (MetaCPAN) |
| Other | `feed` | RSS / Atom / JSON Feed |
| Other | `artifacthub` | Artifact Hub (Helm charts by default) |

Inbound **webhooks** (with `serve`): GitHub, GitLab, Gitea/Codeberg, Bitbucket,
Docker Hub, plus a generic webhook — [webhooks](docs/api/webhooks.md).

Field reference: [sources](docs/configuration/sources.md) ·
[provider categories](docs/concepts/providers.md).

### Notifications (where to send)

| `type` | Integration | Always available? |
|---|---|---|
| `apprise` | [Apprise](https://github.com/caronc/apprise) HTTP API — Telegram, Slack, Discord, email, … (80+ URL schemes) | Yes |
| `webhook` | Custom HTTP `POST` / `PUT` | Yes |
| `smtp` | Direct e-mail (STARTTLS / TLS / plain) | Yes |
| `slack` | Slack Incoming Webhook or Bot `chat.postMessage` | Yes |
| `telegram` | Telegram Bot API `sendMessage` | Yes |
| `express` | eXpress BotX chat | Yes |
| `novu` | [Novu](https://github.com/novuhq/novu) workflow trigger | Yes |
| `kafka` | Apache Kafka | Yes (published images) |
| `nats` | NATS | Yes (published images) |
| `rabbitmq` | RabbitMQ | Yes (published images) |

Routing: source `routing_tag` ↔ notifier `tags`. Details:
[notifications](docs/configuration/apprise.md).

## Quick links

- **Docs site:** https://myskiv-ivan.github.io/xrelease/ (mdBook · GitHub Pages)
- **Docs hub (source):** [`docs/`](docs/README.md)
- **Quickstart:** [`docs/getting-started/quickstart.md`](docs/getting-started/quickstart.md)
- **Docker:** [`docker/`](docker/README.md) · **Helm:** [`deploy/helm/xrelease/`](deploy/helm/xrelease/README.md)
- **Config layout:** [`bootstrap.toml`](bootstrap.toml) (infra) + desired state via UI/API ledger or Git YAML ([`app/releases.example.yaml`](app/releases.example.yaml)); secrets: [`.env.example`](.env.example)
- **Integrations:** [sources](docs/configuration/sources.md) · [notifications](docs/configuration/apprise.md)
- **Dashboard / UI:** [Docker](docker/README.md) · [Authentication](docs/operations/authentication.md) · [OIDC](docs/operations/oidc.md)
- **Management CLI:** [`xrctl`](docs/api/cli.md) — default `http://127.0.0.1:8080`; Docker Compose UI → `http://127.0.0.1:3000`; CI image `ghcr.io/…/xrelease-cli`
- **OpenAPI:** [`api/openapi.json`](api/openapi.json)

```sh
# Published GHCR images
cp .env.example .env && docker compose up -d
# Dashboard / xrctl / curl → http://127.0.0.1:3000  (not :8080 on the host)
# xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" status
```

## Contributing & security

See [`CONTRIBUTING.md`](CONTRIBUTING.md), [`SECURITY.md`](SECURITY.md), and
[`CHANGELOG.md`](CHANGELOG.md) (0.x may include breaking changes between minors —
pin release tags for production).

## License

Apache-2.0 — see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE).
