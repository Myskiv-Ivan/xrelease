# Deployment variants

**Default** chart: CNPG + Gateway API. Install:
[Kubernetes](../getting-started/kubernetes.md).

Optional shapes are local value overrides (another `-f`), not extra files in the repo.

| Want | Override |
|---|---|
| Ingress (nginx / Traefik / …) | `ingress.enabled: true`, `gateway.enabled: false`, `ingress.className`, `ingress.hosts` |
| No Prometheus CRDs | `metrics.serviceMonitor.enabled: false`, `postgresql.cnpg.monitoring.enablePodMonitor: false` |
| StorageClass (e.g. Longhorn) | `postgresql.cnpg.storage.storageClass: longhorn` |
| Builtin single-node PG (lab) | `postgresql.mode: builtin`, `postgresql.enabled: true`, `gateway.enabled: false`, `networkPolicy.enabled: false`, `metrics.serviceMonitor.enabled: false` |
| UI HA (PDB + anti-affinity) | `ui.replicaCount: 2` (or higher) |
| External / managed PG | `postgresql.mode: external`, `secrets.existingSecret` (URL in the Secret) |
| Disable Apprise | `apprise.enabled: false` |
| OIDC UI | `ui.env.VITE_AUTH_MODE: oidc` + `VITE_OIDC_*`; backend `XRELEASE_OIDC_*` in the Secret — [oidc.md](oidc.md) |
| Gateway HTTPS | Uncomment https in `gateway.yaml`, set `gateway.parentRef.sectionName: https` — [gateway.md](gateway.md) |
| Ingress TLS | `ingress.tls` + TLS Secret — [tls.md](tls.md) |
| GitOps / ArgoCD | `config.existingConfigMap` + `secrets.existingSecret` — [`secret.example.yaml`](../../deploy/k8s/secret.example.yaml) |

`ingress.enabled` and `gateway.enabled` are mutually exclusive.

When using Ingress, set `networkPolicy.ingressControllerNamespace` to the
controller namespace (e.g. `traefik`, `kube-system`) if NetworkPolicy stays on.

Deep dives: [CloudNativePG](cloudnativepg.md) · [Gateway API](gateway.md) ·
[TLS](tls.md) · [OIDC](oidc.md).
