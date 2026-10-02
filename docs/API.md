# OpenIndieCommerce API v0.1

OpenIndieCommerce gives a storefront one API for products, prices, checkout sessions and normalized order events.

If this is your first time using the project, start with [`QUICKSTART.md`](QUICKSTART.md). For an independent website, continue with [`WEB_INTEGRATION.md`](WEB_INTEGRATION.md).

## Core flow

```text
admin creates Product + Price
        ↓
website creates Checkout Session
        ↓
customer opens /checkout/{sessionId}
        ↓
Paddle / Dodo / ZPAY completes payment
        ↓
OpenIndieCommerce verifies provider webhook
        ↓
Canonical Order + order.paid
        ↓
signed merchant webhook → your website
```

Money is represented in minor units: USD 19.00 = `1900`, CNY 99.00 = `9900`.

## Admin authentication

Admin endpoints use:

```http
Authorization: Bearer $OIC_ADMIN_TOKEN
```

The admin API should not be exposed publicly without an authenticated edge. The supplied Caddy example blocks `/v1/admin/*`.

## Create a product

`POST /v1/admin/products`

```json
{
  "name": "2026 China AI Market Report",
  "description": "PDF + data appendix",
  "fulfillment": "download"
}
```

Supported v0.1 fulfillment hints: `none`, `download`, `entitlement`, `license`.

## Create a price

`POST /v1/admin/prices`

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

ZPAY example:

```json
{
  "productId": "prod_...",
  "provider": "zpay",
  "currency": "CNY",
  "unitAmount": 9900
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

For Dodo, `providerPriceId` is the Dodo product ID. OpenIndieCommerce creates a Dodo Checkout Session server-side and writes the canonical OIC checkout-session ID into Dodo metadata. Payment is fulfilled only from a verified `payment.succeeded` webhook. Full `refund.succeeded` maps to `order.refunded`; partial refunds do not revoke the whole entitlement. `dispute.lost` maps to `order.chargeback`.

## Create a checkout session

`POST /v1/checkout/sessions`

```json
{
  "priceId": "price_...",
  "email": "buyer@example.com",
  "successUrl": "https://shop.example/order/success",
  "cancelUrl": "https://shop.example/cart",
  "metadata": {
    "userId": "user_123",
    "campaign": "x-launch"
  },
  "expiresInSeconds": 1800
}
```

The response includes a canonical session and `checkoutUrl` such as `/checkout/cs_...`.

The metadata is included in the normalized `order.paid` merchant webhook so the storefront can map a purchase back to its own user or campaign.

## Register a merchant webhook

`POST /v1/admin/webhooks`

```json
{
  "url": "https://shop.example/api/openindie/webhook"
}
```

The response returns a `whsec_...` secret once. Store it server-side.

Successful deliveries include:

```http
OpenIndie-Event-Id: evt_...
OpenIndie-Event-Type: order.paid
OpenIndie-Signature: t=...;v1=...
Content-Type: application/json
```

The signature is HMAC-SHA256 over `timestamp + "." + rawBody`. Verify the timestamp and raw body before fulfilling an order.

Example normalized event:

```json
{
  "id": "evt_paid_ord_...",
  "type": "order.paid",
  "createdAt": 1790937872,
  "data": {
    "order": {
      "id": "ord_...",
      "provider": "zpay",
      "providerOrderId": "zp_...",
      "priceId": "price_...",
      "email": "buyer@example.com",
      "currency": "CNY",
      "amount": 9900,
      "status": "paid"
    },
    "checkoutMetadata": {
      "userId": "user_123"
    }
  }
}
```

## Checkout expiry

Checkout Sessions expire after 30 minutes by default. `expiresInSeconds` may be set from 60 to 86400 seconds. Expired sessions return status `expired` and hosted checkout returns HTTP 410. A provider webhook cannot create a new paid order from an expired session, while retries for an already-recorded provider order remain idempotent.

## Manage merchant webhooks

List endpoints: `GET /v1/admin/webhooks`. Secrets are never returned by this endpoint.

Enable or disable an endpoint: `PATCH /v1/admin/webhooks/{id}` with `{ "active": false }`. Disabled endpoints stop receiving queued or future deliveries until re-enabled.

Inspect recent deliveries: `GET /v1/admin/webhook-deliveries?limit=100`.

Replay one delivery: `POST /v1/admin/webhook-deliveries/{id}/redeliver`. Redelivery resets the existing delivery to `pending`; it does not create a duplicate commerce event.

## OpenAPI

`GET /openapi.json` returns the embedded OpenAPI 3.1 contract for the public/admin v1 surface. See [`API_VERSIONING.md`](API_VERSIONING.md) for compatibility rules.

## Rate limiting

The single-node v0.1 server applies an in-process fixed-window limit by trusted reverse-proxy client IP to checkout creation (60/min), license claim (30/min) and license activation (30/min). Production deployment keeps the service on localhost behind Caddy, which supplies the client forwarding headers. Provider webhooks use provider signature verification and are not subject to this public-client limiter.

## Provider readiness

`GET /v1/admin/providers` is an admin-only, non-secret readiness endpoint. It reports whether Paddle, Dodo and ZPAY have the runtime configuration required to operate. Provider API keys and webhook secrets are never returned.

Use the unified acceptance harness for operational checks.

All providers:

```bash
export OIC_ACCEPTANCE_ADMIN_TOKEN='...'
python3 scripts/provider_acceptance.py \
  --base-url https://commerce.example.com \
  --provider all \
  --dry-run
```

One provider:

```bash
python3 scripts/provider_acceptance.py \
  --base-url https://commerce.example.com \
  --provider dodo \
  --dry-run
```

The accepted provider names are:

```text
paddle
dodo
zpay
```

For a configured provider, running the harness without `--dry-run` creates a disposable OpenIndieCommerce Product/Price/Checkout Session. Paddle/ZPAY return the OIC hosted browser checkout path; Dodo resolves through OIC to the provider checkout URL.

For Paddle and Dodo, set the corresponding provider catalog ID (`OIC_PADDLE_PRICE_ID` or `OIC_DODO_PRODUCT_ID`) or pass `--provider-product-id`.

The acceptance harness is a configuration/checkout test. It does not by itself prove production readiness. See [`PROVIDER_ONBOARDING.md`](PROVIDER_ONBOARDING.md) for the required sandbox/test, live transaction and payout/settlement gates.
