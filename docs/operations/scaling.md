# Scaling & deployment limits

xrelease is designed as a **single-process, single-writer** notifier backed by
**PostgreSQL**. This document covers vertical scaling, horizontal constraints,
and operational knobs.

## Process model

```
bootstrap.toml + desired state → Engine → N independent poll loops (one per source)
                ↓
         PostgreSQL + Apprise / Express / webhooks
```

| Dimension | Behaviour |
|---|---|
| Sources per process | **100–500+** typical; one lightweight loop each |
| Poll concurrency | Unbounded tasks; stagger + jitter spread load |
| HTTP API | Same process in `serve` mode; rate-limited `/api/v1/*` |
| State | **One PostgreSQL database** — single xrelease replica |

## Horizontal scaling

| Pattern | Supported? | Notes |
|---|---|---|
| Multiple processes, **same** Postgres DB | ❌ | Second `serve` fails at startup (`PollerBusy` — advisory lock held) |
| Multiple processes, **split** config by source | ✅ | Partition `sources` in separate `app/releases.yaml` + separate DBs |
| HA Postgres + one xrelease | ✅ | Managed Postgres; still one poller process |

### Delivery leases do not enable multi-replica polling

Outbox row leases prevent double-send when the poll path and the periodic flush
claim the same notification. They do **not** allow two pollers against one
database.

A second `xrelease serve` against the same database fails at startup
(`PollerBusy`) instead of duplicating work. Keep **one poller**
(`replicaCount: 1`, Helm `Recreate`) or split sources across databases.

## Vertical scaling tips

1. **ETag / 304** — reduces upstream bandwidth (`xrelease_polls_not_modified_total`).
2. **Per-source metrics** — `xrelease_source_polls_total{source="…"}`.
3. **Latency histograms** — `xrelease_poll_duration_seconds`,
   `xrelease_notify_duration_seconds`, `xrelease_outbox_flush_duration_seconds`
   (Grafana: [`deploy/grafana/`](../../deploy/grafana/)).
4. **Outbox flush concurrency** — `defaults.outbox_flush_concurrency` in
   `app/releases.yaml` (default 8) parallelises delivery within one flush wave.
5. **Rate limits** — `[api].rate_limit_per_minute` in `bootstrap.toml` for webhook bursts
   (`xrelease_http_rate_limited_total` counts 429s).
6. **Sink health** — `xrelease_sink_deliveries_total{kind,sink,result}` and
   `xrelease_sink_breaker_open{kind,sink}`; outbox lifecycle counters
   (`enqueued` / `delivery_failures` / `dead_lettered` / `requeued`).
7. **Filters** — `pattern` on Docker/npm feeds; fewer identities → smaller DB.
8. **Prune** — `[database].prune_*` controls table growth (`xrelease_prune_deleted_total`).
9. **Graceful shutdown** — SIGINT/SIGTERM stops new polls, waits up to 30s for
   in-flight work, then runs a final outbox drain (leases cover hard kills).

## PostgreSQL growth control

```toml
[database]
postgres_url = "postgres://xrelease:@postgres:5432/xrelease"
prune_seen_after_days = 365
prune_webhooks_after_days = 30
prune_outbox_sent_after_days = 90
prune_advisories_after_days = 90
prune_interval_hours = 24
```

Prune runs on **Engine startup** and on a background timer when
`prune_interval_hours > 0`.

> Pruning old `seen_release` rows may re-notify very old tags if they reappear
> upstream.

## Upstream rate limits

| Source | Anonymous limit | Mitigation |
|---|---|---|
| GitHub Atom | ~60 req/hr | Set `GITHUB_TOKEN` → REST + 5000 req/hr |
| Package registries | Varies | Increase `interval_secs`; use `pattern` |
| Docker Hub | Strict | Token + conservative intervals |

Global cap: `defaults.upstream_requests_per_minute` in `app/releases.yaml`
(or `XRELEASE_UPSTREAM_RPM`).

## Resource expectations

| Sources | RAM | CPU | Postgres disk |
|---|---|---|---|
| 10 | ~30 MB | negligible | < 50 MB |
| 100 | ~50–80 MB | low | 100 MB–1 GB |
| 500 | ~150 MB | moderate | depends on prune |

## When to split deployments

- Different teams need isolated secrets / Apprise routing
- Source count exceeds comfortable poll fan-out (500+)
- Webhook ingress should fail independently of the poller (separate instance)

Each instance: own config pair, own PostgreSQL database, own team tags.

## Platform notes

| | Docker Compose | Kubernetes |
|---|---|---|
| Replicas | 1 `xrelease` service | `replicaCount: 1` |
| Postgres | `postgres` service + volume | Managed or operator |
| Config reload | `--force-recreate xrelease` | ConfigMap update + rollout |
| Default memory limit | 512M (`deploy.resources`) | `resources.limits.memory: 512Mi` |

Chart defaults sit at the 100-source line of the table above (requests
100m/128Mi, limits 1 CPU / 512Mi). Past ~300 sources raise
`resources.requests.memory` first — the poller is memory-bound, not CPU-bound.
The production overlay
([`values.yaml`](../../deploy/k8s/values.yaml))
starts at 200m/256Mi.

See [`deploy/README.md`](../../deploy/README.md).
