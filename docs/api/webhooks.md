# Webhooks guide

Webhooks complement polling — they reduce notification latency for Git forges that support push events.

## Prerequisites

1. Run `xrelease serve`
2. Set `XRELEASE_WEBHOOK_SECRET`
3. Configure matching sources in the desired document (Git YAML, UI Apply, or `xrctl apply`)

## GitHub

**URL:** `https://your-host/api/v1/webhooks/github`

**Events:** Releases → `Release published`

**Secret:** Same as `XRELEASE_WEBHOOK_SECRET` (HMAC SHA-256)

**Mapping:** `repository.full_name` → source with matching `repo` (`type: github`)

```yaml
sources:
  - type: github
    repo: tokio-rs/tokio
```

## GitLab

**URL:** `https://your-host/api/v1/webhooks/gitlab`

**Trigger:** Project → Webhooks → Release events

**Header:** `X-Gitlab-Token: <secret>`

```yaml
sources:
  - type: gitlab
    project: group/project
```

## Gitea / Codeberg

**URL:** `https://your-host/api/v1/webhooks/gitea`

**Header:** `X-Webhook-Secret: <secret>` or `Authorization: Bearer <secret>`

## Bitbucket

**URL:** `https://your-host/api/v1/webhooks/bitbucket`

**Trigger:** Repository → Webhooks → Push (tags)

**Secret:** Same as `XRELEASE_WEBHOOK_SECRET` (HMAC). Bitbucket Cloud sends
`X-Hub-Signature` (hex); some installs also send GitHub-style
`X-Hub-Signature-256` (`sha256=…`).

**Mapping:** `repository.full_name` → source with matching `repo` (`type: bitbucket`)

```yaml
sources:
  - type: bitbucket
    repo: workspace/repo
```

## Docker Hub

**URL:** `https://your-host/api/v1/webhooks/docker`

**Trigger:** Docker Hub → Webhooks → Push to repository

**Header:** `X-Webhook-Secret: <secret>` or `Authorization: Bearer <secret>`

**Mapping:** image name → any configured container source with matching `image`
(`type: docker`, `ghcr`, `quay`, or `ecr`).

```yaml
sources:
  - type: docker
    image: library/nginx
  # also matches: ghcr / quay / ecr with the same image string
```

## Generic

For custom CI pipelines:

**URL:** `POST /api/v1/webhooks/generic`

```json
{
  "source_id": "pypi:requests",
  "tag": "2.32.0",
  "url": "https://pypi.org/project/requests/2.32.0/",
  "prerelease": false
}
```

## Response

```json
{
  "accepted": true,
  "source_id": "github:owner/repo",
  "tag": "v1.2.3",
  "notifications_sent": 1,
  "message": "notification delivered"
}
```

Duplicate or filtered releases return `notifications_sent: 0`.

## Testing locally

Docker Compose (UI proxies the API):

```sh
curl -X POST http://127.0.0.1:3000/api/v1/webhooks/generic \
  -H "X-Webhook-Secret: $XRELEASE_WEBHOOK_SECRET" \
  -H "Content-Type: application/json" \
  -d '{"source_id":"github:owner/repo","tag":"v9.9.9-test"}'
```

Native `xrelease serve` (API on `:8080`):

```sh
curl -X POST http://127.0.0.1:8080/api/v1/webhooks/generic \
  -H "X-Webhook-Secret: $XRELEASE_WEBHOOK_SECRET" \
  -H "Content-Type: application/json" \
  -d '{"source_id":"github:owner/repo","tag":"v9.9.9-test"}'
```

Use HTTPS in production — see [TLS](../operations/tls.md).

## Outbound webhooks (xrelease → your HTTP)

Separate from forge ingress: add `[[notifiers]]` with `type: webhook` in the
app document, or **Config → Edit → Delivery channels → Add channel → Webhook**
(headers, HMAC `secret` / `secret_env`, template).

Examples: [`deploy/examples/webhooks/`](../../deploy/examples/webhooks/).
