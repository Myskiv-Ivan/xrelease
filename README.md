# xrelease

[![CI](https://github.com/Myskiv-Ivan/xrelease/actions/workflows/ci.yml/badge.svg)](https://github.com/Myskiv-Ivan/xrelease/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-GitHub%20Pages-blue)](https://myskiv-ivan.github.io/xrelease/)
[![CodeQL](https://github.com/Myskiv-Ivan/xrelease/actions/workflows/codeql.yml/badge.svg)](https://github.com/Myskiv-Ivan/xrelease/actions/workflows/codeql.yml)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.96-orange?logo=rust)](https://www.rust-lang.org/)

**Self-hosted release notifier.** Watch upstream software you depend on and
notify the right teams — on your infrastructure, without sending config or
secrets to a SaaS.

Typical jobs:

- Watch **Git forges**, **container registries**, **package indexes**, and
  **RSS/Atom** feeds (21 source kinds)
- Diff new tags/releases against **PostgreSQL** state (silent baseline on first
  poll — no history flood)
- Deliver via **[Apprise](https://github.com/caronc/apprise)**, **[Novu](https://github.com/novuhq/novu)**,
  Slack, Telegram, SMTP, webhooks, eXpress BotX, or Kafka / NATS / RabbitMQ
- Route by **team tags** so platform, security, and product each get the right
  channels

Two binaries:

| Binary | Role |
|---|---|
| **`xrelease`** | Backend: poller, outbox, sinks, HTTP API + webhooks (`serve`) |
| **`xrctl`** | Lean remote CLI over that API — no Postgres, no local config authority |

Optional **dashboard** for observability; config editing is opt-in (see below).

## Quick start

```sh
cp .env.example .env
# Fill blank secrets (see comments in .env.example)
docker compose up -d
# Dashboard → http://127.0.0.1:3000
```

Sign in with `XRELEASE_ADMIN_USER` / `XRELEASE_ADMIN_PASSWORD`, then
**Config → Edit → Apply** (or use `xrctl`). Guides:
[Quick start](docs/getting-started/quickstart.md) ·
[Docker](docker/README.md) ·
[Helm](deploy/helm/xrelease/README.md).

## Deployment variants

Two independent choices: **how you author** desired state, and **how the
process runs**. Pick one authoring variant in `[config_api]` of
[`bootstrap.toml`](bootstrap.toml).

### 1 — How you author config (Local / API / API + UI)

| Variant | When to use | Who changes sources / notifiers | Where it lives |
|---|---|---|---|
| **Local** | GitOps, ConfigMaps, audited YAML in Git | Edit files → reload / restart | Disk |
| **API** | CI / GitOps-via-API, no browser editing | `xrctl apply` / curl | PostgreSQL ledger |
| **API + UI** | Operators who prefer forms | Dashboard **Config** (same apply API) | PostgreSQL ledger |

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
writes YAML to disk. Docker Compose and Helm labs use this profile by default
(Compose: multi-org; Helm: single-doc).

| Sample | Variant |
|---|---|
| Docker Compose ([`bootstrap.toml`](bootstrap.toml)) | **API + UI** + multi-org (idle until Apply) |
| Helm ([`deploy/helm/xrelease`](deploy/helm/xrelease)) | **API + UI** + single-doc ledger (idle until Apply) |
| [`deploy/examples/infra-app/`](deploy/examples/infra-app/) | **API** (CI / `xrctl`, no UI editor) |
| [`deploy/examples/multi-team/`](deploy/examples/multi-team/), [`multi-org/`](deploy/examples/multi-org/) | **Local** |

Invalid: `api_config=false` + `source=api`, or `ui_config=true` without API apply.
Full tables: [authoring variants](docs/configuration/overview.md#authoring-variants).

### 2 — How the backend runs

| Mode | Command | Use when |
|---|---|---|
| **Backend** | `xrelease serve` (default) | API + poller + webhooks (Compose / Helm / binary) |

| Platform | UI | Auth | Config authority |
|---|---|---|---|
| **Docker Compose** | on (`:3000`) | local admin + API key; OIDC optional | **API + UI** |
| **Kubernetes (Helm)** | on (Ingress) | session/admin (+ OIDC) | **API + UI** (chart default) |
| **Helm production** | optional (`ui.enabled`) | Secret + Ingress TLS | Local / API / API+UI via examples |
| **Binary without UI** | omit UI container / `ui.enabled: false` | API key / OIDC | same `[config_api]` choice |
| **Remote ops** | — | `xrctl --api-key` → live `serve` | `xrctl apply` when `source=api` |

Only one poller may run per database (advisory lease). Details:
[runtime deployment](docs/operations/deployment.md).

```sh
# Published GHCR images
cp .env.example .env && docker compose up -d
# Dashboard / xrctl / curl → http://127.0.0.1:3000  (not :8080 on the host)
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" status

# Kubernetes
cp deploy/k8s/values.secrets.example.yaml deploy/k8s/values.secrets.yaml
helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.secrets.yaml
```

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

| `type` | Integration |
|---|---|
| `apprise` | [Apprise](https://github.com/caronc/apprise) HTTP API — Telegram, Slack, Discord, email, … (80+ URL schemes) |
| `webhook` | Custom HTTP `POST` / `PUT` |
| `smtp` | Direct e-mail (STARTTLS / TLS / plain) |
| `slack` | Slack Incoming Webhook or Bot `chat.postMessage` |
| `telegram` | Telegram Bot API `sendMessage` |
| `express` | eXpress BotX chat |
| `novu` | [Novu](https://github.com/novuhq/novu) workflow trigger (Cloud US/EU or self-host) |
| `kafka` | Apache Kafka topic producer |
| `nats` | NATS subject publisher |
| `rabbitmq` | RabbitMQ exchange publisher |

Every sink kind is compiled into the binary (local builds, GHCR images, and
GitHub Release archives). Routing: source `routing_tag` ↔ notifier `tags`.
Details: [notifications](docs/configuration/apprise.md) ·
[Novu setup](docs/configuration/apprise.md#novu).

## Docs & links

- **Docs site:** https://myskiv-ivan.github.io/xrelease/ (mdBook · GitHub Pages)
- **Docs hub (source):** [`docs/`](docs/README.md)
- **Config layout:** [`bootstrap.toml`](bootstrap.toml) (infra) + desired state via UI/API ledger or Git YAML ([`app/releases.example.yaml`](app/releases.example.yaml)); secrets: [`.env.example`](.env.example)
- **Dashboard / auth:** [Authentication](docs/operations/authentication.md) · [OIDC](docs/operations/oidc.md) · [TLS](docs/operations/tls.md)
- **Management CLI:** [`xrctl`](docs/api/cli.md) — Compose UI → `http://127.0.0.1:3000`; CI image `ghcr.io/…/xrelease-cli`
- **CI apply:** [CI/CD integration](docs/operations/ci-cd.md)
- **OpenAPI:** [`api/openapi.json`](api/openapi.json)

## Contributing & security

See [`CONTRIBUTING.md`](CONTRIBUTING.md), [`SECURITY.md`](SECURITY.md), and
[`CHANGELOG.md`](CHANGELOG.md) (0.x may include breaking changes between minors —
pin release tags for production).

## License

Apache-2.0 — see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE).
