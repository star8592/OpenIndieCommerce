# Quick Start

This guide takes a fresh checkout of OpenIndieCommerce to a working local commerce server, then creates a Product, Price and Checkout Session.

It deliberately separates **core API smoke testing** from **real provider payment testing**. You can run the core without any payment-provider credentials.

## 1. Prerequisites

Required:

- Rust stable
- Git
- curl

Recommended:

- jq
- Node 22 if you want to run the web-store example

Check:

```bash
rustc --version
cargo --version
git --version
curl --version
jq --version
```

## 2. Clone and test

```bash
git clone https://github.com/star8592/OpenIndieCommerce.git
cd OpenIndieCommerce

cargo test --workspace --all-features
```

Optional full quality gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 -m unittest discover -s scripts/tests -v
node --check examples/web-store/server.mjs
git diff --check
```

## 3. Configure a local instance

Use a disposable local database:

```bash
export OIC_DB=/tmp/openindiecommerce.sqlite3
export OIC_BIND=127.0.0.1:18791
export OIC_ADMIN_TOKEN='development-admin-token-change-me'
```

The built-in merchant/legal pages also require public identity information:

```bash
export OIC_BRAND_NAME='Example Store'
export OIC_SELLER_LEGAL_NAME='Example Seller'
export OIC_SUPPORT_EMAIL='support@example.com'
export OIC_SUPPORT_PHONE='+1 555 0100'
export OIC_REFUND_DAYS=14
```

Start the server:

```bash
cargo run -p openindiecommerce-server
```

Keep this terminal running.

## 4. Verify health

In a second terminal:

```bash
curl -i http://127.0.0.1:18791/health
```

Expected HTTP status: `200`.

JSON:

```json
{"ok":true}
```

Open the built-in public pages:

```text
http://127.0.0.1:18791/
http://127.0.0.1:18791/terms
http://127.0.0.1:18791/privacy
http://127.0.0.1:18791/refund
```

## 5. Check provider readiness

```bash
curl -s \
  -H 'Authorization: Bearer development-admin-token-change-me' \
  http://127.0.0.1:18791/v1/admin/providers | jq
```

At a fresh install, providers should normally report `configured: false`.

That does **not** mean the core server is broken. It only means real payment checkout cannot proceed yet.

## 6. Create a Product

```bash
PRODUCT_JSON=$(curl -s -X POST \
  http://127.0.0.1:18791/v1/admin/products \
  -H 'Authorization: Bearer development-admin-token-change-me' \
  -H 'Content-Type: application/json' \
  -d '{
    "name":"Demo Report",
    "description":"A downloadable demo product",
    "fulfillment":"download"
  }')

echo "$PRODUCT_JSON" | jq
export PRODUCT_ID=$(echo "$PRODUCT_JSON" | jq -r '.id')
echo "$PRODUCT_ID"
```

## 7. Create a Price

A Price selects the payment provider.

For a core API smoke test you can create the record even before provider credentials exist.

### Paddle-shaped Price

Replace `pri_demo` with a real Paddle Price ID before attempting payment:

```bash
PRICE_JSON=$(curl -s -X POST \
  http://127.0.0.1:18791/v1/admin/prices \
  -H 'Authorization: Bearer development-admin-token-change-me' \
  -H 'Content-Type: application/json' \
  -d "{
    \"productId\":\"$PRODUCT_ID\",
    \"provider\":\"paddle\",
    \"currency\":\"USD\",
    \"unitAmount\":1900,
    \"providerPriceId\":\"pri_demo\"
  }")

echo "$PRICE_JSON" | jq
export PRICE_ID=$(echo "$PRICE_JSON" | jq -r '.id')
```

Money uses minor units:

```text
1900 USD = $19.00
9900 CNY = ¥99.00
```

## 8. Create a Checkout Session

```bash
SESSION_JSON=$(curl -s -X POST \
  http://127.0.0.1:18791/v1/checkout/sessions \
  -H 'Content-Type: application/json' \
  -d "{
    \"priceId\":\"$PRICE_ID\",
    \"email\":\"buyer@example.com\",
    \"metadata\":{
      \"userId\":\"demo-user-1\",
      \"source\":\"quickstart\"
    }
  }")

echo "$SESSION_JSON" | jq
export CHECKOUT_PATH=$(echo "$SESSION_JSON" | jq -r '.checkoutUrl')
echo "http://127.0.0.1:18791$CHECKOUT_PATH"
```

This proves the canonical Product -> Price -> Checkout Session path works.

If you open the hosted checkout before configuring Paddle, the server will correctly refuse to continue with a provider-not-configured error.

## 9. Configure one real/test provider

Choose **one** provider first. Do not configure all three just to get started.

### Option A: Dodo test mode

```bash
export OIC_DODO_ENV=test
export OIC_DODO_API_KEY='...'
export OIC_DODO_WEBHOOK_SECRET='whsec_...'
export OIC_DODO_PRODUCT_ID='pdt_...'
```

Then use the acceptance harness:

```bash
export OIC_ACCEPTANCE_ADMIN_TOKEN="$OIC_ADMIN_TOKEN"
python3 scripts/provider_acceptance.py \
  --base-url http://127.0.0.1:18791 \
  --provider dodo \
  --dry-run
```

For a real remote provider callback, your OpenIndieCommerce instance must have a public HTTPS URL. Localhost alone is not reachable from provider webhooks.

### Option B: Paddle Sandbox

```bash
export OIC_PADDLE_CLIENT_TOKEN='test_...'
export OIC_PADDLE_WEBHOOK_SECRET='...'
export OIC_PADDLE_PRICE_ID='pri_...'
```

A `test_` client token makes the hosted page initialize Paddle Sandbox.

### Option C: ZPAY

ZPAY is a direct domestic rail, so use a public HTTPS instance rather than treating it as a fake local sandbox:

```bash
export OIC_PUBLIC_BASE_URL='https://commerce.example.com'
export OIC_ZPAY_PID='...'
export OIC_ZPAY_KEY='...'
```

Use a CNY Price.

## 10. Register your application webhook

Your application should receive canonical events from OpenIndieCommerce, not provider-specific callbacks.

```bash
WEBHOOK_JSON=$(curl -s -X POST \
  http://127.0.0.1:18791/v1/admin/webhooks \
  -H 'Authorization: Bearer development-admin-token-change-me' \
  -H 'Content-Type: application/json' \
  -d '{
    "url":"https://your-app.example/webhooks/openindie"
  }')

echo "$WEBHOOK_JSON" | jq
```

Save the returned `whsec_...` securely. It is used by your application to verify OpenIndieCommerce merchant webhooks.

## 11. What to do next

For a web independent site:

[`WEB_INTEGRATION.md`](WEB_INTEGRATION.md)

For paid desktop software:

[`DESKTOP_SOFTWARE.md`](DESKTOP_SOFTWARE.md)

For payment account setup:

[`PROVIDER_ONBOARDING.md`](PROVIDER_ONBOARDING.md)

For production deployment:

[`DEPLOYMENT.md`](DEPLOYMENT.md)

For exact API details:

[`API.md`](API.md) and [`openapi.json`](openapi.json)
