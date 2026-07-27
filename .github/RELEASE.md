# Build, publish & runners

Maintainer guide for **this** repository’s GitHub Actions (not end-user apply
docs — those stay in [`docs/operations/ci-cd.md`](../docs/operations/ci-cd.md)).

## Pipeline map

```
PR / push main ──► ci.yml        quality gates (no publish)
                   ├ version-sync  bump-version.py --check (fail-fast)
                   ├ rust          fmt/clippy/test + binary validate
                   ├ security      cargo-deny + npm audit (+ Trivy advisory)
                   ├ front         check/test/build
                   ├ helm          lint + template
                   ├ compose       compose config (parallel)
                   └ docker        matrix amd64 (backend|cli|ui; GHA cache shared w/ release)
push main      ──► docs.yml      mdBook → GitHub Pages
push/PR main   ──► codeql.yml    SAST (gated on private repos)
manual         ──► version-bump  bump semver → commit + tag vX.Y.Z
tag v*.*.*     ──► release.yml   binaries + GHCR (cosign) + GitHub Release
```

| Workflow | Trigger | Publishes? |
|---|---|---|
| `ci.yml` | PR, push `main` | No (version-sync / rust+validate / security / front / helm / compose / amd64 docker matrix) |
| `docs.yml` | docs paths | GitHub Pages |
| `codeql.yml` | main + weekly | No |
| `version-bump.yml` | manual on `main` | Git commit + tag only |
| `release.yml` | tag `v*.*.*` or manual | **GHCR** + **GitHub Release** assets |

## Publish a release (happy path)

Do this **in order** — do not invent a tag by hand unless recovering.

1. Merge to `main`, wait for green `ci`.
2. Actions → **version-bump** → Run workflow → branch `main` → **patch** / **minor** / **major**.
3. Workflow bumps Cargo, OpenAPI, front (`package.json` + **`schema.d.ts` via `gen:api`**),
   Helm `Chart.yaml`, compose image tags; commits `chore: release vX.Y.Z`; pushes tag `vX.Y.Z`.
4. **release** starts from the tag (needs `RELEASE_TOKEN`) and publishes:
   - Linux **amd64** archives (`xrelease`, `xrctl`)
   - Images: `ghcr.io/<owner>/xrelease`, `-cli`, `-ui` (**linux/amd64**, SBOM + cosign)
   - GitHub Release with notes + checksums

### Local equivalent (same order)

```bash
git checkout main && git pull
python3 scripts/bump-version.py --bump patch   # or minor / major
(cd front && npm ci && npm run gen:api)
python3 scripts/bump-version.py --check
VERSION="$(python3 scripts/bump-version.py --print)"   # read-only; does not bump again
git add -u && git commit -m "chore: release v${VERSION}"
git tag "v${VERSION}"
git push origin HEAD "v${VERSION}"
# if no RELEASE_TOKEN: Actions → release → tag = vX.Y.Z
```

Never create `v0.1.0` when Cargo is already `0.1.2` — tag must match `Cargo.toml`
at that commit. Prefer `version-bump` over hand-made tags.

### Recover a wrong tag (tag ≠ Cargo.toml)

If someone pushed `vX.Y.Z` on a commit where versions are still older:

```bash
# Only if no GitHub Release / GHCR artifacts exist for that tag yet
git push origin :refs/tags/vX.Y.Z
git tag -d vX.Y.Z   # local, if present

# After versions on main match X.Y.Z:
git checkout main && git pull
python3 scripts/bump-version.py --check
git tag "vX.Y.Z"
git push origin "vX.Y.Z"
# then release.yml (auto via RELEASE_TOKEN, or manual dispatch)
```

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
- Enough for this project’s Linux **amd64** builds (no QEMU / arm64)

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
   | Disk / RAM | Rust release + Docker amd64 builds |
   | Outbound HTTPS | crates.io, npm, GHCR, cosign Fulcio |
   | Linux amd64 (or matching labels) | Job images assume Linux amd64 |

Do not commit registration tokens. Rotate the runner if compromised.
Prefer GitHub-hosted unless you have a concrete need.

To switch back: delete `ACTIONS_RUNS_ON` or set `["ubuntu-latest"]`.

## Manual release (re-run / recover)

```text
Actions → release → Run workflow → tag = vX.Y.Z
```

Tag must already exist and match `Cargo.toml` / chart / compose versions
(run version-bump first if not).

## Dependabot

Config: [`.github/dependabot.yml`](dependabot.yml). Weekly grouped PRs for
**cargo**, **npm** (`/front`), **github-actions**, **docker** (`/docker`).

Dependabot **does not** bump the product version (`0.x.y`), Helm `appVersion`, or
compose image tags — that stays on `version-bump` → `release`.

### Merge rules

```
Dependabot PR
  │
  ├─ CI green (ci.yml)? ── no ──► read failing job
  │                                ├ `@dependabot rebase` / `recreate`
  │                                ├ small fix commit on the PR branch
  │                                └ major API break → close; bump manually with code
  │
  └─ yes ──► squash-merge (no version-bump)
         │
         ├ routine deps ──► wait for next planned version-bump
         └ security / runtime base (node, nginx, alpine) ──►
               green CI on main → version-bump **patch** → release
```

**Never merge with red `ci`.** Historical docker/cargo PRs landed while `rust` /
`security` were failing — do not repeat that.

### Per ecosystem

| PR type | Accept when | Notes |
|---|---|---|
| `rust-dependencies` | `rust` + `security` green | Majors (`toml` 1.x, `jsonwebtoken` 11, crypto 0.11, …) need changelog + local `cargo test` / `cargo deny check`; close and hand-bump if API breaks |
| `npm-dependencies` | `front` + `npm audit` green | `typescript >=7` ignored (`svelte-check`) |
| `github-actions` | `ci` green | `dtolnay/rust-toolchain` ignored — toolchain = `rust-toolchain.toml` + workflow pin |
| docker (`nginx`, `node`, `alpine`, …) | `docker` smoke green | OK to merge |
| docker `rust:*` | **manual only** | Ignored by Dependabot; bump with `rust-toolchain.toml`, `Cargo.toml` `rust-version`, and workflow `toolchain:` together |

Not covered by Dependabot: `postgres:*` and `caronc/apprise` in compose / Helm
`values.yaml` — review those at release time.

### Commands

```bash
gh pr comment <N> --body "@dependabot rebase"
gh pr comment <N> --body "@dependabot recreate"
gh pr close <N> --comment "Breaking; will bump manually with code changes"
```

After a security or runtime-base merge: wait for green `ci` on `main`, then
Actions → **version-bump** → **patch** (and `RELEASE_TOKEN` / manual **release**).

## Artifacts consumers use

| Artifact | Use |
|---|---|
| `ghcr.io/…/xrelease:<ver>` | Backend |
| `ghcr.io/…/xrelease-ui:<ver>` | Dashboard |
| `ghcr.io/…/xrelease-cli:<ver>` | CI apply (`xrctl`) |
| `xrelease-*-linux-*.tar.gz` / `xrctl-*-linux-*.tar.gz` | Binary installs |

Operators: `docker compose up -d` / Helm — see [`deploy/README.md`](../deploy/README.md).
Apply from consumer CI: [`docs/operations/ci-cd.md`](../docs/operations/ci-cd.md).
