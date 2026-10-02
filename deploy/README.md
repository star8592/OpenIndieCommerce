# Production deployment

OpenIndieCommerce production deployment is intentionally small: one Rust binary, one SQLite database, file-backed secrets, systemd, and Caddy TLS termination.

## Layout
- binary: `/opt/openindiecommerce/bin/openindiecommerce-server`
- helpers: `/opt/openindiecommerce/deploy/`
- config: `/etc/openindiecommerce/openindiecommerce.env`
- secrets: `/etc/openindiecommerce/secrets/*` (`0600`, root-owned)
- database: `/var/lib/openindiecommerce/openindiecommerce.sqlite3`
- backups: `/var/backups/openindiecommerce/`

The service listens on localhost by default. Caddy is the public HTTPS edge and the example blocks `/v1/admin/*` from the public internet.

## First boot
1. Create system user/group `openindiecommerce` and the directories above.
2. Copy the release binary and deployment helpers.
3. Generate an admin token. Generate an entitlement key secret only if the optional license module is used.
4. Copy the environment example and configure at least one payment rail.
5. Run `deploy/scripts/preflight.sh`.
6. Enable the systemd service and backup timer, then configure Caddy.
7. Verify HTTPS `/health` and run a sandbox or low-value transaction before public sales.

## Backups
`backup_state.py` uses SQLite's online backup API, safe with WAL enabled. If the optional entitlement secret is configured it is included because deterministic license recovery depends on it.

Run `verify_backup.py <archive>` after copying a backup off-host, and periodically perform a restore drill on a disposable host.
