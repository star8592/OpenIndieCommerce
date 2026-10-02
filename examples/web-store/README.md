# Minimal web-store integration

This Node 22 example has zero npm dependencies. The store backend creates Checkout Sessions and verifies OpenIndieCommerce merchant webhooks.

Required environment:

```bash
export OIC_BASE='https://commerce.example.com'
export OIC_PRICE_ID='price_...'
export OIC_WEBHOOK_SECRET='whsec_...'
node server.mjs
```

Register the example receiver once:

```text
POST /v1/admin/webhooks
url = https://your-store.example/webhooks/openindie
```

The browser talks only to the store backend. The backend creates the checkout session and redirects to the hosted checkout URL. This keeps trusted metadata and redirect URLs under server control.
