# OIDC / SSO

Optional single sign-on for the dashboard. Default install uses
[local username/password](authentication.md); keep **`XRELEASE_API_KEY`** for
`xrctl` and scripts.

xrelease does **not** ship an identity provider. Point it at your existing
OpenID Connect IdP (Keycloak, Authentik, Okta, Microsoft Entra ID, …).

With OIDC enabled, the UI completes the IdP login, then calls
`POST /api/v1/auth/oidc/sync`. That endpoint validates the access token,
**creates or updates** the dashboard user, and assigns an application role from
IdP groups/claims.

## 1. Register a client in your IdP

| Setting | Value |
|---|---|
| Client ID | `xrelease-ui` (or your choice — must match env below) |
| Client type | Public (SPA) |
| Flow | Authorization Code + **PKCE** |
| Redirect URI | `https://<xrelease-host>/login/callback` |
| Logout / post-logout | optional; browser origin of the UI |
| Groups / roles claim | e.g. `groups`, `roles`, or Keycloak `realm_access.roles` |

Compose lab host example: `http://127.0.0.1:3000/login/callback`.  
Production: HTTPS only — see [TLS](tls.md).

## 2. Backend (`XRELEASE_OIDC_*`)

Same keys as [`.env.example`](../../.env.example). Compose: `.env`.  
Kubernetes: Secret (`secret.example.yaml`) or chart `secrets.existingSecret`.

```bash
XRELEASE_OIDC_ISSUER=https://auth.example.com/realms/xrelease
XRELEASE_OIDC_JWKS_URI=https://auth.example.com/realms/xrelease/protocol/openid-connect/certs
XRELEASE_OIDC_AUDIENCE=xrelease-ui
# When discovery/JWKS host differs from browser-facing iss (split DNS / mesh):
# XRELEASE_OIDC_DISCOVERY_URL=https://keycloak.internal/realms/xrelease
# XRELEASE_OIDC_REQUIRED_SCOPE=xrelease:manage

XRELEASE_OIDC_ROLE_CLAIM=groups
XRELEASE_OIDC_ROLE_ADMIN=xrelease-admin,admin
XRELEASE_OIDC_ROLE_OPERATOR=xrelease-operator,operator
XRELEASE_OIDC_ROLE_VIEWER=xrelease-viewer,viewer
# XRELEASE_OIDC_DEFAULT_ROLE=viewer
```

The **backend container/pod** must reach `XRELEASE_OIDC_JWKS_URI` (or discovery).

## 3. UI (`VITE_*`, runtime — no image rebuild)

Applied when the UI container starts (`/ui-config.js`). Compose: `.env` → `ui`
service. Helm: set `ui.env.VITE_AUTH_MODE` / `VITE_OIDC_*` in a local overlay —
[deployment variants](deployment-variants.md).

```bash
VITE_AUTH_MODE=oidc          # or hybrid = local password + SSO
VITE_OIDC_ISSUER=https://auth.example.com/realms/xrelease
VITE_OIDC_CLIENT_ID=xrelease-ui
VITE_OIDC_SCOPES=openid,profile,email,groups
VITE_OIDC_ROLE_CLAIM=groups
VITE_OIDC_ROLE_ADMIN=xrelease-admin,admin
VITE_OIDC_ROLE_OPERATOR=xrelease-operator,operator
VITE_OIDC_ROLE_VIEWER=xrelease-viewer,viewer
# VITE_OIDC_REDIRECT_URI=https://xrelease.example.com/login/callback
```

Keep UI role aliases in sync with `XRELEASE_OIDC_ROLE_*`. The **server** mapping
is authoritative for the role stored after login.

Restart the UI after changing `VITE_*` (Compose / `helm upgrade`).

## 4. Fail-closed API

```toml
[api]
require_auth = true
listen = "0.0.0.0:8080"
```

## Provider recipes

Values below are typical starting points — adjust realm/tenant/paths to your
IdP. After changing env, restart backend + UI.

### Keycloak

| | |
|---|---|
| Issuer | `https://<keycloak>/realms/<realm>` |
| JWKS | `https://<keycloak>/realms/<realm>/protocol/openid-connect/certs` |
| Client | Public client, Standard flow + PKCE, Valid redirect URIs as above |
| Roles claim | Often `realm_access.roles` or a custom `groups` mapper |

```bash
XRELEASE_OIDC_ISSUER=https://keycloak.example.com/realms/xrelease
XRELEASE_OIDC_JWKS_URI=https://keycloak.example.com/realms/xrelease/protocol/openid-connect/certs
XRELEASE_OIDC_AUDIENCE=xrelease-ui
XRELEASE_OIDC_ROLE_CLAIM=realm_access.roles
VITE_AUTH_MODE=oidc
VITE_OIDC_ISSUER=https://keycloak.example.com/realms/xrelease
VITE_OIDC_CLIENT_ID=xrelease-ui
VITE_OIDC_ROLE_CLAIM=realm_access.roles
```

Create IdP roles/groups matching `xrelease-admin` / `xrelease-operator` /
`xrelease-viewer` (or change the `*_ROLE_*` aliases).

### Authentik

| | |
|---|---|
| Issuer | Application launch URL / provider issuer (e.g. `https://authentik.example.com/application/o/xrelease/`) |
| JWKS | `{issuer}jwks/` (trailing slash per Authentik) |
| Client | OAuth2/OIDC provider, PKCE, redirect URI as above |
| Groups | Map Authentik groups into the access token claim you set as `ROLE_CLAIM` |

```bash
XRELEASE_OIDC_ISSUER=https://authentik.example.com/application/o/xrelease/
XRELEASE_OIDC_JWKS_URI=https://authentik.example.com/application/o/xrelease/jwks/
XRELEASE_OIDC_AUDIENCE=xrelease-ui
VITE_OIDC_ISSUER=https://authentik.example.com/application/o/xrelease/
VITE_OIDC_CLIENT_ID=xrelease-ui
```

### Okta

| | |
|---|---|
| Issuer | `https://<org>.okta.com/oauth2/default` (or custom auth server) |
| JWKS | `https://<org>.okta.com/oauth2/default/v1/keys` |
| Client | SPA / OIDC, PKCE, Sign-in redirect URI |
| Groups | Add a groups claim to the access token; set `ROLE_CLAIM` accordingly |

```bash
XRELEASE_OIDC_ISSUER=https://dev-xxxxx.okta.com/oauth2/default
XRELEASE_OIDC_JWKS_URI=https://dev-xxxxx.okta.com/oauth2/default/v1/keys
XRELEASE_OIDC_AUDIENCE=api://default
# or your custom API audience / client id as configured in Okta
VITE_OIDC_ISSUER=https://dev-xxxxx.okta.com/oauth2/default
VITE_OIDC_CLIENT_ID=<okta_spa_client_id>
```

Align `XRELEASE_OIDC_AUDIENCE` with the access-token `aud` Okta issues.

### Microsoft Entra ID (Azure AD)

| | |
|---|---|
| Issuer | `https://login.microsoftonline.com/<tenant-id>/v2.0` |
| JWKS | `https://login.microsoftonline.com/<tenant-id>/discovery/v2.0/keys` |
| Client | App registration, SPA platform, redirect URI, PKCE |
| Audience | Application (client) ID URI or API app ID — must match token `aud` |
| Roles / groups | Optional group claims or app roles; map via `ROLE_CLAIM` |

```bash
XRELEASE_OIDC_ISSUER=https://login.microsoftonline.com/<tenant-id>/v2.0
XRELEASE_OIDC_JWKS_URI=https://login.microsoftonline.com/<tenant-id>/discovery/v2.0/keys
XRELEASE_OIDC_AUDIENCE=<api-app-client-id>
VITE_OIDC_ISSUER=https://login.microsoftonline.com/<tenant-id>/v2.0
VITE_OIDC_CLIENT_ID=<spa-app-client-id>
VITE_OIDC_SCOPES=openid,profile,email,api://<api-app-client-id>/access
```

Expose an API scope on the API app registration and request it from the SPA so
the access token is audience-bound for the backend.

## How users and roles are assigned

1. User signs in at the IdP (Authorization Code + PKCE).
2. UI stores the IdP access token and calls `POST /api/v1/auth/oidc/sync`.
3. Backend validates the JWT, reads `sub` (and email / name when present).
4. **Upsert** the dashboard user by IdP `sub` (create on first login, update later).
5. Role from claim path (`XRELEASE_OIDC_ROLE_CLAIM`): match admin / operator /
   viewer aliases; highest wins; else `XRELEASE_OIDC_DEFAULT_ROLE` (`viewer`).

| IdP group / claim value (defaults) | Application role |
|---|---|
| `xrelease-admin`, `admin` | `admin` (global) |
| `xrelease-operator`, `operator` | `operator` (global) |
| `xrelease-viewer`, `viewer` | `viewer` (global) |
| `xrelease-admin:platform` (or `admin:platform`) | `admin` **only** on org `platform` |
| `xrelease-operator:security` | `operator` on org `security` |

Bare aliases set the **global** role. `alias:org` grants apply only to that
organization. After login the UI calls the API with the **OIDC access token**.
`XRELEASE_API_KEY` remains for automation.

Admins can **link** an IdP `sub` onto a local password user
(`POST /api/v1/auth/users/{id}/oidc`) — see [Authentication](authentication.md).

## Troubleshooting

| Symptom | Check |
|---|---|
| SSO button missing | `VITE_AUTH_MODE=oidc\|hybrid` + `VITE_OIDC_ISSUER` / `CLIENT_ID` on **UI**; restart UI |
| UI login loop | Redirect URI matches IdP registration exactly (scheme/host/path) |
| 401 on API after login | `XRELEASE_OIDC_ISSUER` = JWT `iss`; `AUDIENCE` = token `aud` |
| JWKS fetch error | `XRELEASE_OIDC_JWKS_URI` reachable from the backend network |
| Wrong role | IdP claim vs `XRELEASE_OIDC_ROLE_*` (and matching `VITE_OIDC_ROLE_*`) |
| User missing in DB | Sync failed — backend logs for `/auth/oidc/sync` |

See also [Authentication](authentication.md) and [HTTP API settings](../configuration/api.md).
