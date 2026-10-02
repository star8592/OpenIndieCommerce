# Production Deployment

This document describes the intended single-node production deployment for OpenIndieCommerce v0.1.

The design is deliberately small and auditable:

```text
Internet
   |
 Caddy :443
   |
127.0.0.1:18791
   |
OpenIndieCommerce Rust server
   |
SQLite WAL
```

For v0.1, this is preferred over prematurely introducing Kubernetes, Redis or PostgreSQL.

## 1. Production layout

Recommended paths:

```text
/opt/openindiecommerce/bin/openindiecommerce-server
/opt/openindiecommerce/deploy/
/etc/openindiecommerce/openindiecommerce.env
/etc/openindiecommerce/secrets/
/var/lib/openindiecommerce/openindiecommerce.sqlite3
/var/backups/openindiecommerce/
```

Recommended ownership:

```text
service user: openindiecommerce
service group: openindiecommerce
```

Secret files should normally be root-owned `0600` and loaded through systemd/environment file references according to your host policy.

## 2. Build the server

From a clean checkout:

```bash
cargo build -p openindiecommerce-server --release
```

Binary:

```text
target/release/openindiecommerce-server
```

Run the full quality gate before shipping:

```bash
cargo fmt --all -- --check
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 -m unittest discover -s scripts/tests -v
node --check examples/web-store/server.mjs
git diff --check
```

## 3. Create directories

Example Linux commands; adapt to your distribution and security policy:

```bash
sudo useradd --system --home /var/lib/openindiecommerce --shell /usr/sbin/nologin openindiecommerce || true

sudo install -d -o root -g root -m 0755 /opt/openindiecommerce/bin
sudo install -d -o root -g root -m 0755 /opt/openindiecommerce/deploy
sudo install -d -o root -g root -m 0755 /etc/openindiecommerce
sudo install -d -o root -g root -m 0700 /etc/openindiecommerce/secrets
sudo install -d -o openindiecommerce -g openindiecommerce -m 0750 /var/lib/openindiecommerce
sudo install -d -o openindiecommerce -g openindiecommerce -m 0750 /var/backups/openindiecommerce
```

Install the binary:

```bash
sudo install -o root -g root -m 0755 \
  target/release/openindiecommerce-server \
  /opt/openindiecommerce/bin/openindiecommerce-server
```

Copy deployment helpers from `deploy/`.

## 4. Configure environment

Start from:

```text
deploy/env/openindiecommerce.env.example
```

Recommended production base:

```bash
OIC_BIND=127.0.0.1:18791
OIC_DB=/var/lib/openindiecommerce/openindiecommerce.sqlite3
OIC_ADMIN_TOKEN_FILE=/etc/openindiecommerce/secrets/admin-token
OIC_PUBLIC_BASE_URL=https://commerce.example.com
RUST_LOG=info,openindiecommerce_server=info

OIC_BRAND_NAME=Example Store
OIC_SELLER_LEGAL_NAME=Your Legal Name
OIC_SUPPORT_EMAIL=support@example.com
OIC_SUPPORT_PHONE=+86 ...
OIC_REFUND_DAYS=14
```

Only configure the payment rails you use.

### Paddle

```bash
OIC_PADDLE_CLIENT_TOKEN=...
OIC_PADDLE_WEBHOOK_SECRET_FILE=/etc/openindiecommerce/secrets/paddle-webhook-secret
```

### Dodo

Start in test mode:

```bash
OIC_DODO_ENV=test
OIC_DODO_API_KEY_FILE=/etc/openindiecommerce/secrets/dodo-api-key
OIC_DODO_WEBHOOK_SECRET_FILE=/etc/openindiecommerce/secrets/dodo-webhook-secret
```

Only after acceptance succeeds:

```bash
OIC_DODO_ENV=live
```

### ZPAY

```bash
OIC_ZPAY_PID=...
OIC_ZPAY_KEY_FILE=/etc/openindiecommerce/secrets/zpay-key
```

ZPAY also needs a correct public HTTPS base URL for asynchronous callbacks.

## 5. Generate secrets

Example:

```bash
openssl rand -hex 32 | sudo tee /etc/openindiecommerce/secrets/admin-token >/dev/null
sudo chmod 600 /etc/openindiecommerce/secrets/admin-token
```

If using entitlement/license issuance, generate the entitlement secret as well:

```bash
openssl rand -hex 32 | sudo tee /etc/openindiecommerce/secrets/entitlement-key-secret >/dev/null
sudo chmod 600 /etc/openindiecommerce/secrets/entitlement-key-secret
```

Then enable:

```bash
OIC_ENTITLEMENT_KEY_SECRET_FILE=/etc/openindiecommerce/secrets/entitlement-key-secret
```

Never commit production secret contents.

## 6. Run preflight

Before starting the public service:

```bash
bash deploy/scripts/preflight.sh
```

Preflight should fail closed when required production configuration is missing.

Do not override a failure without understanding the missing requirement.

## 7. systemd

The repository contains systemd templates under:

```text
deploy/systemd/
```

Install the service and backup timer according to the files in that directory.

Typical lifecycle:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now openindiecommerce
sudo systemctl status openindiecommerce --no-pager
```

Watch logs:

```bash
sudo journalctl -u openindiecommerce -f
```

The service should bind only to:

```text
127.0.0.1:18791
```

Confirm:

```bash
ss -ltnp | grep 18791
```

## 8. Caddy / HTTPS

Use the supplied Caddy example under:

```text
deploy/caddy/
```

Production requirements:

- valid HTTPS certificate;
- provider webhook endpoints publicly reachable;
- public checkout routes reachable;
- `/health` may be exposed for monitoring according to your policy;
- `/v1/admin/*` should **not** be publicly reachable;
- reverse proxy should forward client IP headers consistently because v0.1 public rate limiting uses trusted proxy headers.

Verify from outside the server:

```bash
curl -i https://commerce.example.com/health
```

Verify admin API is blocked from public internet:

```bash
curl -i https://commerce.example.com/v1/admin/providers
```

It should not expose admin functionality even if the caller knows the path.

Perform admin operations through localhost, a secure SSH tunnel, VPN, or another authenticated management plane.

## 9. Provider webhook URLs

Typical public routes:

```text
https://commerce.example.com/v1/webhooks/paddle
https://commerce.example.com/v1/webhooks/dodo
https://commerce.example.com/v1/webhooks/zpay
```

Use the exact provider configuration required by each payment platform.

Do not point provider webhooks at a browser frontend route.

## 10. Provider acceptance before launch

From a trusted admin machine:

```bash
export OIC_ACCEPTANCE_ADMIN_TOKEN='...'

python3 scripts/provider_acceptance.py \
  --base-url https://commerce.example.com \
  --provider all \
  --dry-run
```

Then test one provider at a time.

Do not switch from test/sandbox to live simply because configuration is present.

Required evidence before calling a provider production-verified:

```text
provider test/sandbox checkout
verified provider webhook
canonical order.paid
merchant webhook
actual fulfillment
refund/reversal path
low-value live sale
real payout/settlement
```

## 11. SQLite and WAL

The production server uses SQLite with WAL mode.

Do not back up a live WAL database by casually copying only the `.sqlite3` file.

Use the supplied online backup tooling.

Database location:

```text
/var/lib/openindiecommerce/openindiecommerce.sqlite3
```

## 12. Backups

The repository includes online backup and verification scripts under:

```text
deploy/scripts/
```

The backup process should include:

- SQLite consistent online backup;
- entitlement secret when deterministic license recovery depends on it;
- metadata needed to restore the service.

After creating/copying a backup, run the verification script.

Do not consider a backup strategy complete until you perform a restore drill on a disposable machine.

Recommended policy:

```text
daily local backup
+ encrypted off-host copy
+ retention policy
+ periodic restore drill
```

## 13. Monitoring

Minimum monitoring:

```text
HTTPS /health
systemd service status
disk space
backup success
backup verification
provider webhook delivery failures
merchant webhook pending/retry growth
SQLite file growth
```

`/health` performs a database health check; it is not merely a static `ok` endpoint.

## 14. Upgrades

Before upgrading:

1. create and verify backup;
2. record current Git/release version;
3. build the new binary;
4. run tests;
5. stop the service;
6. replace binary;
7. restart;
8. check `/health`;
9. inspect logs;
10. run a low-risk Checkout/acceptance smoke if the release touched commerce/provider logic.

Do not run two unrelated schema-changing binaries against the same SQLite database at the same time.

## 15. Production launch checklist

- [ ] HTTPS domain works
- [ ] Rust server only binds localhost
- [ ] admin API not public
- [ ] legal/support pages contain real merchant information
- [ ] Terms/Privacy/Refund pages accessible
- [ ] secrets use files or secure secret store
- [ ] at least one payment rail configured
- [ ] provider test/sandbox acceptance passed
- [ ] provider webhook signature path passed
- [ ] merchant webhook fulfillment passed
- [ ] refund/reversal tested
- [ ] SQLite backup enabled
- [ ] backup verification passed
- [ ] restore drill completed
- [ ] low-value live purchase completed
- [ ] payout/settlement observed before claiming production-verified status
