# OpenIndieCommerce

Self-hosted commerce orchestration for indie developers and small teams.

OpenIndieCommerce gives websites, desktop software, digital products and SaaS one integration while keeping payment providers replaceable.

It is designed around a common indie-developer problem: overseas sales may be easiest through a Merchant of Record, while mainland-China sales may need a different local rail. Your application should not have to embed both providers' order models, webhook formats and fulfillment logic.

## What it is

```text
Your website / desktop app / SaaS
              |
              v
      OpenIndieCommerce
  Product / Price / Checkout
  Canonical Order / Events
  Signed merchant webhooks
  Optional Entitlements
              |
      +-------+-------+
      |               |
   Paddle / Dodo       ZPAY
      overseas      mainland China
```

OpenIndieCommerce is non-custodial. It does not hold customer funds or card data.
## v0.1 status

Implemented and tested:

- Product and Price records
- Checkout Sessions
- hosted Paddle checkout
- hosted Dodo Payments Checkout Sessions (test/live environments)
- hosted ZPAY Alipay / WeChat checkout
- provider webhook signature verification
- canonical `order.paid`, `order.refunded`, `order.chargeback`
- idempotent provider-order handling
- reliable merchant webhook outbox with HMAC signatures and retry
- checkout metadata round-trip for `userId`, campaign attribution, etc.
- optional license / entitlement module with activation limits
- merchant Terms / Privacy / Refund templates
- systemd + Caddy production templates
- SQLite WAL, online backup and backup verification

The project is still alpha. No claim is made that a live merchant account or payout path has been verified for every provider or jurisdiction.
## Quick start

```bash
export OIC_DB=/tmp/openindiecommerce.sqlite3
export OIC_ADMIN_TOKEN='development-admin-token'
export OIC_BIND=127.0.0.1:18791

# Public/legal merchant identity
export OIC_BRAND_NAME='Example Store'
export OIC_SELLER_LEGAL_NAME='Example Seller'
export OIC_SUPPORT_EMAIL='support@example.com'
export OIC_SUPPORT_PHONE='+1 555 0100'

cargo run -p openindiecommerce-server
```

Add only the payment rails you need. The optional license/entitlement module does not need to be configured for a normal web store.

See `docs/API.md` for Product, Price, Checkout Session and merchant webhook examples.

For payment-rail onboarding, `scripts/provider_acceptance.py` provides a fail-closed provider acceptance harness. Use `--provider all --dry-run` for a single readiness report covering Paddle, Dodo and ZPAY. For a specific configured rail, use `--provider paddle|dodo|zpay --dry-run`; running without `--dry-run` creates a disposable OpenIndieCommerce Product/Price/Checkout Session and returns the next browser/provider checkout URL. Dodo live acceptance still requires explicit `--allow-live`.

See `docs/PROVIDER_ONBOARDING.md` for account onboarding, sandbox/test setup, live cutover and payout/settlement gates.
## Security model

- Payment-provider API/webhook secrets stay server-side.
- Provider callbacks are verified before a canonical order is created or changed.
- Merchant webhooks are signed with HMAC-SHA256 over the raw body and timestamp.
- Order/provider IDs are unique and callbacks are idempotent.
- Admin APIs are bearer-protected and the production Caddy example blocks them from the public edge.
- SQLite uses WAL and production backups use the online backup API.
- License keys are optional; when used, server state stores hashes rather than relying on plaintext keys for normal validation.

## Non-goals for v0.1

- holding funds or replacing a licensed payment processor
- becoming a tax/Merchant-of-Record entity
- rebuilding enterprise payment routing across dozens of PSPs
- metered billing analytics (integrate with a billing engine such as Lago instead)
- multi-tenant hosted SaaS control plane

The initial goal is a small, auditable commerce layer an indie developer can self-host and understand.

## License

Apache-2.0. See `LICENSE`.
