# Provider categories

Category map of the 21 source `type` values. Required fields per type and
shared filters: [Sources reference](../configuration/sources.md).

## Git forges

| type | Upstream |
|---|---|
| `github` | GitHub Releases API |
| `gitlab` | GitLab Releases API |
| `gitea` | Gitea / Forgejo releases |
| `codeberg` | Codeberg (Gitea-compatible) |
| `bitbucket` | Bitbucket tag pushes |

Webhooks are supported for GitHub, GitLab, Gitea/Codeberg, Bitbucket, and Docker Hub
when running `xrelease serve` — see [Webhooks](../api/webhooks.md).

## Container registries

| type | Upstream |
|---|---|
| `docker` | Any OCI registry (set `registry` URL) |
| `ghcr` | GitHub Container Registry |
| `quay` | Quay.io |
| `ecr` | AWS ECR Public gallery |

## Package indexes

| type | Ecosystem |
|---|---|
| `pypi` | Python (PyPI) |
| `npm` | JavaScript (npm) |
| `yarn` | Yarn registry |
| `cargo` | Rust (crates.io) |
| `maven` | JVM (Maven Central) |
| `nuget` | .NET (NuGet.org) |
| `hex` | Elixir (Hex.pm) |
| `rubygems` | Ruby (RubyGems.org) |
| `packagist` | PHP (Packagist) |
| `cpan` | Perl (MetaCPAN) |

Package kinds (except `cpan`) can optionally receive [OSV advisory
enrichment](../configuration/overview.md#security-advisories) when
`[advisories]` is enabled in bootstrap.

## Other

| type | Upstream |
|---|---|
| `feed` | RSS / Atom / JSON Feed |
| `artifacthub` | Artifact Hub (Helm charts by default) |

## Shared source fields

Every entry under `sources` in the desired document supports:

- `pattern` / `exclude_pattern` — regex filters on version tags
- `include_prerelease` / `prerelease_tags` — pre-release handling
- `routing_tag` — route notifications to a team
- `interval_secs` / `jitter_secs` — per-source poll schedule override

Examples: [`app/releases.example.yaml`](../../app/releases.example.yaml).
