# Build, publish & runners

Maintainer guide for **this** repository’s GitHub Actions (not end-user apply
docs — those stay in [`docs/operations/ci-cd.md`](../docs/operations/ci-cd.md)).

## Pipeline map

```
PR / push main ──► ci.yml        quality gates (no publish)
                   ├ rust        fmt/clippy/test/version/validate
                   ├ security    cargo-deny + npm audit (+ Trivy advisory)
                   ├ front       check/test/build
                   ├ helm        lint + template
                   └ docker      amd64 image smoke (no QEMU)
push main      ──► docs.yml      mdBook → GitHub Pages
push/PR main   ──► codeql.yml    SAST (gated on private repos)
manual         ──► version-bump  bump semver → commit + tag vX.Y.Z
tag v*.*.*     ──► release.yml   binaries + GHCR (cosign) + GitHub Release
```

| Workflow | Trigger | Publishes? |
|---|---|---|
| `ci.yml` | PR, push `main` | No (rust+validate / security / front / helm / amd64 image smoke) |
| `docs.yml` | docs paths | GitHub Pages |
| `codeql.yml` | main + weekly | No |
| `version-bump.yml` | manual on `main` | Git commit + tag only |
| `release.yml` | tag `v*.*.*` or manual | **GHCR** + **GitHub Release** assets |

## Publish a release (happy path)

1. Merge to `main`, wait for green `ci`.
2. Actions → **version-bump** → Run workflow → branch `main` → patch / minor / major.
3. Workflow bumps Cargo, OpenAPI, front, Helm `Chart.yaml`, compose image tags,
   commits `chore: release vX.Y.Z`, pushes tag `vX.Y.Z`.
4. **release** builds:
   - Linux amd64/arm64 archives (`xrelease`, `xrctl`)
   - Multi-arch images: `ghcr.io/<owner>/xrelease`, `-cli`, `-ui` (SBOM + cosign)
   - GitHub Release with notes + checksums

### Auto-trigger after version-bump

Pushes authenticated with the default `GITHUB_TOKEN` **do not** start new
workflows. Add repo secret **`RELEASE_TOKEN`**:

| Kind | Scope |
|---|---|
| Fine-grained PAT | Contents: Read and write on this repo |
| Classic PAT | `repo` |

version-bump checks out with `secrets.RELEASE_TOKEN || secrets.GITHUB_TOKEN`.
With the PAT, the tag push starts `release.yml`.

Without `RELEASE_TOKEN`: Actions → **release** → Run workflow → tag `vX.Y.Z`.

### Repo settings checklist

| Setting | Value |
|---|---|
| Actions enabled | On |
| Workflow permissions | Read and write (Actions → General) |
| Pages → Source | GitHub Actions (`docs.yml` can `enablement: true`) |
| Packages | Actions can write to GHCR |
| Optional secret `RELEASE_TOKEN` | Tag → release cascade |
| Optional variable `ACTIONS_RUNS_ON` | Runner labels (below) |
| Optional variable `CODE_SCANNING_ENABLED=true` | Required for CodeQL on **private** repos (needs GHAS) |
| Code scanning | Settings → Code security → Code scanning (GHAS if private) |

### Workflow triggers (rules)

| Workflow | When it runs |
|---|---|
| `ci.yml` | Every PR + push to `main` |
| `docs.yml` | Changes under `docs/**` (or manual); deploy only on `main` push / dispatch |
| `codeql.yml` | PR + push `main` + weekly Mon; **skipped** on private until `CODE_SCANNING_ENABLED` |
| `version-bump.yml` | Manual, only if ref is `main` |
| `release.yml` | Tag `v0.1.0` / `v1.2.3` (not `v.0.1.0`) or manual with that tag |

Do **not** make `codeql` a required status check while the repo is private without GHAS —
the analyze job is skipped and only `gate` succeeds.

## Runners: GitHub-hosted vs self-hosted

Default in all workflows: **`ubuntu-latest`** (GitHub-hosted).
No self-hosted runner is required.

### GitHub-hosted (recommended)

- No install, billed as Actions minutes
- Docker + Buildx available for `ci` / `release` image jobs
- Enough for this project’s Linux + QEMU multi-arch builds

Leave variable `ACTIONS_RUNS_ON` unset (or set `["ubuntu-latest"]`).

### Self-hosted (on-prem runner)

Use when you need private network, custom hardware, or to avoid hosted minutes.

1. Repo or org → **Settings → Actions → Runners → New self-hosted runner**.
2. Install the agent for your OS; register with the one-time token.
3. Labels (typical): `self-hosted`, `linux`, `x64`.
4. Set repository **variable** (Settings → Secrets and variables → Actions →
   Variables):

   | Name | Value (JSON array) |
   |---|---|
   | `ACTIONS_RUNS_ON` | `["self-hosted","linux","x64"]` |

   Workflows resolve:

   ```yaml
   runs-on: ${{ fromJSON(vars.ACTIONS_RUNS_ON || '["ubuntu-latest"]') }}
   ```

5. Requirements on the machine:

   | Need | Why |
   |---|---|
   | Docker + Buildx | `ci` / `release` image builds |
   | Disk / RAM | Rust release + multi-arch QEMU |
   | Outbound HTTPS | crates.io, npm, GHCR, cosign Fulcio |
   | Linux amd64 (or matching labels) | Job images assume Linux |

Do not commit registration tokens. Rotate the runner if compromised.
Prefer GitHub-hosted unless you have a concrete need.

To switch back: delete `ACTIONS_RUNS_ON` or set `["ubuntu-latest"]`.

## Manual release (re-run / recover)

```text
Actions → release → Run workflow → tag = vX.Y.Z
```

Tag must already exist and match `Cargo.toml` / chart / compose versions
(run version-bump first if not).

## Artifacts consumers use

| Artifact | Use |
|---|---|
| `ghcr.io/…/xrelease:<ver>` | Backend |
| `ghcr.io/…/xrelease-ui:<ver>` | Dashboard |
| `ghcr.io/…/xrelease-cli:<ver>` | CI apply (`xrctl`) |
| `xrelease-*-linux-*.tar.gz` / `xrctl-*-linux-*.tar.gz` | Binary installs |

Operators: `docker compose up -d` / Helm — see [`deploy/README.md`](../deploy/README.md).
Apply from consumer CI: [`docs/operations/ci-cd.md`](../docs/operations/ci-cd.md).
