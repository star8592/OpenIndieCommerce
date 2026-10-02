# Independent web-store example

This Node 22 example has zero npm dependencies and demonstrates the complete independent-site flow.

For the architectural explanation first read:

**[`../../docs/WEB_INTEGRATION.md`](../../docs/WEB_INTEGRATION.md)**

## What the example proves

1. your backend creates an OpenIndieCommerce Checkout Session;
2. the browser is redirected to the hosted provider checkout;
3. the success page does **not** trust the browser redirect as proof of payment;
4. a signed `order.paid` merchant webhook creates the local entitlement;
5. the customer receives access to a protected digital download;
6. repeated webhook events are idempotent;
7. `order.refunded` / `order.chargeback` remove the demo entitlement.

## Required environment

```bash
export OIC_BASE='https://commerce.example.com'
export OIC_PRICE_ID='price_...'
export OIC_WEBHOOK_SECRET='whsec_...'
```

Then:

```bash
node server.mjs
```

## Before running it

Create an OpenIndieCommerce Product and Price first. See:

- [`../../docs/QUICKSTART.md`](../../docs/QUICKSTART.md)
- [`../../docs/API.md`](../../docs/API.md)

Register this example's receiver once using the OpenIndieCommerce admin API:

```http
POST /v1/admin/webhooks
```

with a URL such as:

```text
https://your-store.example/webhooks/openindie
```

Save the returned `whsec_...` as `OIC_WEBHOOK_SECRET` on the web-store server.

## Security properties demonstrated

The example:

- verifies the OpenIndieCommerce merchant webhook over the raw body;
- enforces a five-minute timestamp tolerance;
- uses constant-time HMAC comparison;
- treats delivery as at-least-once;
- does not fulfill based on browser redirects;
- revokes local demo entitlement for refund/chargeback events.

Keep `OIC_WEBHOOK_SECRET` server-side. Never expose it to browser JavaScript.

## Persistence warning

For clarity this demo keeps entitlements in memory.

A real site must persist at least:

```text
event.id
order.id
its own user/customer ID
purchase status
entitlement state
```

Prefer applying `event.id` persistence and the fulfillment business effect in one database transaction so duplicate webhook delivery cannot duplicate fulfillment.

## Production adaptation

Replace the in-memory entitlement with your real business model:

- download access;
- SaaS plan;
- membership;
- API quota;
- report/data access;
- desktop license claim.

Then test the complete path:

```text
payment
-> provider webhook
-> OIC order.paid
-> signed merchant webhook
-> your persisted entitlement
-> refund/chargeback
-> entitlement revocation
```
