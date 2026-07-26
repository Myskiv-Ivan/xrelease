# Contributing to xrelease

Thanks for your interest in improving xrelease! This guide covers the local
setup, quality gates, and the release flow.

## Development setup

```sh
# Backend (Rust)
cargo build
cargo test                    # tests needing Postgres self-skip when it is absent

# Frontend (SvelteKit)
cd front
npm ci
npm run check
npm run build
```

A local PostgreSQL speeds up store/scheduler tests:

```sh
docker run --rm -e POSTGRES_USER=xrelease -e POSTGRES_PASSWORD=xrelease \
  -e POSTGRES_DB=xrelease_test -p 5432:5432 postgres:18-alpine
```

Full stack via Docker (development build):

```sh
cp .env.example .env
docker compose -f docker/docker-compose.dev.yaml up -d --build
```

Operators use published images: `docker compose up -d` (see [`docker/README.md`](docker/README.md)).

## Quality gates (run before opening a PR)

| Area | Command |
|---|---|
| Format | `cargo fmt --all` |
| Lint | `cargo clippy --all-targets --all-features -- -D warnings` |
| Tests | `cargo test --all-features` |
| Config | `cargo run -- --config deploy/examples/infra-app/bootstrap.toml --app deploy/examples/infra-app/app/releases.yaml validate --strict` |
| Frontend | `cd front && npm run check && npm run build` |
| Docs | `mdbook build docs` (optional locally; CI publishes to GitHub Pages) |
| Version sync | `python3 scripts/bump-version.py --check` |

CI runs these plus `cargo-deny`, Trivy, and CodeQL. Docs deploy via
[`.github/workflows/docs.yml`](.github/workflows/docs.yml) →
https://myskiv-ivan.github.io/xrelease/.

## Commit & PR conventions

- Conventional Commits: `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`.
- Keep PRs focused; include rationale (the "why") in the description.
- Update user docs under `docs/` when behaviour changes (mdBook → GitHub Pages).
- Note user-facing changes in [`CHANGELOG.md`](CHANGELOG.md) under **Unreleased**.
- Never commit secrets — real Apprise URLs and tokens live in `.env` (gitignored).

## Adding a release source

See existing implementations under `src/sources/` and [Sources reference](docs/configuration/sources.md).
Add a source example to `app/releases.yaml` and tests where practical.

## Release (maintainers)

1. On `main`: Actions → **version-bump** → patch / minor / major.
2. It syncs Cargo, OpenAPI, front, Helm chart, compose files, commits
   `chore: release vX.Y.Z`, and pushes tag `vX.Y.Z`.
3. **release** builds Linux binaries, multi-arch GHCR images (signed + SBOM),
   and publishes the GitHub Release.

For step 3 to start automatically after version-bump, add repo secret
**`RELEASE_TOKEN`** (PAT with Contents write). Without it, run **release**
manually (Actions → release → tag `vX.Y.Z`).

Workflow map: `.github/workflows/`. End-user apply-from-CI examples live in
[`docs/operations/ci-cd.md`](docs/operations/ci-cd.md) (published user docs —
not the maintainer release checklist).

## License

By contributing you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE).
