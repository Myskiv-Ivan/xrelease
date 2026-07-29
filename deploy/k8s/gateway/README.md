# Platform Gateway

`gateway.yaml` is a **platform** object, deliberately outside the Helm release:
it usually fronts several applications and owns the TLS certificates and the
load-balancer address. The chart only attaches an `HTTPRoute` to it through
`gateway.parentRef`.

Edit two things before applying — the hostname and `gatewayClassName` (it must
match a `GatewayClass` your controller reconciles):

```bash
kubectl get gatewayclass          # what this cluster has
kubectl apply -f deploy/k8s/gateway/gateway.yaml
kubectl -n xrelease get gateway xrelease-gateway
```

The `https` listener is commented out; uncomment it once the TLS Secret exists
(or cert-manager fills it) and set `gateway.parentRef.sectionName: https`.

Controller matrix, route troubleshooting, TLS and the NetworkPolicy namespaces:
[Gateway API](../../../docs/operations/gateway.md). Full install:
[Kubernetes](../../../docs/getting-started/kubernetes.md).
