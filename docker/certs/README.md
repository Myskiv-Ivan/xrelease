# TLS certificate + private key for `docker/compose.tls.yaml`
#
# Required files (gitignored — do not commit real secrets):
#
#   cert.pem   — certificate (leaf + intermediates if needed)
#   key.pem    — private key
#
# Mounted into the UI nginx container as /certs/cert.pem and /certs/key.pem.
# Paths are set in `.env`:
#
#   XRELEASE_TLS_CERT=./docker/certs/cert.pem
#   XRELEASE_TLS_KEY=./docker/certs/key.pem
#   XRELEASE_TLS_HTTP_PORT=80
#   XRELEASE_TLS_HTTPS_PORT=443
#   XRELEASE_PUBLIC_HOST=xrelease.local   # for /etc/hosts / DNS (must match cert)
#
# Lab self-signed:
#
#   openssl req -x509 -nodes -newkey rsa:2048 -days 365 \
#     -keyout key.pem -out cert.pem \
#     -subj "/CN=xrelease.local"
#
#   docker compose -f docker-compose.yaml -f docker/compose.tls.yaml up -d
#
# Docs: docs/operations/tls.md
