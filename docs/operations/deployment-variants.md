# Deployment variants

The **default** is the chart itself: CNPG + Gateway API. Install:
[Kubernetes](../getting-started/kubernetes.md).

Optional shapes are plain value overrides — no extra files in the repo. Put
them in a local YAML (e.g. `values.local.yaml`, gitignored) and add another
`-f`.

| Want | Override |
|---|---|
| Ingress instead of Gateway | `ingress.enabled: true`, `gateway.enabled: false`, set `ingress.hosts` |
| Builtin single-node PG (lab) | `postgresql.mode: builtin`, `postgresql.enabled: true`, `gateway.enabled: false`, `networkPolicy.enabled: false`, `metrics.serviceMonitor.enabled: false`, `ui.replicaCount: 1` |
| External / managed PG | `postgresql.mode: external`, `secrets.existingSecret: …` (URL in the Secret) |
| OIDC UI | `ui.env.VITE_AUTH_MODE: oidc` + `VITE_OIDC_*`; backend `XRELEASE_OIDC_*` in the Secret — [oidc.md](oidc.md) |
| Gateway HTTPS | Uncomment https listener in `gateway.yaml`, set `gateway.parentRef.sectionName: https` — [gateway.md](gateway.md) |
| Ingress TLS | `ingress.tls` + a TLS Secret — [tls.md](tls.md) |
| GitOps / ArgoCD | `config.existingConfigMap` + `secrets.existingSecret` — [`secret.example.yaml`](../../deploy/k8s/secret.example.yaml) |

`ingress.enabled` and `gateway.enabled` are mutually exclusive.

Deep dives: [CloudNativePG](cloudnativepg.md) · [Gateway API](gateway.md) ·
[TLS](tls.md) · [OIDC](oidc.md).
