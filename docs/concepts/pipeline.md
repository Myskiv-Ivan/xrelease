# Poll → diff → notify

## Poll path

1. **Fetch** upstream releases (HTTP with ETag caching)
2. **Filter** — prerelease and regex gates
3. **Diff** against `seen_release` (known version identities)
4. **Outbox** — queue a notification per new identity; record in `seen_release`
5. **Enrich** *(optional)* — if `[advisories]` is enabled, attach known CVEs /
   GHSAs for package versions (never blocks delivery) — see
   [Security advisories](../configuration/overview.md#security-advisories)
6. **Deliver** — send to matching sinks; each sink retries independently
7. **Complete** — mark outbox row sent on success

Failed deliveries stay in the outbox; a background flush claims due rows about
every 60s, and each failed attempt backs off exponentially before the next try.
Inline delivery and the flush loop use a short lease so they do not double-send
the same row. Scheduled (`notify_schedule`) releases are held until their cron
moment and combined into a digest when several land on the same moment — see
[Delivery reliability](../configuration/apprise.md#delivery-reliability).

## Webhook path

Same steps 2–7, but step 1 is replaced by parsing the inbound JSON payload,
deduplicating via `webhook_delivery`, and mapping to a configured watch.

## First run

All current releases are recorded as seen **without notifying** (baseline).
