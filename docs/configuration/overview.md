# Configuration overview

xrelease splits **infrastructure** (how the process runs) from **desired state**
(what to watch and where to notify).

| File | Format | Contents |
|---|---|---|
| `bootstrap.toml` | TOML | `[database]`, `[api]`, `[log]`, `[config_api]`, optional `[[organizations]]` |
| `app/releases.yaml` | YAML (or TOML) | Single desired document: `sources`, `teams`, `presets`, `notifiers`, `defaults` |
| `app/<org>/releases.yaml` | YAML (or TOML) | When `[[organizations]]` is set — same desired shape **per organization** |

```bash
xrelease serve --config bootstrap.toml --app app/releases.yaml
xrelease validate --config bootstrap.toml --app app/releases.yaml --strict
# Multi-org (paths come from [[organizations]] — no --app):
xrelease validate --config deploy/examples/multi-org/bootstrap.toml --strict
```

## Authoring variants

Desired state (sources, teams, notifiers) is authored in one of three modes.
Pick **exactly one** `[config_api]` block in `bootstrap.toml`.

| Variant | Flags | Who changes desired state | Where it lives |
|---|---|---|---|
| **Local** | `api_config=false`, `source="local"`, `ui_config=false` | Edit YAML in Git / volume | Disk file |
| **API** | `api_config=true`, `source="api"`, `ui_config=false` | `xrctl` / CI / curl | PostgreSQL ledger |
| **API + UI** | `api_config=true`, `source="api"`, `ui_config=true` | Dashboard forms (same apply API) | PostgreSQL ledger |

| Flag | Type | Default | Meaning |
|---|---|---|---|
| `api_config` | bool | `false` | Mount `POST …/config/apply` and `/rollback` |
| `source` | `local` \| `api` | `local` | Boot authority + whether push-apply is accepted |
| `ui_config` | bool | `false` | Show the dashboard `/config` form editor |

> **Defaults:** Docker Compose and Helm chart defaults = **API + UI**
> (idle until first Apply). **Local** / **API**: docs + `deploy/examples/` only.

### 1 — Local (GitOps / files)

Disk is the source of truth. No push-apply; no dashboard editor.

```toml
[config_api]
api_config = false
source = "local"
ui_config = false
```

| Step | Action |
|---|---|
| Author | Edit `app/releases.yaml` or `app/<org>/releases.yaml` (Git → volume / ConfigMap) |
| Activate | Restart, `SIGHUP`, or `POST /api/v1/reload` / `xrctl reload` |
| Multi-org | Each `[[organizations]]` **must** set `app = "…"` |

Advanced (rarely needed): mount apply routes but keep file authority — push
returns **409**; still use reload after editing the file:

```toml
[config_api]
api_config = true
source = "local"
ui_config = false
```

### 2 — API (CI / `xrctl`)

Ledger is authoritative. Automation pushes whole documents. Dashboard has
**observability only** (no Config editor).

```toml
[config_api]
api_config = true
source = "api"
ui_config = false
```

| Step | Action |
|---|---|
| Author | `xrctl apply <file>` / `POST …/config/apply` (Bearer admin + optional HMAC) |
| Validate first | `xrctl validate <file>` or `POST …/config/validate` |
| Multi-org | `xrctl --organization <id> apply …` — whole-document apply returns **409** |
| First boot | Optional `--app` / org `app` is a **seed** only; omit for empty until Apply |

Requires auth that can apply (`admin` role or API key). Optional
`XRELEASE_CONFIG_APPLY_SECRET` → `X-Config-Signature`. See
[HTTP API settings](api.md).

### 3 — API + UI (dashboard editor)

Same as **API**, plus the dashboard **Config** form editor. The UI never writes
YAML to disk — it calls the same apply API.

```toml
[config_api]
api_config = true
source = "api"
ui_config = true
```

| Step | Action |
|---|---|
| Author | Org switcher → **Config** → **Edit** → **Apply** (needs `admin`) |
| Also OK | Still use `xrctl` / CI against the same ledger |
| First boot | Empty org/`app` → idle (no sources) until first Apply |

This is the repo-root [`bootstrap.toml`](../../bootstrap.toml) used by Docker Compose.

### Invalid combinations

`xrelease validate` rejects these:

```toml
# Ledger without apply routes
api_config = false
source = "api"

# UI editor without API apply (ui_config alone does nothing)
ui_config = true
api_config = false   # or source = "local"
```

Do not use old names `enabled` / `ui_editing`, or `source` values `file` /
`ledger` / `auto` — they are rejected. Use only `api_config`, `ui_config`, and
`source = "local" | "api"`.

Listen / auth / HMAC / `apply_scope`: [HTTP API settings](api.md).
Runtime / install: [Runtime deployment](../operations/deployment.md).

## Bootstrap example (**Local** / single document)

```toml
[database]
postgres_url = "postgres://xrelease:xrelease@postgres:5432/xrelease"

[log]
level = "info"   # override with XRELEASE_LOG (e.g. xrelease=debug,reqwest=warn)
                 # blank/invalid XRELEASE_LOG keeps this value / falls back to info

[api]
listen = "0.0.0.0:8080"   # required in containers/K8s (not 127.0.0.1)

[config_api]
api_config = false
source = "local"
# ui_config = false
```

Docker Compose lab uses **API + UI** (ledger + `[[organizations]]`) — see the
repo-root [`bootstrap.toml`](../../bootstrap.toml).

## Desired-state example

Paste into UI Apply, or save as `app/releases.yaml` for GitOps. Full sample:
[`app/releases.example.yaml`](../../app/releases.example.yaml).

```yaml
defaults:
  interval_secs: 900

# Built-ins: wildcard, any-stable, semver, semver-v, numeric, major-minor,
# calver, semver-pre, docker-semver, prerelease, stable — no need to declare them.
# Optional custom presets override a built-in or add new names:
# presets:
#   weekly-security:
#     pattern: '^v?\d+\.\d+\.\d+$'
#     routing_tag: security-team

notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls: []

sources:
  - type: github
    repo: owner/repo
    preset: semver-v
```

Field details: [Sources](sources.md) (21 integrations),
[Notifications](apprise.md) (sinks · templates · secrets · YAML / UI),
[API](api.md). Secrets template: [`.env.example`](../../.env.example).

Env overlays for infra/secrets (`XRELEASE_DATABASE_URL`, `XRELEASE_API_KEY`,
`XRELEASE_LOG`, notifier `*_env`, optional `XRELEASE_APPRISE_ENDPOINT`) apply
**after** the desired document is loaded. Apprise **targets** are document-only —
see [Notifications — recommended authoring](apprise.md#recommended-authoring-model).

## Precedence

```
[config_api].source = api  + applied revision  →  PostgreSQL config_revision
[config_api].source = local (or no revision)   →  app/releases.yaml
bootstrap.toml                                 →  infra only (never app sections)
environment variable                           →  overlays file / ledger values
```

Putting application sections (`sources`, `notifiers`, `teams`, `presets`, …) in
`bootstrap.toml` is a **hard error**. Infra sections (and `[[organizations]]`)
in a pushed desired document are also rejected.

## Organizations (multi desired files)

When `[[organizations]]` is declared in bootstrap, each entry’s optional `app`
file is loaded (required for `source = local`; optional seed for `source = api`),
routing tags / source ids are namespaced as `{id}::…`, and the process runs a
single merged poller. Omit `[[organizations]]` for a single `app/releases.yaml`
(or single ledger stream when `source = "api"`).

With `source = "api"`, an org may omit `app` entirely — the instance boots with
an empty placeholder (`desired_source = empty`) until the first UI / `xrctl`
apply writes the ledger stream.

With `source = "local"` (typical GitOps multi-org), edit each org YAML in Git
and reload. With `source = "api"`, apply stores documents in PostgreSQL
(`config_revision`). Example layout:
[`deploy/examples/multi-org/`](../../deploy/examples/multi-org/).

## How CLI, API, UI, and Postgres interact

Recommended **Local** loop: commit `app/releases.yaml` → CI
`validate --strict` → deploy ConfigMap → `reload` / rollout.

| Action | Local CLI (`xrelease`) | API / `xrctl` | UI | Postgres |
|---|---|---|---|---|
| Lint desired + infra | `validate [--strict]` | `POST /config/validate` | Validate | Optional |
| List sources | `sources` | `GET /sources` | Sources pages | Runtime state |
| Hot-swap (**Local**) | edit + restart / SIGHUP | `POST /reload` | — | Unchanged when `source=local` |
| Hot-swap (**API** / **API + UI**) | — | `POST /config/apply` | `/config` editor (**API + UI**) | Appends `config_revision` |
| Rollback | — | `POST /config/rollback` | Revisions UI | Re-applies previous row |
| Poll / notify state | `serve` | webhooks + outbox | Outbox / status | `seen_release`, outbox |

## Source presets

**Built-in** (always on): `wildcard`, `any-stable`, `semver`, `semver-v`, `numeric`,
`major-minor`, `calver`, `semver-pre`, `docker-semver`, `prerelease`, `stable` —
see [Sources](sources.md#built-in-presets). Prefer these for filter policy; keep
`routing_tag` on the source (teams are deployment-specific).

**Custom** blocks under `presets` share filters, schedule, and optional
`routing_tag`. Per-source fields always win; unset fields inherit. A custom
entry with a built-in name replaces that built-in.

```yaml
sources:
  - type: docker
    image: library/nginx
    preset: numeric
    routing_tag: platform-team
  - type: ghcr
    image: org/app
    preset: numeric
    interval_secs: 1800   # override interval only
```

Unknown `preset` names fail validation / watch build. Unused *user* presets warn
(`validate --strict` promotes warnings to errors); unused built-ins do not.
