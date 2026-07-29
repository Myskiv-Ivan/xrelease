# Grafana dashboard

Import [`dashboard.json`](dashboard.json) into Grafana (Dashboards → Import).

Scrape `xrelease serve` at `/metrics` (Prometheus text 0.0.4).

`/metrics` is **unauthenticated**, so scrape the backend directly rather than
through an edge proxy. The UI nginx returns `404` for it wherever that nginx
is reachable from outside the host (Helm Ingress, `docker/compose.tls.yaml`).

**Kubernetes** — Prometheus Operator:

```bash
# --reuse-values keeps the release's existing values; without it the flags
# below would be the ONLY values and the install would lose its password.
helm upgrade xrelease ./deploy/helm/xrelease --reuse-values \
  --set metrics.serviceMonitor.enabled=true
```

Plain Prometheus, or Docker Compose (`xrelease` resolves on the compose
network; the backend port is not published to the host):

```yaml
scrape_configs:
  - job_name: xrelease
    metrics_path: /metrics
    static_configs:
      # k8s: xrelease.<namespace>.svc:8080
      - targets: ["xrelease:8080"]
```

## Panels

| Panel | Metrics |
|---|---|
| Polls / min | `xrelease_polls_*` |
| Notifications / min | `xrelease_notifications_total` |
| Outbox depth | `xrelease_outbox_{pending,failed,dead}` |
| Poll / notify latency | `xrelease_{poll,notify}_duration_seconds` histograms |
| Outbox flush latency | `xrelease_outbox_flush_duration_seconds` |
| Webhooks | `xrelease_webhooks_*` |

Latency histograms use fixed second buckets (`0.005` … `30`, `+Inf`). Adjust
`[defaults].outbox_flush_concurrency` if flush p95 climbs under a deep outbox.
