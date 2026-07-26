# Configuration examples

Sample **`bootstrap.toml` + app YAML** patterns. Secrets always come from the
**single** repo-root [`.env.example`](../../.env.example) (Docker) or
[`deploy/k8s/secret*.yaml`](../k8s/) (Kubernetes) — do not add per-example `.env`
templates.

## Defaults vs examples

| What | Authoring | Notes |
|---|---|---|
| **Docker default** (repo root) | **API + UI** + multi-org | [`bootstrap.toml`](../../bootstrap.toml) — Compose mounts this; idle until Apply |
| **K8s default** (chart `values.yaml`) | **API + UI** + single doc | + [`values.secrets.yaml`](../k8s/values.secrets.example.yaml) — idle until Apply |
| [`infra-app/`](infra-app/) | **API** (CI / `xrctl`, no UI editor) | Split infra + apply — example only |
| [`multi-team/`](multi-team/) | **Local** | Per-team routing (eXpress / Apprise tags) — example only |
| [`multi-org/`](multi-org/) | **Local** (+ how to switch to API) | `[[organizations]]` + `app/<org>/…` — example only |
| [`webhooks/`](webhooks/) | snippets only | Outbound `type: webhook` notifier |

## Layout rules

| Path | Role |
|------|------|
| Repo root `bootstrap.toml` + `app/` | **Active Docker lab** used by Compose |
| Repo root `.env.example` | **Single** secrets template (Docker + UI + OIDC) |
| `deploy/examples/*/` | Copy-paste patterns — not auto-mounted |
| `deploy/k8s/secret*.yaml` | K8s Secret examples only (never desired-state YAML) |
| `deploy/helm/xrelease/values.yaml` | Helm defaults (**API + UI**) |
| `deploy/helm/xrelease/` | Chart — no sample secrets |

Secrets never live under `examples/`. Desired state never lives under `k8s/`.

## Deploy paths

| Platform | Start here |
|----------|------------|
| **Docker** | [`docker/README.md`](../../docker/README.md) — `docker compose up -d` |
| **Kubernetes** | [`deploy/k8s/README.md`](../k8s/README.md) — chart defaults + secrets |
| **User guide** | [`docs/getting-started/quickstart.md`](../../docs/getting-started/quickstart.md) |

Copy example config into place (Docker) or a ConfigMap (K8s) — see each example
README. For **Local** under Compose you must mount the app YAML (see example
instructions); the default Compose lab does **not** mount `app/releases.yaml`.
