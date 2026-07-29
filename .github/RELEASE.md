# Build, publish & runners

Maintainer guide for **this** repository’s GitHub Actions (not end-user apply
docs — those stay in [`docs/operations/ci-cd.md`](../docs/operations/ci-cd.md)).

## Pipeline map

```
PR / push main
  │
  ├─ ci.yml                         quality gates (no publish)
  │    version-sync                 bump-version.py --check (fail-fast)
  │      ├ rust                     fmt / clippy / test + validate
  │      ├ front                    check / test / build
  │      ├ security                 cargo-deny + npm audit + OSV + Trivy
  │      ├ helm                     lint + template + package
  │      ├ compose                  compose config
  │      └ docker ◄─ rust+front     amd64 smoke (GHA cache → release / e2e)
  │    dependency-review (PR only)  GitHub Dependency Graph (new vulns in PR)
  │
  ├─ k8s-e2e.yml                    kind + Helm (paths: deploy/, docker/, …)
  ├─ docs.yml                       mdBook → Pages (docs/**)
  └─ codeql.yml                     SAST (gated on CODE_SCANNING_ENABLED)

manual     ──► version-bump.yml     bump semver → commit + tag vX.Y.Z
tag v*.*.* ──► release.yml          binaries + GHCR + OCI Helm + GitHub Release
```

| Workflow | Trigger | Publishes? |
|---|---|---|
| `ci.yml` | PR, push `main` | No |
| `k8s-e2e.yml` | PR/push when deploy/docker/ci helm paths change | No |
| `docs.yml` | `docs/**` | GitHub Pages |
| `codeql.yml` | main + weekly | No (SARIF) |
| `version-bump.yml` | manual on `main` | Git commit + tag |
| `release.yml` | tag `v*.*.*` or manual | **GHCR** + **OCI chart** + **Release** |

### Why jobs are separate (not redundant)

| Split | Reason |
|---|---|
| `ci` docker vs `k8s-e2e` | e2e only when chart/images change; rebuilds with same GHA cache scopes |
| `ci` helm vs `k8s-e2e` | helm = render/validate without cluster; e2e = runtime |
| `security` vs `dependency-review` | lockfiles on every run vs **diff of what the PR introduces** |
| `security` vs CodeQL | advisories/lockfiles vs SAST source analysis |
| `release` docker vs `ci` docker | tag ref + push/sign; ci only warms cache / smoke |

Do **not** chain `release` → wait for `ci` on the same commit: the tag points at an
already-merged, CI-green `main` tip after `version-bump`.

## Publish a release (happy path)

Do this **in order** — do not invent a tag by hand unless recovering.

1. Merge to `main`, wait for green `ci`.
2. Actions → **version-bump** → Run workflow → branch `main` → **patch** / **minor** / **major**.
3. Workflow bumps Cargo, OpenAPI, front (`package.json` + **`schema.d.ts` via `gen:api`**),
   Helm `Chart.yaml` / `appVersion`, compose image tags, and pinned `image.tag` in
   `deploy/k8s/values.yaml`; commits
   `chore: release vX.Y.Z`; pushes tag `vX.Y.Z`.
4. **release** starts from the tag (needs `RELEASE_TOKEN`) and publishes:
   - Linux **amd64** archives (`xrelease`, `xrctl`)
   - Images: `ghcr.io/<owner>/xrelease`, `-cli`, `-ui` (**linux/amd64**, SBOM + cosign)
   - Helm chart: `oci://ghcr.io/<owner>/charts/xrelease:<ver>` (cosign) + `.tgz` on the Release
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
```

### Auto-trigger after version-bump

Pushes with the default `GITHUB_TOKEN` **do not** start new workflows. Add repo
secret **`RELEASE_TOKEN`**:

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
| Pages → Source | GitHub Actions |
| Packages | Actions can write to GHCR |
| Dependency graph | On (needed for `dependency-review`) |
| Optional secret `RELEASE_TOKEN` | Tag → release cascade |
| Optional variable `ACTIONS_RUNS_ON` | Runner labels (below) |
| Optional variable `CODE_SCANNING_ENABLED=true` | Turns on `codeql.yml` analyze |
| Code scanning | **Advanced** setup only — disable Default setup |

### Workflow triggers

| Workflow | When |
|---|---|
| `ci.yml` | Every PR + push to `main` |
| `k8s-e2e.yml` | PR + push `main` when `deploy/**`, `docker/**`, `.github/ci/**`, or `front/nginx.conf` change |
| `docs.yml` | `docs/**` (or manual); deploy only on `main` |
| `codeql.yml` | PR + push `main` + weekly; analyze gated on `CODE_SCANNING_ENABLED` |
| `version-bump.yml` | Manual, `main` only |
| `release.yml` | Tag `v0.1.0` / `v1.2.3` (not `v.0.1.0`) or manual |

Do **not** make `codeql` / `analyze` required while scanning is off — only `gate` runs.

## Dependency scanning

| Layer | What | When |
|---|---|---|
| `dependency-review` | PR Dependency Graph diff | PRs only; fail on **high+** |
| `cargo-deny` | Rust advisories, bans, licenses, sources | Every CI |
| `npm audit` | Front lockfile (`--package-lock-only`) | Every CI; fail on **high+** |
| OSV Scanner | `Cargo.lock` + `front/package-lock.json` | Every CI |
| Trivy | Secrets + IaC misconfig (not vuln — covered above) | Every CI |
| Dependabot | Weekly PRs | cargo / npm / actions / docker |
| CodeQL | SAST | Separate workflow |

## Runners: GitHub-hosted vs self-hosted

Default: **`ubuntu-latest`**. Optional repo variable `ACTIONS_RUNS_ON` as JSON
array (e.g. `["self-hosted","linux","x64"]`).

Self-hosted needs Docker/Buildx, disk/RAM for Rust+images, outbound HTTPS.

## Dependabot

Config: [`.github/dependabot.yml`](dependabot.yml). Weekly grouped PRs for
**cargo**, **npm** (`/front`), **github-actions**, **docker** (`/docker`).

Dependabot **does not** bump the product version — that stays on
`version-bump` → `release`.

### Merge rules

```
Dependabot PR
  │
  ├─ CI green? ── no ──► rebase / fix / close majors that need hand-bump
  └─ yes ──► squash-merge (no version-bump)
         ├ routine deps ──► next planned version-bump
         └ security / runtime base ──► green main → version-bump **patch** → release
```

**Never merge with red `ci`.**

| PR type | Accept when | Notes |
|---|---|---|
| `rust-dependencies` | `rust` + `security` green | Majors need changelog + local deny/test |
| `npm-dependencies` | `front` + `security` green | `typescript >=7` ignored |
| `github-actions` | `ci` green | `dtolnay/rust-toolchain` ignored |
| docker (nginx, node, …) | `docker` smoke green | OK |
| docker `rust:*` | **manual only** | Bump with toolchain.toml + workflows together |

Not covered by Dependabot: `postgres:*` and `caronc/apprise` in compose / Helm.

```bash
gh pr comment <N> --body "@dependabot rebase"
gh pr comment <N> --body "@dependabot recreate"
```

## Artifacts consumers use

| Artifact | Use |
|---|---|
| `ghcr.io/…/xrelease:<ver>` | Backend |
| `ghcr.io/…/xrelease-ui:<ver>` | Dashboard |
| `ghcr.io/…/xrelease-cli:<ver>` | CI apply (`xrctl`) |
| `oci://ghcr.io/…/charts/xrelease:<ver>` | Helm chart |
| `xrelease-<ver>.tgz` | Chart on the Release |
| `xrelease-*-linux-*.tar.gz` / `xrctl-*-linux-*.tar.gz` | Binaries |

Operators: Compose / Helm — [`deploy/README.md`](../deploy/README.md),
[`docs/getting-started/kubernetes.md`](../docs/getting-started/kubernetes.md).
Apply from consumer CI: [`docs/operations/ci-cd.md`](../docs/operations/ci-cd.md).
