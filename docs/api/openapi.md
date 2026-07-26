# OpenAPI specification

xrelease exposes an **OpenAPI 3.0** document at runtime:

```
GET /openapi.json
```

When the UI is enabled, the same document is available through the nginx proxy:

```
GET http://127.0.0.1:3000/openapi.json
```

## View the spec

1. Start `xrelease serve` (or the Docker UI stack).
2. Open [Swagger Editor](https://editor.swagger.io/).
3. File → Import URL →
   - Compose / UI proxy: `http://127.0.0.1:3000/openapi.json`
   - Native binary: `http://127.0.0.1:8080/openapi.json`

`info.version` tracks the xrelease release. Breaking API changes bump the minor
version until 1.0.

Endpoint summary: [API overview](overview.md).
