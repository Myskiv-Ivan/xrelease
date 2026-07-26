# Sources reference

Integrations for **what to watch** — 21 `type` values under `sources:` in the
desired document. Grouped overview: [Provider categories](../concepts/providers.md).
Canonical examples: [`app/releases.example.yaml`](../../app/releases.example.yaml)
· [`deploy/examples/`](../../deploy/examples/).

## Supported `type` values (21)

| Category | type | Watches |
|---|---|---|
| Git | `github` | GitHub releases |
| Git | `codeberg` | Codeberg releases |
| Git | `gitea` | Self-hosted Gitea/Forgejo |
| Git | `gitlab` | GitLab releases |
| Git | `bitbucket` | Bitbucket Cloud / Server tags |
| Containers | `docker` | Any OCI registry (custom URL) |
| Containers | `ghcr` | GitHub Container Registry |
| Containers | `quay` | Quay.io |
| Containers | `ecr` | AWS ECR Public gallery |
| Packages | `pypi` | Python packages (PyPI) |
| Packages | `npm` | npm packages |
| Packages | `yarn` | Yarn registry (npm-compatible API) |
| Packages | `cargo` | Rust crates (crates.io) |
| Packages | `maven` | Maven Central artifacts |
| Packages | `nuget` | NuGet.org packages |
| Packages | `hex` | Hex.pm (Elixir) packages |
| Packages | `rubygems` | RubyGems.org gems |
| Packages | `packagist` | Packagist (PHP) packages |
| Packages | `cpan` | Perl CPAN (MetaCPAN) |
| Other | `feed` | RSS / Atom / JSON Feed |
| Other | `artifacthub` | Artifact Hub catalog (Helm by default) |

Polling covers all kinds. **Webhook push** (with `xrelease serve`) is available
for GitHub, GitLab, Gitea/Codeberg, Bitbucket, Docker Hub, and a generic
endpoint — see [Webhooks](../api/webhooks.md).

## Type-specific fields

Every source also accepts shared filter/schedule fields ([below](#shared-fields-sourcecommon))
and optional `id` / `token` / `token_env` where noted.

| type | Required | Optional | Notes |
|---|---|---|---|
| `github` | `repo` (`owner/repo`) | `token`, `token_env` | GitHub.com |
| `codeberg` | `repo` | `token`, `token_env` | Codeberg.org |
| `gitea` | `host`, `repo` | `token`, `token_env` | Self-hosted Gitea / Forgejo |
| `gitlab` | `project` | `host`, `token`, `token_env` | Default host `https://gitlab.com` |
| `bitbucket` | `repo` | `edition` (`cloud`/`server`), `host`, `token`, `token_env` | Server edition needs `host` |
| `docker` | `image` | `registry`, `token`, `token_env` | Default registry = Docker Hub |
| `ghcr` / `quay` / `ecr` | `image` | `token`, `token_env` | Registry URL is implicit |
| `pypi` / `npm` / `yarn` / `cargo` / `nuget` / `hex` / `rubygems` / `packagist` / `cpan` | `name` | — | Package / crate / gem id |
| `maven` | `name` (`group:artifact`) | — | Warns if `:` is missing |
| `feed` | `url` | — | RSS / Atom / JSON Feed |
| `artifacthub` | `name` (`repo/chart`) | `host`, `package_kind` | Default kind `helm`; host `https://artifacthub.io` |

```yaml
sources:
  - type: github
    repo: tokio-rs/tokio
    preset: semver-v
  - type: gitea
    host: https://git.example.com
    repo: platform/service
    token_env: GITEA_TOKEN
  - type: gitlab
    project: group/project
  - type: docker
    image: library/nginx
    preset: numeric
  - type: maven
    name: org.apache.commons:commons-lang3
  - type: artifacthub
    name: bitnami/nginx
```

Live field catalogue: `GET /api/v1/config/schema` (and `xrctl schema`).

## Shared fields (`SourceCommon`)

Every source accepts (via flatten) and may inherit from a named **preset**:

| Field | Meaning |
|---|---|
| `preset` | Built-in name or entry under top-level `presets` (source fields override) |
| `routing_tag` | Team routing — matches notifier `tags` / `[[teams]].tag` |
| `interval_secs` / `jitter_secs` | Poll schedule (else `defaults`) |
| `poll_on_startup` | Immediate first poll |
| `notify_schedule` | Crontab deferral; empty string opts out of default |
| `pattern` / `exclude_pattern` | Tag regex filters |
| `include_prerelease` / `prerelease_tags` | Pre-release policy |
| `exclude_updated` | Ignore body/URL edits on seen tags |

## Built-in presets

Always available — no need to declare them under `presets:`. A same-named
user entry **replaces** the built-in entirely. Listed by
`GET /api/v1/config/schema` → `builtin_presets` / `preset_names`.

| Name | Effect |
|---|---|
| `wildcard` | All tags including pre-releases (no pattern) |
| `any-stable` | All non-prerelease tags (no pattern) |
| `semver` | Stable tags `v1.2.3` or `1.2.3` (`^v?\d+\.\d+\.\d+$`) |
| `semver-v` | Stable tags that require a leading `v` |
| `numeric` | Numeric tags without `v` — Docker / PyPI style (`1.2.3`) |
| `major-minor` | `v1.2` / `1.2` only |
| `calver` | Calendar versioning `YYYY.M.D` / `YYYY.MM.DD` |
| `semver-pre` | Semver including `-rc.1` / build metadata |
| `docker-semver` | Numeric semver; exclude `latest` / `nightly` / `edge` |
| `prerelease` | `alpha` / `beta` / `rc` channels only |
| `stable` | Like `semver`, plus `exclude_updated` (ignore body/URL churn) |

```yaml
sources:
  - type: github
    repo: org/service-a
    preset: semver-v
    routing_tag: platform-team
  - type: docker
    image: library/nginx
    preset: numeric
    routing_tag: platform-team
```

## Custom presets

Same fields as `SourceCommon` (except nesting another `preset`). A user entry
with a built-in name replaces that built-in.

```yaml
presets:
  weekly-security:
    pattern: '^v?\d+\.\d+\.\d+$'
    routing_tag: security-team
    interval_secs: 3600

sources:
  - type: github
    repo: org/service-a
    preset: weekly-security
  - type: github
    repo: org/service-b
    preset: weekly-security
    interval_secs: 1800   # override
```

Example blocks: [`app/releases.example.yaml`](../../app/releases.example.yaml).  
Machine-readable field list: `GET /api/v1/config/schema` → `source_common_fields`.
