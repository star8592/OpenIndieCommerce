# OpenIndieCommerce

Self-hosted commerce orchestration for indie developers and small teams.

**中文文档：[`README.zh-CN.md`](README.zh-CN.md)**

OpenIndieCommerce gives websites, desktop software, digital products and SaaS one commerce integration while keeping payment providers replaceable. It is designed around a common indie-developer problem: overseas sales may be easiest through a Merchant of Record, while mainland-China sales may need a different local rail. Your application should not have to embed several providers' checkout models, webhook formats, refund semantics and license delivery logic.

> **Status:** alpha. Core code, provider adapters, webhook normalization, entitlements, deployment templates and CI are implemented and tested. A provider/jurisdiction is not considered production-verified until a real transaction and real payout/settlement have both succeeded.

## Table of contents

- [What OpenIndieCommerce does](#what-openindiecommerce-does)
- [Who should use it](#who-should-use-it)
- [Architecture](#architecture)
- [Supported payment rails](#supported-payment-rails)
- [Choose your integration](#choose-your-integration)
- [10-minute local quick start](#10-minute-local-quick-start)
- [Create your first product and checkout](#create-your-first-product-and-checkout)
- [Configure a payment provider](#configure-a-payment-provider)
- [Use it from a web independent site](#use-it-from-a-web-independent-site)
- [Use it for desktop software licensing](#use-it-for-desktop-software-licensing)
- [Production deployment](#production-deployment)
- [Provider acceptance testing](#provider-acceptance-testing)
- [Security model](#security-model)
- [Documentation map](#documentation-map)
- [Current limitations](#current-limitations)

## What OpenIndieCommerce does

OpenIndieCommerce is the commerce layer between your product and payment providers.

It owns the provider-neutral concepts that your application should care about:

- Product
- Price
- Checkout Session
- canonical Order
- `order.paid`
- `order.refunded`
- `order.chargeback`
- signed merchant webhooks
- optional entitlements / license keys / device activation

Provider adapters translate Paddle, Dodo Payments and ZPAY into those canonical concepts.

OpenIndieCommerce **does not** hold customer funds, replace a payment processor, or store card data.

## Who should use it

Good fits include:

- an independent website selling reports, PDFs, datasets or downloads;
- a desktop application selling a Pro license;
- a SaaS application that wants payment-provider independence;
- a Chinese mainland developer who wants an overseas MoR plus a domestic Alipay/WeChat rail;
- a software factory that launches many small products and does not want to rebuild checkout, webhooks, refunds and licensing for every product.

Example products:

```text
research report / PDF / ZIP / CSV
online course / download bundle
SaaS Pro plan
API entitlement
Windows/macOS/Linux desktop license
private dataset
paid membership
```

## Architecture

```text
Your website / desktop app / SaaS
              |
              |  create Checkout Session
              v
      OpenIndieCommerce
  Product / Price / Checkout
  Canonical Order / Events
  Merchant webhook outbox
  Optional Entitlements
              |
      +-------+-------+
      |       |       |
   Paddle    Dodo    ZPAY
 overseas   overseas  China
   MoR       MoR     direct
```

Payment success is always determined by a **verified provider webhook**, never by a browser success redirect.

The normal flow is:

```text
1. Admin creates Product + Price
2. Your app creates a Checkout Session
3. Customer opens /checkout/{sessionId}
4. Provider completes payment
5. Provider webhook reaches OpenIndieCommerce
6. OpenIndieCommerce verifies signature, amount, currency and idempotency
7. Canonical order.paid is created
8. Signed merchant webhook is delivered to your application
9. Your application fulfills the order
```

## Supported payment rails

| Provider | Intended use | Current adapter status | Provider-side verification |
|---|---|---|---|
| Paddle | primary overseas MoR | implemented | signed Paddle webhook |
| Dodo Payments | overseas MoR backup | implemented | Standard Webhooks signature |
| ZPAY | mainland-China Alipay / WeChat | implemented | signed async callback + amount/PID checks |

The provider onboarding and real-money validation checklist is in [`docs/PROVIDER_ONBOARDING.md`](docs/PROVIDER_ONBOARDING.md).

## Choose your integration

If you are unsure where to start:

### I have a web independent site

Read:

1. [`docs/QUICKSTART.md`](docs/QUICKSTART.md)
2. [`docs/WEB_INTEGRATION.md`](docs/WEB_INTEGRATION.md)
3. [`examples/web-store/README.md`](examples/web-store/README.md)

Your website backend creates Checkout Sessions and receives signed `order.*` webhooks.

### I sell desktop software

Read:

1. [`docs/QUICKSTART.md`](docs/QUICKSTART.md)
2. [`docs/DESKTOP_SOFTWARE.md`](docs/DESKTOP_SOFTWARE.md)
3. [`docs/API.md`](docs/API.md)

Your desktop application does **not** contain Paddle/Dodo/ZPAY credentials. It talks to the entitlement/license API; payment happens through OpenIndieCommerce.

### I want to deploy this in production

Read:

1. [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md)
2. [`deploy/README.md`](deploy/README.md)
3. [`docs/PROVIDER_ONBOARDING.md`](docs/PROVIDER_ONBOARDING.md)
4. [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md)

## 10-minute local quick start

### Requirements

- Linux, macOS or WSL for development
- Rust stable
- `curl`
- `jq` is recommended for shell examples

Clone the repository:

```bash
git clone https://github.com/star8592/OpenIndieCommerce.git
cd OpenIndieCommerce
```

Configure a local instance:

```bash
export OIC_DB=/tmp/openindiecommerce.sqlite3
export OIC_ADMIN_TOKEN='development-admin-token-change-me'
export OIC_BIND=127.0.0.1:18791

export OIC_BRAND_NAME='Example Store'
export OIC_SELLER_LEGAL_NAME='Example Seller'
export OIC_SUPPORT_EMAIL='support@example.com'
export OIC_SUPPORT_PHONE='+1 555 0100'
```

Start it:

```bash
cargo run -p openindiecommerce-server
```

Check health from another terminal:

```bash
curl -s http://127.0.0.1:18791/health | jq
```

Expected:

```json
{
  "ok": true
}
```

Check provider readiness:

```bash
curl -s \
  -H 'Authorization: Bearer development-admin-token-change-me' \
  http://127.0.0.1:18791/v1/admin/providers | jq
```

A provider can remain unconfigured while you test the core Product/Price/Checkout APIs.

For the complete walkthrough, see [`docs/QUICKSTART.md`](docs/QUICKSTART.md).

## Create your first product and checkout

The admin API is protected with:

```http
Authorization: Bearer $OIC_ADMIN_TOKEN
```

Create a product:

```bash
curl -s -X POST http://127.0.0.1:18791/v1/admin/products \
  -H 'Authorization: Bearer development-admin-token-change-me' \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "2026 AI Market Report",
    "description": "PDF report + data appendix",
    "fulfillment": "download"
  }' | jq
```

Create a provider-backed Price. Money is stored in minor units: `1900` means USD 19.00; `9900` means CNY 99.00.

Paddle example:

```json
{
  "productId": "prod_...",
  "provider": "paddle",
  "currency": "USD",
  "unitAmount": 1900,
  "providerPriceId": "pri_..."
}
```

Dodo example:

```json
{
  "productId": "prod_...",
  "provider": "dodo",
  "currency": "USD",
  "unitAmount": 1900,
  "providerPriceId": "pdt_..."
}
```

ZPAY example:

```json
{
  "productId": "prod_...",
  "provider": "zpay",
  "currency": "CNY",
  "unitAmount": 9900
}
```

Then create a Checkout Session:

```bash
curl -s -X POST http://127.0.0.1:18791/v1/checkout/sessions \
  -H 'Content-Type: application/json' \
  -d '{
    "priceId": "price_...",
    "email": "buyer@example.com",
    "successUrl": "https://shop.example/order/success",
    "cancelUrl": "https://shop.example/cart",
    "metadata": {
      "userId": "user_123",
      "campaign": "launch"
    }
  }' | jq
```

The response contains a canonical `checkoutUrl` such as:

```text
/checkout/cs_...
```

Open:

```text
http://127.0.0.1:18791/checkout/cs_...
```

A configured provider is required to proceed to actual payment.

## Configure a payment provider

You only need the providers you intend to use.

### Paddle

```bash
export OIC_PADDLE_CLIENT_TOKEN='test_...'
export OIC_PADDLE_WEBHOOK_SECRET='...'
```

Each OpenIndieCommerce Paddle Price also stores the Paddle `providerPriceId` (`pri_...`). A `test_` client token makes the hosted Paddle page use Paddle Sandbox automatically.

### Dodo Payments

```bash
export OIC_DODO_ENV=test
export OIC_DODO_API_KEY='...'
export OIC_DODO_WEBHOOK_SECRET='whsec_...'
```

Each Dodo Price stores the Dodo product ID as `providerPriceId`.

Do not switch to `OIC_DODO_ENV=live` until test-mode acceptance passes.

### ZPAY

```bash
export OIC_PUBLIC_BASE_URL='https://commerce.example.com'
export OIC_ZPAY_PID='...'
export OIC_ZPAY_KEY='...'
```

Use a CNY Price. The hosted page offers the configured Alipay/WeChat routes.

For production, prefer `_FILE` secrets instead of plaintext environment variables. See [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md).

## Use it from a web independent site

Your browser should never call admin APIs and should never contain provider secrets.

Recommended pattern:

```text
browser
  -> your website backend
  -> POST OIC /v1/checkout/sessions
  -> redirect browser to OIC checkoutUrl

provider
  -> OIC verified provider webhook
  -> canonical order.paid
  -> signed OIC merchant webhook
  -> your website backend
  -> grant download / account / membership / data access
```

Important rule: **do not fulfill based on the customer's success page redirect.** Fulfill only after a valid signed `order.paid` webhook.

A complete zero-dependency Node example lives in [`examples/web-store`](examples/web-store).

Detailed guide: [`docs/WEB_INTEGRATION.md`](docs/WEB_INTEGRATION.md).

## Use it for desktop software licensing

Desktop software should keep payment and provider credentials off-device.

Recommended pattern:

```text
Desktop app
   | open Buy Pro
   v
OpenIndieCommerce checkout
   |
   | verified payment
   v
Entitlement / license
   |
   v
Desktop activate -> validate -> deactivate
```

The desktop application only needs public commerce/license endpoints and the user's license/claim information. Provider API keys and webhook secrets remain on the server.

BigFileViewer is the first real consumer of this architecture.

Detailed guide: [`docs/DESKTOP_SOFTWARE.md`](docs/DESKTOP_SOFTWARE.md).

## Production deployment

The supplied deployment model is intentionally small:

```text
Internet
   |
 Caddy (HTTPS)
   |
127.0.0.1:18791
   |
OpenIndieCommerce
   |
SQLite WAL
```

Recommended production paths:

```text
/opt/openindiecommerce/bin/openindiecommerce-server
/etc/openindiecommerce/openindiecommerce.env
/etc/openindiecommerce/secrets/*
/var/lib/openindiecommerce/openindiecommerce.sqlite3
/var/backups/openindiecommerce/
```

Production rules:

- bind the Rust service to localhost;
- expose only Caddy/HTTPS publicly;
- block `/v1/admin/*` from the public edge;
- keep secrets in `0600` files and use `*_FILE` environment variables;
- run `deploy/scripts/preflight.sh` before starting public sales;
- enable SQLite online backups;
- run backup verification and periodic restore drills;
- test a sandbox or low-value transaction before launch.

Detailed guide: [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md).

## Provider acceptance testing

OpenIndieCommerce ships a fail-closed acceptance harness.

Check all provider configuration:

```bash
export OIC_ACCEPTANCE_ADMIN_TOKEN='...'
python3 scripts/provider_acceptance.py \
  --base-url https://commerce.example.com \
  --provider all \
  --dry-run
```

Check one rail:

```bash
python3 scripts/provider_acceptance.py \
  --base-url https://commerce.example.com \
  --provider dodo \
  --dry-run
```

For a configured provider, running without `--dry-run` creates a disposable Product/Price/Checkout Session and prints the next checkout URL.

The code being implemented is not enough to call a provider production-ready. The release gate is:

```text
adapter tests
  -> provider sandbox/test payment
  -> verified canonical order
  -> merchant fulfillment
  -> refund/chargeback path
  -> low-value live transaction
  -> real payout/settlement
```

See [`docs/PROVIDER_ONBOARDING.md`](docs/PROVIDER_ONBOARDING.md).

## Security model

- payment provider API keys and webhook secrets stay server-side;
- provider callbacks are verified before creating or changing a canonical order;
- provider order IDs and canonical processing are idempotent;
- merchant webhooks are HMAC-SHA256 signed;
- admin APIs use bearer authentication;
- production Caddy configuration blocks admin endpoints from the public internet;
- high-risk public endpoints have rate limiting;
- checkout sessions expire;
- SQLite runs in WAL mode;
- production backups use SQLite's online backup API;
- license keys are optional, and normal validation does not depend on storing plaintext keys;
- OpenIndieCommerce does not store card data.

## Documentation map

| Document | Use it for |
|---|---|
| [`README.zh-CN.md`](README.zh-CN.md) | Chinese overview and usage guide |
| [`docs/QUICKSTART.md`](docs/QUICKSTART.md) | first local run from zero |
| [`docs/WEB_INTEGRATION.md`](docs/WEB_INTEGRATION.md) | independent website / digital product integration |
| [`docs/DESKTOP_SOFTWARE.md`](docs/DESKTOP_SOFTWARE.md) | desktop paid software and license architecture |
| [`docs/API.md`](docs/API.md) | API examples and normalized commerce flow |
| [`docs/openapi.json`](docs/openapi.json) | OpenAPI 3.1 machine-readable contract |
| [`docs/PROVIDER_ONBOARDING.md`](docs/PROVIDER_ONBOARDING.md) | Paddle / Dodo / ZPAY onboarding and real-money gates |
| [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md) | production topology and installation procedure |
| [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md) | common failures and diagnostics |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | architecture notes |
| [`docs/API_VERSIONING.md`](docs/API_VERSIONING.md) | v1 compatibility policy |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | roadmap |

## Development checks

```bash
cargo fmt --all -- --check
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 -m unittest discover -s scripts/tests -v
node --check examples/web-store/server.mjs
git diff --check
```

## Current limitations

v0.1 intentionally does not try to be an enterprise billing platform.

Current non-goals / deferred work:

- custody of funds;
- becoming a Merchant of Record;
- storing card data;
- dozens of PSP adapters;
- multi-tenant hosted control plane;
- complex metered billing analytics;
- automatic tax/legal advice;
- claiming payout reliability before real payout evidence exists.

For metered billing, OpenIndieCommerce should interoperate with a billing engine rather than rebuild one.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
