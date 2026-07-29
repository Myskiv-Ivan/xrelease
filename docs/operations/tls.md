# TLS & transport security

HTTPS is terminated **at the edge**. On Docker that is the **UI nginx**
container (same image as HTTP). On Kubernetes, the **Gateway** listener
(canonical) or Ingress terminates TLS. The `xrelease` API process always speaks
HTTP on the private network.

```mermaid
flowchart LR
  Client[Browser / webhooks] -->|HTTPS| Edge[UI nginx / Gateway / Ingress]
  Edge -->|HTTP + X-Forwarded-Proto| API[xrelease :8080]
  API --> PG[(PostgreSQL)]
```

| Hop | Default | With TLS |
|---|---|---|
| Clients → UI | HTTP `127.0.0.1:3000` | HTTPS on UI nginx (`cert.pem` + `key.pem`) |
| UI nginx → API | HTTP | HTTP; `X-Forwarded-Proto` = `https` |
| API → managed Postgres | plain (Compose) | `XRELEASE_DATABASE_SSL_MODE=require` |

## Docker — HTTP (default)

```bash
cp .env.example .env
docker compose up -d
# → http://127.0.0.1:3000
```

Backend `:8080` is not published. Leave `VITE_API_URL` empty.

## Docker — HTTPS (nginx + cert/key)

`docker/compose.tls.yaml` remounts
[`docker/nginx.tls.conf`](../../docker/nginx.tls.conf) into the UI container,
publishes HTTP/HTTPS ports, and mounts your PEMs.

Configure in **`.env`**:

| Variable | Default | Purpose |
|---|---|---|
| `XRELEASE_PUBLIC_HOST` | — | Hostname for DNS/`/etc/hosts` (must match cert) |
| `XRELEASE_TLS_CERT` | `./docker/certs/cert.pem` | Certificate PEM |
| `XRELEASE_TLS_KEY` | `./docker/certs/key.pem` | Private key PEM |
| `XRELEASE_TLS_HTTP_PORT` | `80` | Published HTTP (redirect → HTTPS) |
| `XRELEASE_TLS_HTTPS_PORT` | `443` | Published HTTPS |

```bash
# .env
XRELEASE_PUBLIC_HOST=xrelease.example.com
XRELEASE_TLS_CERT=./docker/certs/cert.pem
XRELEASE_TLS_KEY=./docker/certs/key.pem
XRELEASE_TLS_HTTP_PORT=80
XRELEASE_TLS_HTTPS_PORT=443

docker compose -f docker-compose.yaml -f docker/compose.tls.yaml up -d
# → https://xrelease.example.com  (loopback :3000 is not published in this mode)
xrctl --api-url "https://$XRELEASE_PUBLIC_HOST" --api-key "$XRELEASE_API_KEY" status
```

Lab self-signed:

```bash
openssl req -x509 -nodes -newkey rsa:2048 -days 365 \
  -keyout docker/certs/key.pem -out docker/certs/cert.pem \
  -subj "/CN=xrelease.local"
# .env → XRELEASE_PUBLIC_HOST=xrelease.local
# /etc/hosts → 127.0.0.1 xrelease.local
docker compose -f docker-compose.yaml -f docker/compose.tls.yaml up -d
```

OIDC: `VITE_OIDC_REDIRECT_URI=https://<host>/login/callback`, restart `ui`.
Webhooks: `https://<host>/api/v1/webhooks/...`.

## Kubernetes — Ingress + TLS Secret

For the **default** stack (Gateway API), terminate TLS on the Gateway
listener — see [Gateway API](gateway.md). The example below is only if you
switched to Ingress ([deployment variants](deployment-variants.md)).

```bash
kubectl -n xrelease create secret tls xrelease-tls \
  --cert=cert.pem \
  --key=key.pem

# values.local.yaml:
#   ingress:
#     enabled: true
#     tls: [{ secretName: xrelease-tls, hosts: [xrelease.example.com] }]
#     hosts: [{ host: xrelease.example.com, paths: [{ path: /, pathType: Prefix }] }]
#   gateway:
#     enabled: false

helm upgrade --install xrelease ./deploy/helm/xrelease \
  --namespace xrelease --create-namespace \
  -f deploy/k8s/values.secrets.yaml \
  -f values.local.yaml
```

## PostgreSQL client TLS (managed DB)

```bash
XRELEASE_DATABASE_URL=postgres://xrelease:secret@db.example.com:5432/xrelease
XRELEASE_DATABASE_SSL_MODE=require
```

## Checklist

- [ ] `cert.pem` + `key.pem` (Compose `.env` paths, or K8s TLS Secret)
- [ ] Strong secrets — [Authentication](authentication.md)
- [ ] `XRELEASE_WEBHOOK_SECRET`; forge URLs use `https://`
- [ ] Apprise not public
- [ ] OIDC redirects use `https://` — [OIDC](oidc.md)

See [Docker](../getting-started/docker.md) · [Kubernetes](../getting-started/kubernetes.md).
