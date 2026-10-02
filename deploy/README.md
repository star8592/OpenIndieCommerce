# Production deployment assets

This directory contains the concrete files used by the OpenIndieCommerce single-node production deployment.

For the full installation procedure, topology, secret handling, Caddy, systemd, backups, monitoring and launch checklist, read:

**[`../docs/DEPLOYMENT.md`](../docs/DEPLOYMENT.md)**

## Intended topology

```text
Internet
   |
 Caddy HTTPS
   |
127.0.0.1:18791
   |
OpenIndieCommerce
   |
SQLite WAL
```

OpenIndieCommerce production deployment is intentionally small: one Rust binary, one SQLite database, file-backed secrets, systemd and Caddy TLS termination.

## Layout

- binary: `/opt/openindiecommerce/bin/openindiecommerce-server`
- helpers: `/opt/openindiecommerce/deploy/`
- config: `/etc/openindiecommerce/openindiecommerce.env`
- secrets: `/etc/openindiecommerce/secrets/*`
- database: `/var/lib/openindiecommerce/openindiecommerce.sqlite3`
- backups: `/var/backups/openindiecommerce/`

The service should listen on localhost. Caddy is the public HTTPS edge. The supplied edge example is intended to keep `/v1/admin/*` off the public internet.

## Directory contents

```text
deploy/
  caddy/      reverse-proxy / HTTPS example
  env/        production environment template
  scripts/    preflight, backup and verification helpers
  systemd/    service / timer templates
```

## First boot summary

1. Create the `openindiecommerce` system user/group.
2. Create the production directories.
3. Install a tested release binary.
4. Create file-backed secrets.
5. Copy and fill `env/openindiecommerce.env.example`.
6. Configure at least one payment rail.
7. Run `scripts/preflight.sh`.
8. Install/enable systemd service and backup timer.
9. Configure Caddy and HTTPS.
10. Verify `/health`.
11. Run provider sandbox/test acceptance.
12. Complete a low-value real transaction before public launch.

## Backups

`backup_state.py` uses SQLite's online backup API, which is safe with WAL enabled. If deterministic entitlement/license recovery depends on the entitlement secret, that secret is part of the recovery set.

Always verify copied backups and periodically perform a restore drill on a disposable host.

See [`../docs/DEPLOYMENT.md`](../docs/DEPLOYMENT.md) for the complete operational procedure.
