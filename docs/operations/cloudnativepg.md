# PostgreSQL with CloudNativePG

**Default** database mode for the Helm chart (`postgresql.mode: cnpg`). The
built-in StatefulSet (`mode: builtin`) remains available for labs — a **single
replica with no backups**, fine for evaluation, not for production data.

| | `postgresql.mode: builtin` | `postgresql.mode: cnpg` | `postgresql.mode: external` |
|---|---|---|---|
| Owner | this chart | CloudNativePG operator | your cloud provider |
| Replicas | 1 | 3 (configurable) | provider |
| Failover | none — pod restart | automated, ~seconds | provider |
| Backups | `pg_dump` by hand | continuous WAL → object storage | provider |
| Minor upgrades | edit image, recreate pod | rolling, operator-driven | provider |
| Connection | chart Secret | operator Secret `<cluster>-app` | `database.url` / Secret |

xrelease stays a **single writer** in all three: one poller holds an advisory
lease on the database ([scaling](scaling.md)). CloudNativePG makes the
*database* highly available, not the poller.

## Install the operator

Cluster-scoped, one per cluster, independent of any xrelease release:

```bash
helm repo add cnpg https://cloudnative-pg.github.io/charts
helm repo update
helm upgrade --install cnpg cnpg/cloudnative-pg \
  --namespace cnpg-system --create-namespace

kubectl -n cnpg-system rollout status deploy/cnpg-cloudnative-pg
kubectl get crd clusters.postgresql.cnpg.io
```

## Install xrelease

CNPG is the **chart default**. Install walkthrough:
[Kubernetes](../getting-started/kubernetes.md).

The chart renders a `Cluster`, waits for the primary in an init container, and
reads `XRELEASE_DATABASE_URL` from the operator-managed Secret — **no database
password goes into your values**.

```bash
kubectl -n xrelease get cluster xrelease-db
kubectl -n xrelease get secret xrelease-db-app -o jsonpath='{.data.uri}' | base64 -d
```

`postgresql.auth.password` applies only to `mode: builtin` — leave it unset for CNPG.

## How the wiring works

CloudNativePG publishes:

| Object | Role |
|---|---|
| Secret `<cluster>-app` | `uri`, `username`, `password`, `host`, `port`, `dbname` |
| Service `<cluster>-rw` | always the current primary — reads **and** writes |
| Service `<cluster>-ro` | replicas only |
| Service `<cluster>-r` | any instance |

The chart uses `<cluster>-rw` (the poller writes) and takes `uri` verbatim.
The URI has no `sslmode`, so `postgresql.cnpg.sslMode` is passed separately as
`XRELEASE_DATABASE_SSL_MODE` — explicit config wins over the URL.

`verify-full` additionally needs the operator CA mounted into the backend pod
and `XRELEASE_DATABASE_SSL_ROOT_CERT` pointed at it; `require` encrypts
without verifying the peer and needs no extra wiring.

## Reusing an existing Cluster

Already running CloudNativePG? Point the chart at it and skip creation:

```yaml
postgresql:
  mode: cnpg
  enabled: false
  cnpg:
    create: false
    clusterName: shared-db
    database: xrelease
```

The Secret `shared-db-app` must exist in the release namespace. A Cluster in
another namespace means copying that Secret over — Kubernetes does not
reference Secrets across namespaces.

Give xrelease its **own database**: a second poller against the same database
fails at startup with `PollerBusy`.

## Backups

A Cluster without backups is not production. Create the credentials Secret,
then enable:

```bash
kubectl -n xrelease create secret generic xrelease-backup-s3 \
  --from-literal=ACCESS_KEY_ID=... \
  --from-literal=ACCESS_SECRET_KEY=...
```

```yaml
postgresql:
  cnpg:
    backup:
      enabled: true
      destinationPath: s3://my-bucket/xrelease
      endpointURL: ""          # set for MinIO / Ceph
      retentionPolicy: 30d
      credentialsSecret: xrelease-backup-s3
```

That configures continuous WAL archiving. Schedule base backups separately —
the operator's `ScheduledBackup` is not part of this chart:

```yaml
apiVersion: postgresql.cnpg.io/v1
kind: ScheduledBackup
metadata:
  name: xrelease-db-daily
  namespace: xrelease
spec:
  schedule: "0 0 2 * * *"      # 6 fields: seconds first
  backupOwnerReference: self
  cluster:
    name: xrelease-db
```

## Lifecycle

The `Cluster` carries `helm.sh/resource-policy: keep`, so `helm uninstall`
leaves the database and its PVCs behind. Remove it deliberately:

```bash
kubectl -n xrelease delete cluster xrelease-db
```

Minor-version upgrades and failovers are rolling; the xrelease pod reconnects
through `<cluster>-rw`. In-flight polls fail and retry — delivery is covered
by outbox leases ([scaling](scaling.md)).

## Monitoring

`postgresql.cnpg.monitoring.enablePodMonitor: true` makes the operator export
PostgreSQL metrics (needs the Prometheus Operator CRDs, same as
`metrics.serviceMonitor`). CloudNativePG publishes a Grafana dashboard of its
own; the xrelease dashboard in [`deploy/grafana/`](../../deploy/grafana/)
covers the application side.

## Storage notes

`postgresql.cnpg.walStorage.enabled` puts WAL on a separate volume — worth it
when the outbox is busy and the data disk is a network volume.

Migrating from the built-in StatefulSet is a dump/restore, not a volume move
(different `PGDATA` layout — see [storage](storage.md)):

```bash
kubectl -n xrelease exec sts/xrelease-postgresql -- \
  pg_dump -U xrelease xrelease > xrelease.sql
kubectl -n xrelease exec -i xrelease-db-1 -- \
  psql -U xrelease xrelease < xrelease.sql
```

Stop the poller first (`kubectl -n xrelease scale deploy/xrelease --replicas=0`)
so nothing writes mid-dump.
