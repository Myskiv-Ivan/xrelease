# Grafana dashboard

Import [`dashboard.json`](dashboard.json) into Grafana (Dashboards → Import).

Scrape `xrelease serve` at `/metrics` (Prometheus text 0.0.4). Example scrape config:

```yaml
scrape_configs:
  - job_name: xrelease
    metrics_path: /metrics
    static_configs:
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
