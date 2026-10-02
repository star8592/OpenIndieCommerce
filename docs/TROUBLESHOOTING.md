# Troubleshooting

This guide covers the most common OpenIndieCommerce v0.1 problems.

Start by separating failures into four layers:

```text
1. OpenIndieCommerce process / database
2. OpenIndieCommerce configuration
3. provider checkout / provider webhook
4. merchant webhook / your fulfillment
```

Do not debug all four at once.

## 1. `/health` fails

Check the process:

```bash
systemctl status openindiecommerce --no-pager
journalctl -u openindiecommerce -n 200 --no-pager
```

Local development:

```bash
cargo run -p openindiecommerce-server
```

Check listener:

```bash
ss -ltnp | grep 18791
```

Check database path permissions:

```bash
ls -ld /var/lib/openindiecommerce
ls -l /var/lib/openindiecommerce/openindiecommerce.sqlite3*
```

`/health` performs a database health check. A failing database can therefore make health return non-200.

## 2. Public site returns merchant identity configuration error

The built-in public/legal pages require:

```text
OIC_SELLER_LEGAL_NAME
OIC_SUPPORT_EMAIL
OIC_SUPPORT_PHONE
```

Recommended also:

```text
OIC_BRAND_NAME
OIC_REFUND_DAYS
```

Production preflight intentionally fails when required public merchant information is missing.

## 3. Admin API returns 401

Admin endpoints require:

```http
Authorization: Bearer $OIC_ADMIN_TOKEN
```

If production uses:

```text
OIC_ADMIN_TOKEN_FILE
```

read the value from the configured secret file on the trusted admin machine/process. Do not expose the secret in browser JavaScript.

Example:

```bash
curl -i \
  -H "Authorization: Bearer $OIC_ADMIN_TOKEN" \
  http://127.0.0.1:18791/v1/admin/providers
```

Production Caddy may intentionally block admin endpoints from the public internet. In that case, use localhost/SSH/VPN rather than changing the edge to expose admin APIs.

## 4. `/v1/admin/providers` says provider is not configured

This endpoint checks whether required runtime configuration exists. It does not test payout eligibility or merchant approval.

### Paddle

Check:

```text
OIC_PADDLE_CLIENT_TOKEN
OIC_PADDLE_WEBHOOK_SECRET or OIC_PADDLE_WEBHOOK_SECRET_FILE
```

The Price must also contain a real Paddle `providerPriceId` when creating a real checkout.

### Dodo

Check:

```text
OIC_DODO_ENV=test|live
OIC_DODO_API_KEY or OIC_DODO_API_KEY_FILE
OIC_DODO_WEBHOOK_SECRET or OIC_DODO_WEBHOOK_SECRET_FILE
```

The Price must contain the Dodo product ID.

### ZPAY

Check:

```text
OIC_ZPAY_PID
OIC_ZPAY_KEY or OIC_ZPAY_KEY_FILE
OIC_PUBLIC_BASE_URL
```

ZPAY requires a public callback-reachable base URL for real payments.

## 5. Product/Price creation works but hosted checkout fails

This usually means the canonical core is working but the selected payment rail is incomplete.

Check:

1. Price `provider` value;
2. provider runtime configuration;
3. provider-specific `providerPriceId` when required;
4. correct currency;
5. public base URL where required.

Use:

```bash
python3 scripts/provider_acceptance.py \
  --base-url https://commerce.example.com \
  --provider all \
  --dry-run
```

## 6. Paddle page opens but checkout does not

Check the client token and provider Price ID.

A Paddle Sandbox token should begin with the expected test-token form used by your Paddle account. OpenIndieCommerce uses a `test_` client token to initialize the Paddle page in sandbox mode.

Do not mix Sandbox catalog IDs with Live credentials. Paddle Sandbox and Live catalog/configuration are separate.

Also check browser developer-console errors for Paddle JS initialization failures.

## 7. Dodo acceptance refuses to run Live

This is intentional.

The acceptance harness protects real-money Dodo environments.

For test mode:

```bash
OIC_DODO_ENV=test
```

Only run Live acceptance when you intentionally want a real-money validation and have reviewed the command. Use the explicit live override required by the harness.

Do not weaken this guard just to make a test command convenient.

## 8. ZPAY checkout rejects the Price

ZPAY hosted checkout requires CNY pricing.

Use:

```json
{
  "provider":"zpay",
  "currency":"CNY",
  "unitAmount":9900
}
```

where `9900` means ¥99.00.

Also verify PID, key and callback/public base URL.

## 9. Customer reaches success page but no fulfillment happens

This is often correct behavior while the provider webhook is still pending.

The success page is **not proof of payment**.

Check:

1. did provider webhook reach OpenIndieCommerce?
2. did signature verification succeed?
3. was a canonical `order.paid` created?
4. did OpenIndieCommerce enqueue merchant webhook delivery?
5. did your application return 2xx?
6. did your application process the event idempotently?

Never “fix” this by granting access from the browser redirect.

## 10. Merchant webhook is not arriving

List registered endpoints:

```http
GET /v1/admin/webhooks
```

Check that the endpoint is active.

Inspect deliveries:

```http
GET /v1/admin/webhook-deliveries?limit=100
```

Replay one delivery:

```http
POST /v1/admin/webhook-deliveries/{id}/redeliver
```

Common causes:

- DNS/TLS error at your app;
- receiver returns non-2xx;
- firewall;
- wrong URL;
- webhook endpoint disabled;
- application signature verification bug.

## 11. Merchant webhook signature verification fails

Verify using the exact raw request body received on the wire.

Do not:

```text
parse JSON
-> stringify JSON again
-> verify re-serialized bytes
```

Correct:

```text
raw bytes
-> verify HMAC
-> then parse JSON
```

Also enforce timestamp tolerance.

See `examples/web-store/server.mjs` for a working reference.

## 12. Duplicate webhook caused duplicate fulfillment

Your merchant webhook handler must be idempotent.

Persist `event.id` before/with fulfillment in one transaction where possible.

If the same event ID was already processed, return success without repeating the business effect.

OpenIndieCommerce itself normalizes provider callbacks idempotently, but your downstream application must also be idempotent.

## 13. Checkout returns HTTP 410

The Checkout Session is probably expired or no longer open.

Checkout Sessions expire after 30 minutes by default.

Create a new session rather than trying to reopen an expired one.

For custom TTL:

```json
{
  "expiresInSeconds":1800
}
```

## 14. Refund happened but customer still has access

Separate two layers:

```text
provider refund
-> OpenIndieCommerce order.refunded
-> merchant webhook
-> your app revokes local entitlement
```

Check each layer.

If using the optional OpenIndieCommerce entitlement/license module, check server-side revocation/validation state.

If your own app manages access, your webhook handler is responsible for revocation.

## 15. License activation limit can be exceeded

The current server-side activation count should be enforced atomically in the OpenIndieCommerce license service.

If you believe the limit was exceeded:

- record the exact license/order;
- record concurrent activation timestamps;
- do not try to enforce the fix only in the desktop client;
- add/reproduce a server-side concurrency regression test.

## 16. Desktop app works online but fails offline

Offline grace is a product policy, not a provider policy.

Check:

- last successful validation timestamp;
- local cache permissions;
- whether the app is accidentally treating any network failure as revocation;
- whether license status is actually revoked vs merely unverifiable.

A transient provider/OIC outage should not be confused with a refunded/revoked license unless your policy explicitly requires always-online validation.

## 17. SQLite is locked / busy

v0.1 uses SQLite WAL and a busy timeout.

Check for:

- multiple incompatible service instances writing the same database;
- manual tools holding long write transactions;
- filesystem issues;
- unsupported network filesystem behavior.

Do not run several independently upgraded schema writers against the same DB.

## 18. Backup verification fails

Treat this as a real production incident until understood.

Do not delete the last known-good backup.

Check:

- archive completeness;
- SQLite integrity result;
- expected tables;
- entitlement secret presence when required for deterministic license recovery;
- disk corruption or truncated copy.

Use the supplied backup verification script and perform restore drills.

## 19. Provider acceptance says `BLOCKED`

That is a deliberate fail-closed result, not necessarily a bug.

It usually means the requested provider runtime configuration is incomplete.

Check the onboarding guide:

[`PROVIDER_ONBOARDING.md`](PROVIDER_ONBOARDING.md)

Then rerun:

```bash
python3 scripts/provider_acceptance.py \
  --base-url https://commerce.example.com \
  --provider all \
  --dry-run
```

## 20. How to collect a useful bug report

Include:

```text
OpenIndieCommerce commit/release
OS/distribution
Rust version if building from source
provider (Paddle/Dodo/ZPAY)
test/sandbox/live context
HTTP route and status
sanitized server logs
whether /health succeeds
whether /v1/admin/providers says configured
whether provider webhook arrived
whether canonical order exists
whether merchant webhook delivery exists
```

Never include:

```text
API keys
webhook secrets
admin token
payout credentials
identity documents
full license keys
customer payment data
```
