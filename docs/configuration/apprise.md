# Notifications

Integrations for **where to send** release events — Apprise, Novu, Slack,
Telegram, SMTP, webhooks, eXpress, and optional brokers.

**Authoring rule:** destinations (`urls`, `config_key`, Novu workflow / topic,
chat ids, …) live only in the **desired document** — Git YAML, dashboard
**Delivery channels**, or `xrctl apply` / `POST /api/v1/config/apply`.

**Env** is for secrets and infra only (API keys, BotX Bearer, SMTP password,
`XRELEASE_NOVU_API_KEY`, and optionally `XRELEASE_APPRISE_ENDPOINT` so
Compose/Helm can point at the Apprise sidecar).

Declare sinks under `notifiers:` (for example
`{ type: apprise, … }` or `{ type: novu, … }`).

## Sink kinds

| `type` | Integration |
|---|---|
| `apprise` | [Apprise](https://github.com/caronc/apprise) HTTP API — Telegram, Slack, Discord, email, … (80+ URL schemes) |
| `webhook` | Custom HTTP `POST` / `PUT` |
| `smtp` | Direct e-mail (STARTTLS / TLS / plain) |
| `slack` | Slack Incoming Webhook **or** Bot `chat.postMessage` |
| `telegram` | Telegram Bot API `sendMessage` |
| `express` | eXpress BotX chat |
| `novu` | [Novu](https://github.com/novuhq/novu) workflow trigger (email / SMS / Slack / in-app / …) |
| `kafka` | Apache Kafka topic producer |
| `nats` | NATS subject publisher |
| `rabbitmq` | RabbitMQ exchange publisher |

Every sink kind is compiled into the binary (local `cargo build`, GHCR images,
and GitHub Release archives).

```yaml
# Destinations in the document; secrets via *_env → process env
notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls:
      - mailto://user:APP_PASSWORD@google.com?to=ops@example.com
      # - tgram://bot-token/chat-id
    tags: [platform-team]         # empty = broadcast
  - type: express
    name: express-platform
    tags: [platform-team]
    base_url: https://cts.example.com
    group_chat_id: dec60c05-77b7-0d78-159e-b4fbee4d48f6
    access_token_env: XRELEASE_EXPRESS_TOKEN_PLATFORM
  - type: novu
    name: novu-platform
    tags: [platform-team]
    # base_url: https://api.novu.co   # or https://eu.api.novu.co / self-host
    workflow: xrelease-new-release
    topic_key: "{{tag}}"              # Novu Topic; or set subscriber_id instead
    api_key_env: XRELEASE_NOVU_API_KEY
  - type: slack
    name: slack-ops
    tags: [platform-team]
    webhook_url_env: XRELEASE_SLACK_WEBHOOK_URL
    # Or bot mode instead of webhook:
    # bot_token_env: XRELEASE_SLACK_BOT_TOKEN
    # channel: "#ops"
  - type: telegram
    name: tg-ops
    tags: [platform-team]
    chat_id: "-1001234567890"
    bot_token_env: XRELEASE_TELEGRAM_BOT_TOKEN
    parse_mode: HTML
  - type: smtp
    name: mail-ops
    tags: [platform-team]
    host: smtp.example.com
    port: 587
    from: xrelease@example.com
    to: [ops@example.com]
    username: xrelease
    password_env: XRELEASE_SMTP_PASSWORD
  # - type: kafka
  #   brokers: ["kafka:9092"]
  #   topic: xrelease.events
  #   key: "{{source_id}}"
  # - type: rabbitmq
  #   url_env: XRELEASE_RABBITMQ_URL
  #   exchange: xrelease
  #   routing_key: "{{kind}}"
```

| Field | Role |
|---|---|
| notifier `tag` | Apprise **channel** tag inside persistent Apprise config |
| `notifiers[].tags` | xrelease **team routing** — matches source `routing_tag` |
| `novu.topic_key` | Novu Topic key (templates: `{{tag}}`, `{{source_id}}`, …) |
| `novu.subscriber_id` | Novu Subscriber id when not using a topic |

Notifier templates use a shared Mustache dialect — see
[Templates](#templates) below. Placeholders are also listed by
`GET /api/v1/config/schema` → `template_placeholders`.

---

## Templates

All sinks share one lightweight Mustache dialect (`{{field}}`). Double braces
never collide with literal `{` / `}` in JSON bodies
(`{"text":"{{title}}"}` is valid). Unknown placeholders and missing optionals
render as empty strings. Blank / whitespace-only templates are treated as
**unset** (the sink uses its default body).

### Placeholders

| Placeholder | Source |
|---|---|
| `{{source_id}}` | Stable id, e.g. `github:tokio-rs/tokio` |
| `{{source_kind}}` | Human kind, e.g. `GitHub` |
| `{{kind}}` | Alias of `{{source_kind}}` |
| `{{title}}` | Short notification title |
| `{{body}}` | Markdown release notes |
| `{{url}}` | Canonical release URL (empty if none) |
| `{{tag}}` | Source `routing_tag` (empty if none) |

### Where templates apply

| Sink | Config fields | Default when unset |
|---|---|---|
| **slack** / **telegram** / **express** | `template` (message body) | `title` + blank line + `body` + blank line + `url` |
| **telegram** | `chat_id` (always templated) | fixed chat id / `{{tag}}` |
| **slack** (bot mode) | `channel` (always templated) | `#ops` / `C…` / `{{tag}}` |
| **webhook** | `template` (HTTP body) | canonical JSON (see below) |
| **kafka** | `template` (record value), `key` (partition key) | JSON body; no key |
| **nats** | `template` (message), `subject` (always templated) | JSON body; subject e.g. `releases.{{kind}}` |
| **rabbitmq** | `template` (message), `routing_key` (always templated) | JSON body; key e.g. `{{kind}}` |
| **smtp** | `subject_template`, `template` (body) | subject = `title`; body = see `body_format` |
| **smtp** | `body_format` when body `template` unset | `text` → title+body+url; `markdown` → body+url |
| **novu** | `topic_key` / `subscriber_id` | structured payload (no body template — format in Novu workflow) |
| **apprise** | — | raw `title` + `body` + `format` (no Mustache body template) |

### Canonical JSON body (webhook / kafka / nats / rabbitmq)

```json
{
  "source_id": "github:org/app",
  "source_kind": "GitHub",
  "kind": "GitHub",
  "title": "app: v1.2.3",
  "body": "…",
  "url": "https://…",
  "tag": "platform-team"
}
```

`url` / `tag` are omitted when absent.

### Examples

```yaml
# Chat message
- type: slack
  webhook_url_env: XRELEASE_SLACK_WEBHOOK_URL
  template: "*{{title}}*\n{{body}}\n{{url}}"

# Telegram per-team chat via routing tag
- type: telegram
  chat_id: "{{tag}}"
  bot_token_env: XRELEASE_TELEGRAM_BOT_TOKEN
  parse_mode: HTML
  template: "<b>{{title}}</b>\n{{body}}\n{{url}}"

# Webhook / n8n custom JSON
- type: webhook
  url: https://hooks.example.com/xrelease
  template: '{"text":"{{title}}","link":"{{url}}","team":"{{tag}}"}'

# Kafka ordered by source
- type: kafka
  brokers: ["kafka:9092"]
  topic: xrelease.events
  key: "{{source_id}}"
  # template omitted → canonical JSON

# NATS / RabbitMQ routing
- type: nats
  url_env: XRELEASE_NATS_URL
  subject: "releases.{{kind}}"
- type: rabbitmq
  url_env: XRELEASE_RABBITMQ_URL
  exchange: xrelease
  routing_key: "{{kind}}.{{tag}}"

# SMTP
- type: smtp
  host: smtp.example.com
  from: xrelease@example.com
  to: [ops@example.com]
  password_env: XRELEASE_SMTP_PASSWORD
  subject_template: "[{{tag}}] {{title}}"
  template: "{{body}}\n\n{{url}}"          # optional; wins over body_format
  # body_format: text                      # default when template unset
```

### Novu payload (not a Mustache body)

xrelease sends `payload` fields that a Novu workflow can bind:

`source_id`, `kind`, `source_kind`, `title`, `body`, `url?`, `tag?`.

Targeting uses templated `topic_key` / `subscriber_id` (e.g. `topic_key: "{{tag}}"`).

See [`.env.example`](../../.env.example) and
[`deploy/examples/multi-team/`](../../deploy/examples/multi-team/).

---

## Recommended authoring model

| Own in… | Put there |
|---|---|
| **YAML / UI / `xrctl apply`** | `endpoint`, `urls`, `config_key`, `to`, chats, routing `tags`, sources, teams |
| **Env / K8s Secret** | `XRELEASE_API_KEY`, `XRELEASE_EXPRESS_*`, `XRELEASE_SMTP_PASSWORD`, `XRELEASE_WEBHOOK_SECRET`, `XRELEASE_NOVU_API_KEY`, `XRELEASE_SLACK_*`, `XRELEASE_TELEGRAM_BOT_TOKEN`, `XRELEASE_RABBITMQ_URL`, `XRELEASE_NATS_URL`, DB URL |
| **Bootstrap / Compose / Helm** | Postgres, `[config_api]`, Apprise sidecar; `XRELEASE_APPRISE_ENDPOINT` → sidecar base URL |

```text
desired document (YAML ↔ UI ↔ xrctl)     +     secrets / infra (env)
        sources / notifiers / urls                  tokens / passwords
        config_key (optional)                       XRELEASE_APPRISE_ENDPOINT
                         \                         /
                          └──── runtime sinks ────┘
```

### Env vars for notifier secrets

| Variable | Role |
|---|---|
| `XRELEASE_APPRISE_ENDPOINT` | **Infra only** — base URL of the Apprise API (Compose/Helm set `http://apprise:8000` / in-cluster service). Does not set targets. |
| `XRELEASE_NOVU_API_KEY` | Novu secret API key when `api_key` is empty / `api_key_env` unset |
| `XRELEASE_SLACK_WEBHOOK_URL` | Slack Incoming Webhook when `webhook_url` empty / `webhook_url_env` unset |
| `XRELEASE_SLACK_BOT_TOKEN` | Slack bot token when `bot_token` empty / `bot_token_env` unset |
| `XRELEASE_TELEGRAM_BOT_TOKEN` | Telegram bot token when `bot_token` empty / `bot_token_env` unset |
| `XRELEASE_SMTP_PASSWORD` | SMTP AUTH when `password` empty / `password_env` unset |
| `XRELEASE_RABBITMQ_URL` | AMQP URL when `url` empty / `url_env` unset |
| `XRELEASE_NATS_URL` | NATS URL when `url` empty / `url_env` unset |

Apprise **targets** (`urls`, `config_key`) and Novu **workflow / topic / subscriber**
live only in the desired-state document — not in environment variables (except the
API key overlay above). Secrets for Slack / Telegram / SMTP / RabbitMQ follow the
same `inline → *_env → XRELEASE_*` resolution as Express / Novu.

---

## Novu

[Novu](https://github.com/novuhq/novu) is a notification platform: define a workflow
in the Novu dashboard (or Framework), map subscribers/topics, then let xrelease
**trigger** that workflow on each release event.

### What you configure where

| Where | What |
|---|---|
| **Novu Cloud / self-host** | Create a **Workflow** (note its trigger id). Create a **Topic** per team (or one Subscriber). Attach email/Slack/… steps in the workflow. |
| **xrelease desired document** (YAML / UI Delivery channels / `xrctl apply`) | `type: novu`, `workflow`, `topic_key` or `subscriber_id`, routing `tags` |
| **Env / K8s Secret** | `XRELEASE_NOVU_API_KEY` (or per-sink `api_key_env`) — never commit the secret |
| **Optional** | `base_url` — default `https://api.novu.co`; EU = `https://eu.api.novu.co`; self-host = your API root (no `/v1`) |

### Minimal xrelease config

```yaml
notifiers:
  - type: novu
    workflow: xrelease-new-release
    topic_key: "{{tag}}"                 # or subscriber_id: ops-bot
    api_key_env: XRELEASE_NOVU_API_KEY
    # base_url: https://eu.api.novu.co  # optional
    tags: [platform-team]

teams:
  - tag: platform-team
    name: Platform

sources:
  - type: github
    repo: org/app
    routing_tag: platform-team
```

In `.env` / Secret:

```bash
XRELEASE_NOVU_API_KEY=nv_...
```

### End-to-end checklist

1. In **Novu**: workflow trigger id = `xrelease-new-release` (same string as `workflow`).
2. In **Novu**: Topic key = `platform-team` (same as source `routing_tag` / `{{tag}}`), with subscribers attached.
3. In **xrelease**: add the `notifiers` block above (UI → Delivery channels → kind **Novu** → Apply, or YAML + apply/reload).
4. Set `XRELEASE_NOVU_API_KEY` on the xrelease process.
5. **Test**: Delivery channels → Test (or `POST /api/v1/notifiers/test`).
6. On a release event, Novu Activity Feed should show a trigger with payload `title` / `body` / `url` / `tag`.

xrelease sends `POST {base_url}/v1/events/trigger` with a stable `transactionId` /
`Idempotency-Key` so outbox retries do not double-deliver. Payload fields:
`source_id`, `kind`, `source_kind`, `title`, `body`, `url` (if any), `tag` (if any).

`topic_key` wins over `subscriber_id` when both are set. Empty `tags` on the
notifier = broadcast (every event); non-empty = only matching `routing_tag`.

---

## Slack, Telegram, SMTP, Kafka, NATS, RabbitMQ

First-class sinks share the same outbox, team `tags`, Mustache templates, and
secret resolution (`inline → *_env → XRELEASE_*`). Use these when you only need
one channel instead of an Apprise URL scheme.

### Slack

```yaml
# Incoming Webhook (simplest)
- type: slack
  tags: [platform-team]
  webhook_url_env: XRELEASE_SLACK_WEBHOOK_URL

# Or Bot API
- type: slack
  tags: [platform-team]
  bot_token_env: XRELEASE_SLACK_BOT_TOKEN
  channel: "#ops"   # or C0123ABCD; templates OK
```

Set **either** webhook **or** bot+channel — not both.

### Telegram

```yaml
- type: telegram
  tags: [platform-team]
  chat_id: "-1001234567890"
  bot_token_env: XRELEASE_TELEGRAM_BOT_TOKEN
  parse_mode: HTML   # optional: HTML | Markdown | MarkdownV2
```

### SMTP (email)

```yaml
- type: smtp
  host: smtp.example.com
  port: 587
  from: xrelease@example.com
  to: [ops@example.com]
  username: xrelease
  password_env: XRELEASE_SMTP_PASSWORD
  tls: starttls
  subject_template: "[{{tag}}] {{title}}"   # optional
  # template: "{{body}}\n\n{{url}}"          # optional body; wins over body_format
  body_format: text                           # text | markdown (when template unset)
```

### Kafka / NATS / RabbitMQ

```yaml
- type: kafka
  brokers: ["kafka:9092"]
  topic: xrelease.events
  key: "{{source_id}}"

- type: nats
  url_env: XRELEASE_NATS_URL
  subject: "releases.{{kind}}"

- type: rabbitmq
  url_env: XRELEASE_RABBITMQ_URL
  exchange: xrelease
  routing_key: "{{kind}}"
```

Broker bodies default to the canonical JSON payload; optional `template` uses the
same Mustache dialect as webhooks (see [Templates](#templates)).

---

## How UI and CLI share one document

UI **Delivery channels** and **`xrctl apply` / `POST /api/v1/config/apply`**
write the **same** desired document into the config ledger when
`[config_api].source = "api"`. They are not separate channels.

```mermaid
sequenceDiagram
  participant Op as Operator
  participant Client as UI or xrctl
  participant API as POST /config/apply
  participant Ledger as Config ledger
  participant XR as xrelease runtime

  Op->>Client: edit Delivery channels or YAML file
  Client->>API: full desired document
  API->>Ledger: new applied revision
  API->>XR: hot-swap → build_notifiers
```

Requires the **API** or **API + UI** variant (`api_config = true`, `source = "api"`).
Dashboard forms also need `ui_config = true`. See
[Authoring variants](overview.md#authoring-variants) and [`xrctl`](../api/cli.md).

### Typical UI flow

1. Put targets in `app/releases.yaml`, **or** set them in the form.
2. **Config → Delivery channels** — `endpoint`, `urls` (one per line), `format`, routing tags.
3. **Apply** → **Test**.

UI does not edit Apprise `config_key` / channel `tag` (YAML only; preserved on
round-trip). Setting `urls` in the UI clears those YAML-only fields so both
modes are not left set.

### Typical CLI flow

```bash
# Compose UI stack (nginx proxies API):
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" \
  validate app/releases.yaml
xrctl --api-url http://127.0.0.1:3000 --api-key "$XRELEASE_API_KEY" \
  apply app/releases.yaml --if-match none --label "notify-update"

# Native binary (default --api-url http://127.0.0.1:8080):
# xrctl --api-key "$XRELEASE_API_KEY" validate app/releases.yaml
```

---

## UI vs YAML vs env (field map)

### Apprise

| Concern | UI | YAML | Env |
|---|---|---|---|
| `endpoint` | Yes | Yes | Optional `XRELEASE_APPRISE_ENDPOINT` (sidecar URL) |
| `urls` | **Yes** | **Yes** | — |
| `format` | Yes | Yes | — |
| Routing `tags` | Yes | Yes | — |
| `config_key` / Apprise `tag` | No (preserved from YAML) | Yes | — |

### eXpress

| Concern | UI / YAML | Env |
|---|---|---|
| `base_url`, `group_chat_id`, `recipients`, `tags` | Document | — |
| Bearer | `access_token` or `access_token_env: NAME` | Value of `NAME`, or fallback `XRELEASE_EXPRESS_ACCESS_TOKEN` |

### SMTP

| Concern | UI / YAML | Env |
|---|---|---|
| `host`, `from`, `to`, `tags`, … | Document | — |
| AUTH password | `password` field, or empty | `XRELEASE_SMTP_PASSWORD` (one shared fallback) |

### Webhook

HMAC: `secret` / `secret_env` in the document → named env or `XRELEASE_WEBHOOK_SECRET`.
Destination `url` is treated as a secret on `GET /api/v1/config` (path tokens
are common) and restored on Apply when left blank.

---

## Secret handling (all notifiers)

| Layer | Behavior |
|---|---|
| **Git / desired YAML** | Prefer `*_env` names only; put values in `.env` / K8s Secret |
| **Ledger (API apply)** | Structure + `*_env` refs in `config_revision.content`; values in `app_secret` (AES-GCM). Prefer `*_env` / `urls_env`. |
| **`GET /api/v1/config`** | Secrets replaced with `<redacted>` (Apprise urls, webhook url/secret/headers, Slack/Telegram tokens, Novu/Express/SMTP secrets, NATS/RabbitMQ URL userinfo) |
| **UI Apply** | Blank / redacted fields are omitted; server restores previous values |
| **Runtime** | `resolve_secret`: inline → named env → `XRELEASE_*` global; `<redacted>` never used as a real credential |
| **Logs / `Debug`** | Options holding tokens do not derive `Debug`; webhook HMAC secret is redacted in debug output |

---

## Different destination lists

| Goal | How |
|---|---|
| Same event → several targets | Several lines under `urls:` / several `to:` addresses |
| Different teams → different targets | Separate `notifiers` with `tags: […]` + source `routing_tag` |
| Secrets out of Git | `access_token_env` / blank SMTP `password` + env value; or Apprise `config_key` with channels stored in Apprise |

### Fan-out (one Apprise sink, many URLs)

```yaml
notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls:
      - mailto://user:APP_PASSWORD@google.com?to=ops@example.com
      - mailto://user:APP_PASSWORD@google.com?to=sre@example.com
      - tgram://bot-token/chat-id
```

### Team routing (Apprise + eXpress + SMTP)

```yaml
teams:
  - tag: platform-team
  - tag: security-team

notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls: ["mailto://user:APP_PASSWORD@google.com?to=platform@example.com"]
    tags: [platform-team]

  - type: express
    name: express-security
    tags: [security-team]
    base_url: https://cts.example.com
    group_chat_id: aaaa1111-0000-0000-0000-securitychat
    access_token_env: XRELEASE_EXPRESS_TOKEN_SECURITY

  - type: smtp
    name: smtp-platform
    tags: [platform-team]
    host: smtp.example.com
    from: releases@example.com
    to: [platform@example.com]
    username: releases
    # password empty → XRELEASE_SMTP_PASSWORD

sources:
  - type: github
    repo: org/app
    routing_tag: platform-team
```

```bash
# .env — secrets only
XRELEASE_EXPRESS_TOKEN_PLATFORM=…
XRELEASE_EXPRESS_TOKEN_SECURITY=…
XRELEASE_SMTP_PASSWORD=…
```

Empty sink `tags` = broadcast. See
[`deploy/examples/multi-team/`](../../deploy/examples/multi-team/).

### Persistent Apprise (document `config_key`)

Channels registered inside Apprise (`/add/<key>`), xrelease holds the key in YAML:

```yaml
notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    config_key: release-channels
    # tag: platform   # optional subset inside that Apprise config
```

---

## Docker sidecar

Compose runs `caronc/apprise`. xrelease reaches it at `http://apprise:8000`
via `XRELEASE_APPRISE_ENDPOINT` in
[`docker-compose.yaml`](../../docker-compose.yaml). Targets come from
`app/releases.yaml` or UI Apply.

## Per-source team routing

```yaml
sources:
  - type: github
    repo: team/critical-service
    routing_tag: security-team
```

Unused `teams` entries and orphan `routing_tag` values produce validation
warnings (`validate --strict` fails the build).

## Example Apprise URLs

```
tgram://bot-token/chat-id
discord://webhook-id/token
slack://tokenA/tokenB/tokenC
mailto://user:pass@smtp.example.com
```

```yaml
notifiers:
  - type: apprise
    endpoint: http://apprise:8000
    urls:
      - mailto://user:APP_PASSWORD@google.com?to=user@google.com
```

Then **Delivery channels → Test** (or `POST /api/v1/notifiers/test`).

## Delivery reliability

Every notification is durably queued (the outbox) before delivery is
attempted. Optional tuning lives under `defaults` in `app/releases.yaml` — see
[Configuration overview](overview.md):

- **Retry backoff.** Failed deliveries use exponential backoff (starting at
  the ~60s flush cadence, capped by `outbox_retry_backoff_max_secs`).
- **Digest batching.** Schedule-deferred releases for the same team and moment
  combine into one message.
- **Per-sink circuit breaker.** After `sink_breaker_failure_threshold`
  failures, a sink is skipped for `sink_breaker_cooldown_secs`.
- **Ops meta-alerts.** Set `defaults.ops_routing_tag` to a team tag that
  matches a notifier; when an outbox row becomes `dead` or a sink breaker
  opens, xrelease notifies that tag directly (not via the outbox, so a broken
  sink cannot recurse). Unset = metrics/UI only. In multi-org mode the tag is
  namespaced like other routing tags (`{org}::{tag}`). Live breaker state and
  deferred-outbox depth are on `GET /api/v1/status` (and
  `xrelease_outbox_deferred` / sink breaker gauges in Prometheus).
