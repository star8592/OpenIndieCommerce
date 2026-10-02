# Independent web-store integration

This Node 22 example has zero npm dependencies and demonstrates the complete independent-site flow:

1. your backend creates an OpenIndieCommerce Checkout Session;
2. the browser is redirected to the hosted provider checkout;
3. the success page does **not** trust the browser redirect as proof of payment;
4. a signed `order.paid` merchant webhook creates the local entitlement;
5. the customer receives access to a protected digital download;
6. repeated webhook events are idempotent;
7. `order.refunded` / `order.chargeback` remove the demo entitlement.

Required environment:

```bash
export OIC_BASE='https://commerce.example.com'
export OIC_PRICE_ID='price_...'
export OIC_WEBHOOK_SECRET='whsec_...'
node server.mjs
```

Register the receiver once with `POST /v1/admin/webhooks` and URL `https://your-store.example/webhooks/openindie`.

For clarity this demo keeps entitlements in memory. A real site should persist `event.id`, `order.id`, its own user/customer ID and entitlement state in its database. Fulfillment must remain idempotent because webhook delivery is at-least-once.

The merchant webhook verifier enforces a five-minute timestamp tolerance and constant-time HMAC comparison. Keep `OIC_WEBHOOK_SECRET` server-side; never expose it to browser JavaScript.
