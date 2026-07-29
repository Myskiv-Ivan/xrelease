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

Valid flags: `api_config`, `ui_config`, and `source = "local" | "api"` only.

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

## Security advisories

Optional enrichment via bootstrap `[advisories]`: when a **package** release is
delivered, the notification can list known CVEs / GHSAs for that exact version.
**Disabled by default** in the binary (omit the table or set `enabled = false`).
The Compose lab [`bootstrap.toml`](../../bootstrap.toml) turns it **on**; Helm
`bootstrapInline` does not — add `[advisories]` there if you want enrichment.

```toml
[advisories]
enabled = true
# endpoint = "https://api.osv.dev"   # self-hosted OSV mirror avoids third-party disclosure
# timeout_secs = 5
# cache_ttl_secs = 3600
# breaker_threshold = 5
# breaker_cooldown_secs = 300
# sweep_interval_secs = 3600   # background sweep; 0 disables it
# sweep_batch = 10             # versions per source per round; 0 disables it
```

> **Privacy.** Enabling this sends watched package names and versions to the
> configured OSV endpoint. Use a self-hosted mirror if that disclosure is
> unacceptable.

| Covered | Not queried |
|---|---|
| `pypi`, `npm`, `yarn` (as npm), `cargo`, `maven`, `nuget`, `hex`, `rubygems`, `packagist` | `cpan`, Git forges, containers, Artifact Hub, feeds |

**Why containers are not covered.** A `docker` / `ghcr` / `quay` / `ecr` source
reads `GET /v2/<image>/tags/list` — a list of tag *names* and nothing else. A
tag is not a package coordinate, so there is nothing to ask OSV about. Finding
CVEs in an image means resolving the tag to a manifest, pulling its layers or
an attached SBOM, enumerating the OS and language packages inside, and querying
each one (OSV does index `Debian:*`, `Alpine:*`, `Ubuntu:*` …). That is image
scanning — Trivy / Grype / Docker Scout territory — not something xrelease does,
and guessing an ecosystem from a tag name would produce confident nonsense. The
dashboard therefore hides the severity column entirely for container sources
rather than showing a permanently empty one.

Enrichment never blocks delivery: timeouts, HTTP errors, or an open circuit
breaker leave the notification unenriched. Digest messages are not enriched
(several releases in one message). Withdrawn advisories are omitted. A
notification lists at most eight findings (`…and N more`); the full set is in
the dashboard.

**Dashboard:** severity chips on a source’s release table (`C` / `H` / `M` /
`L` / `I` — `I` = severity unknown) and **Advisories** on the source page
(`/sources/{id}/advisories`). Cached findings also appear on
`GET /api/v1/sources/{id}` (not on the sources list). Retention:
[`prune_advisories_after_days`](../operations/scaling.md#postgresql-growth-control),
which covers both the findings and the check ledger below.

### Which releases actually get looked up

Not every release, and not all at once — the lookups are bounded so neither a
delivery nor a page load can stall on a third party.

| Path | Scope | Bound |
|---|---|---|
| Delivery enrichment | Only versions that produced a notification | Baseline (first) polls notify nothing, so their versions are never enriched this way; digests are skipped |
| **Background sweep** | **Every synced release of every watched package source** | `sweep_batch` (10) versions per source per round, every `sweep_interval_secs` (1 h) |
| Source-detail page | Newest **200** synced releases | **5** OSV lookups per request, run concurrently |
| Repeat lookups | — | In-process cache for `cache_ttl_secs`; results and "checked, nothing found" both persisted |

Every verified answer — including a clean one — is recorded, so each pass
resumes where the last stopped instead of re-confirming the same newest
releases. A lookup that never reached OSV (disabled, breaker open, timeout) is
deliberately *not* recorded, so an outage cannot silently mark a version clean.

### Background sweep

Delivery-time enrichment only covers versions that actually notified, and a
source's first poll is a silent baseline — so without a sweep, everything a
source was already publishing when you added it stays unchecked unless a human
opens its detail page. The sweep closes that gap on its own.

- **On by default** whenever `[advisories] enabled = true`. Enabling enrichment
  already discloses the watched package names; the sweep only adds more
  versions of those same packages.
- Walks sources one at a time, `sweep_batch` versions of each concurrently, so
  peak in-flight requests stay at `sweep_batch` no matter how many sources are
  configured.
- First round starts 60 s after boot, so it does not pile onto startup polling.
- Converges and then costs nothing: a source with 200 synced releases is fully
  covered in under a day, after which each round reads one indexed anti-join
  per source and returns no work.
- Follows the watch list — a config apply restarts it against the new set, and
  a source with no OSV coordinate (container, Git forge, feed) never enters it.
- Self-limiting during an OSV outage: once the circuit breaker opens, the rest
  of the round short-circuits without touching the network and records nothing.

Set either `sweep_interval_secs = 0` or `sweep_batch = 0` to turn it off.

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
