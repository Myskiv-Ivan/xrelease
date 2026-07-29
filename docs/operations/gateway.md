# Gateway API front door

Default deploy uses Gateway API. The chart renders **only the HTTPRoute**;
CRDs, controller, `GatewayClass` and `Gateway` are platform objects.
Manifests: [`deploy/k8s/gateway/`](../../deploy/k8s/gateway/README.md).

Install walkthrough: [Kubernetes](../getting-started/kubernetes.md).

## Controllers

| Controller | Install | `gatewayClassName` |
|---|---|---|
| **Envoy Gateway** | `helm install eg oci://docker.io/envoyproxy/gateway-helm --version v1.2.4 -n envoy-gateway-system --create-namespace` | `eg` |
| **NGINX Gateway Fabric** | `helm install ngf oci://ghcr.io/nginx/charts/nginx-gateway-fabric -n nginx-gateway --create-namespace` | `nginx` |
| **Istio** | `istioctl install --set profile=minimal` | `istio` |
| **Cilium** | `gatewayAPI.enabled=true` in the Cilium chart | `cilium` |

## Values

| Key | Purpose |
|---|---|
| `gateway.enabled` | Render HTTPRoute (default `true`) |
| `gateway.parentRef.name` | Gateway to attach to |
| `gateway.parentRef.sectionName` | Bind one listener (e.g. `https`) |
| `gateway.hostnames` | Site overlay — [`deploy/k8s/values.yaml`](../../deploy/k8s/values.yaml) |
| `gateway.api.*` | Second route for forge webhooks |

Routing: everything → UI Service → nginx proxies `/api` and probes.

## TLS

Uncomment the `https` listener in [`gateway.yaml`](../../deploy/k8s/gateway/gateway.yaml),
create the TLS Secret, then set in a local overlay:

```yaml
gateway:
  parentRef:
    sectionName: https
```

```bash
kubectl -n xrelease create secret tls xrelease-tls --cert=cert.pem --key=key.pem
helm upgrade xrelease ./deploy/helm/xrelease \
  -f deploy/k8s/values.yaml \
  -f deploy/k8s/values.secrets.yaml \
  -f values.local.yaml
```

## NetworkPolicy

`networkPolicy.ingressControllerNamespace` must name the Gateway data plane
(default `envoy-gateway-system`).

| Controller | Namespace |
|---|---|
| Envoy Gateway | `envoy-gateway-system` |
| NGINX Gateway Fabric | `nginx-gateway` |
| Istio | `istio-system` |
| Cilium | `kube-system` |

## Ingress instead

Set `ingress.enabled: true` and `gateway.enabled: false` — see
[deployment variants](deployment-variants.md).
