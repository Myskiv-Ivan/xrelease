# Multi-organization example

One shared `bootstrap.toml`, separate desired-state YAML per organization.
This sample is **Local** (GitOps files). The Docker Compose **default** is
different: **API + UI** multi-org with empty orgs until Apply (repo-root
`bootstrap.toml`, no `app` mounts).

| File | Purpose |
|---|---|
| `bootstrap.toml` | Infra + `[[organizations]]` (**Local**) |
| `app/platform/releases.yaml` | Platform sources / teams / sinks |
| `app/security/releases.yaml` | Security sources / teams / sinks |

## Local (this example)

Paths in `[[organizations]].app` are relative to the bootstrap file's directory.
Runtime namespaces ids/tags as `{organization_id}::{…}`.

Copy the sample next to Compose, or point `--config` at
`deploy/examples/multi-org/bootstrap.toml` after the stack is running and
validate with the container binary:

```bash
docker compose exec xrelease xrelease \
  --config /config/bootstrap.toml validate --strict
```

### Docker (Local multi-org)

```bash
cp deploy/examples/multi-org/bootstrap.toml bootstrap.toml
# Mount org files — in docker-compose.yaml uncomment e.g.:
#   - ./deploy/examples/multi-org/app:/etc/xrelease/app:ro
# And point [[organizations]].app at app/platform/releases.yaml etc.
# (paths are relative to the bootstrap file dir inside the container: /etc/xrelease/)
```

Prefer validating with `--config deploy/examples/multi-org/bootstrap.toml` from
the host, or adapt paths after copying `app/` next to your mounted bootstrap.

### Kubernetes

The Helm chart mounts a **single** `releases.yaml`. Multi-org file trees need a
custom volume layout (not the stock chart). For multi-tenant UI/API on one
process, use **API + UI** (Compose default) or one org per instance.

## Authoring variants

| `[config_api]` | Boot reads | UI / `xrctl apply` |
|---|---|---|
| **Local** (this example) | `app/<org>/releases.yaml` | Refused (409) — edit Git, then reload |
| **API** / **API + UI** | `config_revision` per org | Allowed — document in PostgreSQL; disk YAML is optional seed |

With `source = "api"`, `app` on each org is **optional** (omit to boot empty and
author via UI / `xrctl`). See
[Authoring variants](../../../docs/configuration/overview.md#authoring-variants).

## Per-organization apply (`source = "api"`)

```bash
# Compose UI proxy; pass the same key the backend has in XRELEASE_API_KEY
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" organizations
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" \
  --organization platform show
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" \
  --organization platform apply app/platform/releases.yaml --label "$GIT_SHA"
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" \
  --organization platform rollback
```

On a multi-org instance, whole-document `POST /api/v1/config/apply` returns
**409** — always pass `--organization`.

`POST /api/v1/reload` (and SIGHUP) re-resolves **every** org from its authority.

## Validate

```bash
xrelease validate --config deploy/examples/multi-org/bootstrap.toml --strict
```
