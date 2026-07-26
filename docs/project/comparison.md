# Comparison with alternatives

| | xrelease | GitHub Release Monitor | NewReleases.io + CLI |
|---|---|---|---|
| **Hosting** | Self-hosted | Self-hosted | SaaS |
| **Interface** | YAML/TOML + CLI + API + optional dashboard | Web UI | Web + CLI |
| **Polling** | Your infrastructure | Built-in | Their servers |
| **Webhooks** | Yes (`serve`) | No | Yes (SaaS) |
| **Providers** | 21 in-binary | 3 Git hosts | 22+ SaaS |
| **Notifications** | Apprise (80+) + Slack/Telegram/SMTP/Novu/webhooks + brokers | SMTP + Apprise | Native integrations |
| **State** | PostgreSQL | PostgreSQL or SQLite | Remote |
| **Config authoring** | **Local**, **API** / `xrctl`, or **API + UI** | Web UI | SaaS UI |
| **OpenAPI** | Yes | No | Proprietary API |
| **License** | Apache-2.0 | AGPL-3.0 | Proprietary |
| **Release sub-channels** | `prerelease_tags` | UI toggles | SaaS UI |

## When to choose xrelease

- You want **self-hosted** control of config, state, and notification secrets
- You need **Apprise**, **Novu**, or brokers for notification fan-out
- You watch **packages + containers + Git** in one config
- You want **webhooks + polling** with an OpenAPI-documented API
- You prefer **GitOps** for desired state, with optional API/UI apply when needed

## When to choose alternatives

- **GitHub Release Monitor** — heavier web app, built-in user auth / i18n focus
- **NewReleases** — maximum provider coverage without operating infrastructure

xrelease can coexist with other tools: use it for registries and feeds your SaaS
does not cover, or as a fully self-hosted Git release notifier.

Supported source types: [Sources reference](../configuration/sources.md).
