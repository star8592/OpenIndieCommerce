# Web Independent-Site Integration

This guide describes the recommended way to use OpenIndieCommerce from a normal independent website, digital-goods store, SaaS backend or member site.

The important architectural rule is:

> **Your browser talks to your own backend. Your backend talks to OpenIndieCommerce. Provider secrets stay on servers. Fulfillment happens only after a signed `order.paid` webhook.**

## 1. Recommended topology

```text
Customer browser
      |
      v
Your website / API
      |
      | POST /v1/checkout/sessions
      v
OpenIndieCommerce
      |
      v
Paddle / Dodo / ZPAY
      |
      | verified provider webhook
      v
OpenIndieCommerce
      |
      | signed merchant webhook
      v
Your website backend
      |
      v
Fulfillment
```

Fulfillment might mean:

- create a download entitlement;
- enable a SaaS plan;
- unlock a report;
- issue a software license;
- create a membership;
- increase API quota;
- grant access to a private dataset.

## 2. What belongs where

### Browser

May contain:

- your public product page;
- product ID or your own SKU;
- customer UI state;
- redirect to a returned Checkout URL.

Must **not** contain:

- `OIC_ADMIN_TOKEN`;
- Paddle/Dodo/ZPAY server secrets;
- merchant webhook secret;
- entitlement key secret.

### Your website backend

Owns:

- your user/customer records;
- your product-to-OIC Price mapping;
- creating Checkout Sessions;
- receiving OpenIndieCommerce merchant webhooks;
- final fulfillment state;
- idempotency for your own delivery logic.

### OpenIndieCommerce

Owns:

- provider-independent Checkout Session;
- provider callback verification;
- canonical order state;
- normalized `order.*` events;
- webhook delivery/retry;
- optional entitlement/license infrastructure.

## 3. Create checkout from your backend

Assume your application already knows its OpenIndieCommerce `priceId`.

Your backend calls:

```http
POST /v1/checkout/sessions
Content-Type: application/json
```

Example payload:

```json
{
  "priceId": "price_123",
  "email": "buyer@example.com",
  "successUrl": "https://shop.example/order/success",
  "cancelUrl": "https://shop.example/cart",
  "metadata": {
    "userId": "usr_123",
    "internalOrderId": "web_456",
    "campaign": "launch-2026"
  }
}
```

The metadata is intentionally application-defined. Use it to correlate the later canonical order back to your own user/order.

Do not place secrets in metadata.

The response includes:

```json
{
  "checkoutUrl": "/checkout/cs_..."
}
```

Redirect the browser to your OpenIndieCommerce public base URL plus that path.

Example:

```text
https://commerce.example.com/checkout/cs_...
```

## 4. Do not trust the success redirect

A user reaching:

```text
https://shop.example/order/success
```

does **not** prove the payment succeeded.

Why:

- browser redirects can be replayed;
- a user can manually open the URL;
- network timing can make redirect and webhook order differ;
- some payment methods are asynchronous.

Your success page should display something like:

```text
Payment received or processing.
We are confirming the order.
```

Then query your own backend for entitlement/order status.

Your backend should only set that status after receiving a valid signed merchant webhook from OpenIndieCommerce.

## 5. Register a merchant webhook

Admin request:

```bash
curl -s -X POST https://commerce.example.com/v1/admin/webhooks \
  -H "Authorization: Bearer $OIC_ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{
    "url":"https://shop.example/webhooks/openindie"
  }'
```

The response returns a secret such as:

```text
whsec_...
```

Save it immediately in your application server's secret store.

The normal listing API does not return secrets later.

## 6. Verify merchant webhook signatures

OpenIndieCommerce sends headers including:

```http
OpenIndie-Event-Id: evt_...
OpenIndie-Event-Type: order.paid
OpenIndie-Signature: t=...;v1=...
Content-Type: application/json
```

The signature is HMAC-SHA256 over:

```text
timestamp + "." + rawBody
```

Important:

- verify against the **raw request body**;
- reject stale timestamps;
- use constant-time signature comparison;
- do not parse/re-serialize JSON before signature verification.

The example implementation in [`../examples/web-store/server.mjs`](../examples/web-store/server.mjs) shows the expected pattern.

## 7. Handle canonical events

### `order.paid`

Typical action:

1. verify signature;
2. check whether `event.id` was already processed;
3. find your local user/order from `checkoutMetadata`;
4. store the canonical OIC order ID;
5. grant entitlement;
6. commit both event-id and entitlement atomically if possible;
7. return HTTP 2xx.

### `order.refunded`

Typical action:

- mark the local purchase refunded;
- revoke the associated entitlement/download/license if your product policy requires it.

### `order.chargeback`

Typical action:

- revoke entitlement;
- record the dispute/chargeback state;
- avoid silently re-granting access on webhook retry.

## 8. Your webhook handler must be idempotent

OpenIndieCommerce webhook delivery is designed as at-least-once delivery.

Your application should persist processed event IDs.

Pseudo-code:

```text
BEGIN TRANSACTION

if webhook_events contains event.id:
    return 200

apply business effect
insert webhook_events(event.id)

COMMIT
return 200
```

Never assume a webhook is delivered exactly once.

## 9. Recommended local data model

A small independent site can start with:

```text
users
products
purchases
entitlements
processed_webhook_events
```

Example purchase fields:

```text
id
user_id
oic_order_id
oic_price_id
provider
provider_order_id
currency
amount
status
created_at
```

Example entitlement fields:

```text
id
user_id
product_id
purchase_id
status
granted_at
revoked_at
```

## 10. Digital download example

Do not return a raw permanent file URL directly from the webhook.

Recommended pattern:

```text
order.paid
  -> local entitlement
  -> authenticated download endpoint
  -> short-lived signed URL or streamed response
```

When `order.refunded` arrives:

```text
entitlement.status = revoked
```

Future download requests then fail authorization.

## 11. SaaS example

Metadata when creating Checkout Session:

```json
{
  "userId": "usr_123",
  "plan": "pro"
}
```

On `order.paid`:

```text
users.plan = pro
users.plan_source_order = ord_...
```

On refund/chargeback:

```text
users.plan = free
```

For recurring/complex billing, keep the entitlement model independent from provider-specific subscription objects.

## 12. Webhook operations

List endpoints:

```http
GET /v1/admin/webhooks
```

Disable one:

```http
PATCH /v1/admin/webhooks/{id}

{"active":false}
```

Recent deliveries:

```http
GET /v1/admin/webhook-deliveries?limit=100
```

Replay one existing delivery:

```http
POST /v1/admin/webhook-deliveries/{id}/redeliver
```

Redelivery does not create a new commerce event; it reuses the existing delivery/event.

## 13. Checkout expiry

Checkout Sessions expire after 30 minutes by default.

You can set:

```json
{
  "expiresInSeconds": 1800
}
```

Allowed v0.1 range: 60 to 86400 seconds.

An expired session cannot become a new paid order. Retries of an already-recorded provider order remain idempotent.

## 14. Reference implementation

Run the example:

```bash
cd examples/web-store

export OIC_BASE='https://commerce.example.com'
export OIC_PRICE_ID='price_...'
export OIC_WEBHOOK_SECRET='whsec_...'

node server.mjs
```

The example demonstrates:

- backend Checkout Session creation;
- signed merchant webhook verification;
- local entitlement creation;
- protected download;
- webhook idempotency;
- refund/chargeback revocation.

## 15. Production checklist for a web store

- [ ] OIC runs behind HTTPS
- [ ] admin API is not exposed publicly
- [ ] website backend, not browser, owns private integration state
- [ ] merchant webhook secret stored server-side
- [ ] webhook verifies raw body signature and timestamp
- [ ] processed event IDs persisted
- [ ] fulfillment is idempotent
- [ ] success page does not grant access itself
- [ ] refund/chargeback revocation tested
- [ ] provider sandbox/test payment completed
- [ ] real low-value transaction completed before public launch
- [ ] first real payout/settlement confirmed before calling the rail production-verified
