# syntax=docker/dockerfile:1

# ---- Build stage -----------------------------------------------------------
# linux/amd64 only (release + CI). Debian Trixie matches distroless/cc-debian13
# glibc. PostgreSQL client uses vendored OpenSSL for optional TLS.
# Must match rust-toolchain.toml / CI toolchain (Dependabot ignores `rust` bumps).
FROM rust:1.96-trixie AS build
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
         pkg-config \
         libssl-dev \
         libsasl2-dev \
         zlib1g-dev \
         cmake \
         perl \
         make \
         binutils \
    && rm -rf /var/lib/apt/lists/*

# Cache dependencies: copy manifests first, build a stub, then the real sources.
# The root manifest is a workspace whose only member is `cli/`, so that member's
# manifest + a stub entrypoint must exist for any root cargo command to resolve.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY cli/Cargo.toml ./cli/Cargo.toml
RUN mkdir -p src cli/src \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs \
    && echo 'fn main() {}' > cli/src/main.rs \
    && cargo build --release --locked -p xrelease || true

COPY src ./src
COPY cli/src ./cli/src
COPY api/openapi.json api/openapi.json
# Two binaries share one dependency compile: the daemon (`xrelease`) with every
# notifier sink, and the lean API client (`xrctl`, no daemon deps) so operators
# can `docker exec … xrctl apply` against the instance without exposing the API.
RUN touch src/main.rs src/lib.rs cli/src/main.rs \
    && cargo build --release --locked -p xrelease \
    && cargo build --release --locked -p xrelease-cli \
    && install -D target/release/xrelease /app/xrelease \
    && install -D target/release/xrctl /app/xrctl \
    && strip --strip-unneeded /app/xrelease /app/xrctl

# ---- Filesystem skeleton (distroless has no shell / useradd) ----------------
FROM debian:trixie-slim AS prepare
RUN mkdir -p /data /etc/xrelease \
    && chown -R 65532:65532 /data /etc/xrelease

# ---- Runtime stage ---------------------------------------------------------
# Distroless cc: glibc + libgcc + CA bundle, nonroot UID 65532. No shell/package
# manager — smaller attack surface than debian-slim (~tens of MB vs ~80+ MB).
FROM gcr.io/distroless/cc-debian13:nonroot AS runtime

COPY --from=prepare --chown=65532:65532 /data /data
COPY --from=prepare --chown=65532:65532 /etc/xrelease /etc/xrelease
COPY --from=build --chown=65532:65532 /app/xrelease /usr/local/bin/xrelease
COPY --from=build --chown=65532:65532 /app/xrctl /usr/local/bin/xrctl

WORKDIR /data

# Distroless :nonroot already defaults to UID 65532; declare USER for scanners (DS-0002).
USER nonroot

# Set XRELEASE_DATABASE_URL at runtime (e.g. postgres://user:pass@host:5432/db).
# Do NOT bake XRELEASE_APP_CONFIG: multi-org UI/API boots from [[organizations]] +
# ledger; single-doc mounts set --app / XRELEASE_APP_CONFIG explicitly.
ENV XRELEASE_CONFIG=/etc/xrelease/bootstrap.toml \
    XRELEASE_LOG=info

HEALTHCHECK --interval=60s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/xrelease", "--config", "/etc/xrelease/bootstrap.toml", "health"]

ENTRYPOINT ["/usr/local/bin/xrelease"]
CMD ["serve"]
