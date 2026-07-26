# Security Policy

## Supported versions

The latest released `0.x` line receives security fixes. Pin to a tagged
release for production and watch GHCR for updates.

## Reporting a vulnerability

Please **do not** open public issues for security problems.

- Use GitHub **Private vulnerability reporting** on
  [Myskiv-Ivan/xrelease](https://github.com/Myskiv-Ivan/xrelease/security/advisories/new), or
- email the maintainers listed in the repository profile.

Include affected version/commit, reproduction steps, and impact. We aim to
acknowledge within 72 hours and to ship a fix or mitigation as soon as
practical, crediting reporters unless anonymity is requested.

## Hardening notes

- Keep secrets in `.env` / your secret manager — never in `bootstrap.toml` or `app/releases.yaml`.
- **`api.require_auth` defaults to `true`:** `xrelease serve` refuses to start
  unless `XRELEASE_API_KEY` / `[api].api_key`, OIDC (`issuer`), or local UI
  auth (`XRELEASE_SESSION_SECRET`) is configured. Set `require_auth = false`
  only on trusted lab networks; `validate` still warns when the API is open.
- Set `XRELEASE_WEBHOOK_SECRET` to enforce webhook signature verification.
- Do not copy lab placeholders from `.env.example` into production — generate
  fresh values (`openssl rand -hex 32`, `openssl rand -base64 32`).
- Terminate **HTTPS at Ingress / reverse proxy** — pods speak HTTP on the
  cluster network. See [`docs/operations/tls.md`](docs/operations/tls.md).
- For managed PostgreSQL set `XRELEASE_DATABASE_SSL_MODE=require` (or
  `verify-full` + `XRELEASE_DATABASE_SSL_ROOT_CERT`) — see [`docs/operations/tls.md`](docs/operations/tls.md).
- Released images are signed with cosign (keyless) and ship SBOM + provenance:

  ```sh
  cosign verify ghcr.io/myskiv-ivan/xrelease:<tag> \
    --certificate-identity-regexp 'https://github.com/Myskiv-Ivan/xrelease/.+' \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com
  ```
