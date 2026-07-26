# Webhook examples (inbound vs outbound)

| Direction | Config | Files |
|---|---|---|
| **Inbound** (forge → xrelease) | `XRELEASE_WEBHOOK_SECRET` + matching sources | `.env.example`, `bootstrap.toml`, [docs/api/webhooks.md](../../../docs/api/webhooks.md) |
| **Outbound** (xrelease → HTTP) | `[[notifiers]] type = "webhook"` or UI | [`outbound.yaml`](outbound.yaml), [`outbound.toml`](outbound.toml) |

UI: **Config → Edit → Delivery channels → Add channel → Webhook** (needs
**API + UI**). Paste snippets into a desired document for **Local** / **API**.

Compose webhook tests: `http://127.0.0.1:3000/api/v1/webhooks/…` (UI proxy).
