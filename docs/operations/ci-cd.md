# CI/CD integration

How **your** pipelines talk to a running xrelease instance.

Use the lean **`xrelease-cli`** image (or the `xrctl` binary) against a deployed
`xrelease serve`. See also [CLI — CI/CD](../api/cli.md#cicd-separate-from-the-server).

Do **not** use Docker-in-Docker: the job image *is* `xrelease-cli`, so you run
`xrctl` directly.

## Prerequisites

1. xrelease deployed ([Docker](../getting-started/docker.md) /
   [Kubernetes](../getting-started/kubernetes.md)).
2. API reachable from the runner (UI proxy URL or Ingress).
3. Secret `XRELEASE_API_KEY` (same value as on the server).
4. Desired YAML in the consumer repo (e.g. `app/releases.yaml`).

`[config_api]` must allow apply (`source = "api"`). Details:
[authoring variants](../configuration/overview.md#authoring-variants).

## GitLab CI

Job image = `xrelease-cli` (contains `xrctl` + shell). No DinD, no nested
`docker run`.

```yaml
# .gitlab-ci.yml
xrelease-apply:
  image: ghcr.io/myskiv-ivan/xrelease-cli:latest
  rules:
    - if: $CI_COMMIT_BRANCH == "main"
      changes: ["app/releases.yaml"]
  script:
    - xrctl --api-url "$XRELEASE_URL" --api-key "$XRELEASE_API_KEY" validate app/releases.yaml
    - xrctl --api-url "$XRELEASE_URL" --api-key "$XRELEASE_API_KEY" apply app/releases.yaml --if-match none --label "$CI_COMMIT_SHA"
```

Set `XRELEASE_URL` / `XRELEASE_API_KEY` in GitLab CI/CD variables (masked).
Pin a release tag instead of `latest` in production.

## GitHub Actions

Checkout needs the host runner (git/Node). Then run the CLI container once —
hosted runners already have Docker; still no DinD service.

```yaml
# .github/workflows/xrelease-apply.yml
name: xrelease-apply
on:
  push:
    branches: [main]
    paths: ["app/releases.yaml"]

jobs:
  apply:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Validate + apply
        env:
          XRELEASE_API_KEY: ${{ secrets.XRELEASE_API_KEY }}
          XRELEASE_URL: ${{ vars.XRELEASE_URL }}
        run: |
          docker run --rm -v "$PWD:/work" -w /work \
            ghcr.io/myskiv-ivan/xrelease-cli:latest \
            xrctl --api-url "$XRELEASE_URL" --api-key "$XRELEASE_API_KEY" \
            validate app/releases.yaml
          docker run --rm -v "$PWD:/work" -w /work \
            ghcr.io/myskiv-ivan/xrelease-cli:latest \
            xrctl --api-url "$XRELEASE_URL" --api-key "$XRELEASE_API_KEY" \
            apply app/releases.yaml --if-match none --label "${GITHUB_SHA}"
```

Store `XRELEASE_API_KEY` as an Actions **secret**. Prefer a HTTPS URL.

## Binary on the runner (no container)

```bash
curl -sSL -o xrctl.tgz \
  "https://github.com/Myskiv-Ivan/xrelease/releases/latest/download/xrctl-<ver>-linux-amd64.tar.gz"
tar xzf xrctl.tgz
./xrctl --api-url "$XRELEASE_URL" --api-key "$XRELEASE_API_KEY" \
  apply app/releases.yaml --if-match none --label "$CI_COMMIT_SHA"
```

Replace `<ver>` with the version from
[GitHub Releases](https://github.com/Myskiv-Ivan/xrelease/releases).

## Network notes

| Runner location | API URL |
|---|---|
| Same Compose host | `http://127.0.0.1:3000` (UI proxy) |
| Cluster job | in-cluster Service / Ingress URL |
| External SaaS runner | public HTTPS Ingress (see [TLS](tls.md)) |

Allow the runner egress to that URL only; do not expose Postgres or Apprise
publicly.

## Related

- [Deployment](deployment.md) · [Authentication](authentication.md) · [CLI](../api/cli.md)
