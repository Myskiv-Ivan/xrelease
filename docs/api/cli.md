# `xrctl` — management CLI

`xrctl` drives a **running** `xrelease serve` over its HTTP API. It is a pure
API client: it opens no database, reads no local server config, and never writes
desired state to disk (apply goes to the API ledger). That makes it safe to run
from a laptop or a CI runner — you only need the API URL and (if set) the API key.

It ships as a **separate binary** from the backend (`xrelease-*.tar.gz` and
`xrctl-*.tar.gz`), so operators can install the CLI without the full backend.

## Where does `xrctl` connect?

`xrctl` talks to the **management HTTP API** of a running `xrelease serve`.
There is no separate CLI port — use the same base URL you would for
`curl` / the dashboard proxy.

| How you run the server | Base URL for `xrctl` | Notes |
|---|---|---|
| Native `xrelease serve` | `http://127.0.0.1:8080` | Default `[api].listen` |
| Docker Compose UI stack | `http://127.0.0.1:3000` | Host publishes nginx on **:3000**; backend `:8080` is **not** on the host |
| Kubernetes + UI Ingress | `https://your-host` | Ingress → UI → proxy → backend `:8080` |
| Kubernetes API-only Ingress | `https://hooks-host` | Direct to the backend Service |

Pass `--api-url` (and `--api-key` when the server has `[api].api_key`). Trailing
slashes are stripped. Connection settings are **CLI flags only** — xrctl does
not read `XRELEASE_API_*` from the environment (those env vars configure the
**backend**, not the client).

```sh
# Native binary (default — you can omit --api-url):
xrctl status
# same as:
xrctl --api-url http://127.0.0.1:8080 status

# Docker Compose (UI on :3000):
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" status
```

If connection fails with “connection refused”, you are almost always pointing at
the wrong port (Compose users: use **3000**, not 8080).

## CI/CD (separate from the server)

`xrctl` is meant to run **outside** the backend process — a CI job, a laptop, or
another cluster — against an already-running `serve`. It never opens Postgres
and never loads `bootstrap.toml`.

Ship it as:

| Artifact | When |
|---|---|
| Binary `xrctl-*.tar.gz` | [GitHub Releases](https://github.com/Myskiv-Ivan/xrelease/releases) |
| Image `ghcr.io/myskiv-ivan/xrelease-cli` | Same release tags as the backend |

```sh
# Apply desired state from CI (mount the repo; pass URL/key as args)
docker run --rm \
  -v "$PWD:/work" -w /work \
  ghcr.io/myskiv-ivan/xrelease-cli:latest \
  xrctl --api-url https://xrelease.example.com \
  --api-key "$XRELEASE_API_KEY" \
  apply app/releases.yaml --if-match none --label "$CI_COMMIT_SHA"
```

Prefer the image as the **job image** (GitLab) so you call `xrctl` with no
nested Docker — [CI/CD integration](../operations/ci-cd.md).

GitHub Actions (checkout on the host, then one `docker run`):

```yaml
- name: Apply desired state
  env:
    XRELEASE_API_KEY: ${{ secrets.XRELEASE_API_KEY }}
  run: |
    docker run --rm \
      -v "$PWD:/work" -w /work \
      ghcr.io/myskiv-ivan/xrelease-cli:latest \
      xrctl --api-url https://xrelease.example.com \
      --api-key "$XRELEASE_API_KEY" \
      validate app/releases.yaml
    docker run --rm \
      -v "$PWD:/work" -w /work \
      ghcr.io/myskiv-ivan/xrelease-cli:latest \
      xrctl --api-url https://xrelease.example.com \
      --api-key "$XRELEASE_API_KEY" \
      apply app/releases.yaml --if-match none --label "${GITHUB_SHA}"
```

(The backend image also embeds `xrctl` for `docker exec` convenience; prefer
`xrelease-cli` in CI so the job does not pull the full backend.)

## Connection flags

Every command accepts these global flags:

| Flag | Default |
|---|---|
| `--api-url` | `http://127.0.0.1:8080` |
| `--api-key` | *(none)* |
| `--organization` | *(none)* |
| `--format` | `text` (or `json`) |

`--organization <id>` scopes:

- **config** commands → `/api/v1/organizations/{id}/config/…` (id percent-encoded)
- **`sources` / `outbox`** → `?organization=<id>` (same filter as the dashboard OrgSwitcher)

## Commands

| Command | API call | Purpose |
|---|---|---|
| `xrctl status` | `GET /api/v1/status` | Uptime, sources, outbox depth (incl. deferred), open breakers |
| `xrctl sources` | `GET /api/v1/sources` | Configured sources + live runtime state |
| `xrctl outbox` | `GET /api/v1/outbox` | Pending / failed notifications |
| `xrctl organizations` | `GET /api/v1/organizations` | `[[organizations]]` catalogue + live source counts |
| `xrctl show` | `GET …/config` | Effective + desired config (secrets redacted) |
| `xrctl schema` | `GET /api/v1/config/schema` | Accepted source kinds, available sinks, presets, team tags |
| `xrctl history [--limit N]` | `GET …/config/revisions` | Apply/reject audit ledger (metadata only) |
| `xrctl validate <file>` | `POST …/config/validate` | Dry-run a desired-state document; **exits non-zero if invalid** |
| `xrctl apply <file> [--if-match …] [--label …]` | `POST …/config/apply` | Hot-swap a whole desired-state document |
| `xrctl rollback` | `POST …/config/rollback` | Re-apply the previous applied revision |
| `xrctl reload` | `POST /api/v1/reload` | Re-read desired state from the server's own authority |

`…/config` is `/api/v1/config` by default; with `--organization <id>` the same
commands address `/api/v1/organizations/{id}/config` — one organization's
document and ledger stream. On a multi-org instance the whole-document
`apply`/`rollback` answer `409`; pick the organization instead:

```sh
xrctl organizations
xrctl --organization platform show
xrctl --organization platform sources
xrctl --organization platform apply app/platform/releases.yaml --label "$GIT_SHA"
```

`apply`/`rollback` return `404` unless `[config_api].api_config = true` on the
server (and `source = "api"` — otherwise apply returns `409`). `reload` is for
`source = "local"` instances (single file or every org file); on a
single-document `source = "api"` instance it returns `409` (use `apply`).

Delivery channels (Apprise / SMTP / eXpress) in the applied document interact with
env overlays — see
[Notifications — how UI and CLI share one document](../configuration/apprise.md#how-ui-and-cli-share-one-document).

See [Authoring variants](../configuration/overview.md#authoring-variants)
(**Local** / **API** / **API + UI**).

## Whole-document config authoring

`xrctl` never patches individual fields — there is no "add a source" that would
rewrite YAML behind your back. `apply` submits a **complete desired-state
document** (the same shape as `app/releases.yaml`), the server validates it,
records it in the config ledger, and hot-swaps the runtime. The same whole-
document shape is used by GitOps files, the HTTP API, and the optional UI editor.

```sh
# Gate a change in CI without applying it (non-zero exit fails the job):
xrctl validate app/releases.yaml

# Apply the committed desired state unconditionally (CI has no prior read):
xrctl apply app/releases.yaml --if-match none --label "$GIT_SHA"
```

### Optimistic concurrency (`--if-match`)

`apply` guards against clobbering a concurrent editor via HTTP `If-Match`:

- `--if-match auto` *(default)* — read the current `ETag` first and require it.
  A stale document is rejected with `412`; re-run to pick up the new revision.
  This is the right mode for a human at a terminal.
- `--if-match none` — apply unconditionally. Use in CI, which pushes the
  committed state and has no reason to have read the running config.
- `--if-match <sha>` — require this exact revision.

## JSON output

`--format json` prints the raw API response for scripting; the default `text`
renders a compact human summary. `validate` and `apply` both surface the
server's validation `report` (which source is misrouted, which sink is unknown)
rather than a bare status code.
