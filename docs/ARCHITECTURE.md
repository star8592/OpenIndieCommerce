# Architecture

OpenIndieCommerce is a self-hosted commerce orchestration layer. Its most important architectural goal is to keep **product business logic independent from payment-provider business logic**.

## 1. System boundary

```text
                Your products
   +----------------------------------+
   | Web store / SaaS / Desktop app   |
   +----------------------------------+
                    |
                    | provider-neutral API
                    v
        +---------------------------+
        |    OpenIndieCommerce      |
        |---------------------------|
        | Product / Price           |
        | Checkout Session          |
        | Canonical Order           |
        | Canonical Events          |
        | Merchant Webhook Outbox   |
        | Optional Entitlement      |
        | Optional License/Devices  |
        +---------------------------+
             |          |          |
             v          v          v
          Paddle      Dodo       ZPAY
           MoR         MoR      China direct
```

OpenIndieCommerce owns commerce state and normalization. Payment providers own payment execution, payment credentials and their own settlement/payout systems.

## 2. Core domain model

### Product

Represents what the merchant sells.

Examples:

```text
PDF report
SaaS Pro
Desktop Pro license
Dataset
Course
API entitlement
```

The Product is provider-neutral.

### Price

Connects a Product to one provider/currency/amount.

A Product may therefore have multiple Prices:

```text
Product: BigFileViewer Pro

Price A:
  provider = paddle
  currency = USD

Price B:
  provider = dodo
  currency = USD

Price C:
  provider = zpay
  currency = CNY
```

This is the basic mechanism that allows one product to use different payment rails without changing the product model.

### Checkout Session

Represents one buyer intent to purchase one Price.

It stores:

- Price reference;
- buyer email;
- success/cancel URL;
- application-defined metadata;
- expiry;
- canonical status.

It is created before provider checkout begins.

### Canonical Order

A provider-specific successful payment becomes one provider-neutral canonical Order.

The application should use the canonical Order rather than Paddle/Dodo/ZPAY transaction structures.

### Canonical Event

Current important events:

```text
order.paid
order.refunded
order.chargeback
```

These events are delivered to merchant applications through the merchant webhook outbox.

## 3. Checkout data flow

```text
Your backend
  |
  | POST /v1/checkout/sessions
  v
Checkout Session
  |
  | GET /checkout/{id}
  v
Provider-specific checkout adapter
  |
  v
Paddle / Dodo / ZPAY
```

The browser is used to complete payment but is not trusted to assert payment result.

## 4. Payment confirmation data flow

```text
Provider
   |
   | signed callback/webhook
   v
Provider adapter
   |
   | verify signature
   | verify amount/currency where applicable
   | map checkout/order identity
   | enforce idempotency
   v
Canonical Order
   |
   v
Canonical order.paid
   |
   v
Merchant Webhook Outbox
   |
   | signed event
   v
Your application
   |
   v
Fulfillment
```

A browser success redirect is deliberately outside the trusted payment-confirmation chain.

## 5. Provider adapters

Provider adapters should be thin translators.

They are responsible for:

- creating/initializing provider checkout;
- verifying provider callbacks;
- extracting provider transaction/order identity;
- checking required payment fields;
- translating provider lifecycle events into canonical events.

They should not own product-specific fulfillment logic.

### Paddle

Typical mapping:

```text
transaction.completed -> order.paid
refund lifecycle      -> order.refunded
reversal/dispute      -> canonical reversal semantics where implemented
```

Paddle can act as Merchant of Record.

### Dodo Payments

Current mapping includes:

```text
payment.succeeded -> order.paid
full refund.succeeded -> order.refunded
dispute.lost -> order.chargeback
```

A partial refund does not automatically revoke the whole entitlement.

### ZPAY

ZPAY is treated as a mainland-China direct payment rail.

The adapter verifies:

- signature;
- merchant PID;
- payment state;
- exact amount;
- duplicate callback idempotency.

ZPAY is not treated as a Merchant of Record.

## 6. Merchant webhook outbox

OpenIndieCommerce does not require your application to understand provider webhooks.

Instead it emits a signed merchant event:

```text
OpenIndieCommerce
   |
   | order.paid
   | HMAC signed
   v
Your backend
```

The outbox supports:

- retry;
- endpoint enable/disable;
- recent delivery inspection;
- manual redelivery;
- at-least-once delivery.

Your application must therefore process events idempotently.

## 7. Entitlement architecture

Payment and entitlement are intentionally separate concepts.

```text
payment proves purchase
entitlement defines access
```

Examples:

```text
Order paid -> downloadable report entitlement
Order paid -> SaaS Pro entitlement
Order paid -> desktop license entitlement
```

A refund/chargeback may revoke an entitlement according to the product policy.

This makes provider migration easier because access is not defined as “a Paddle transaction exists”.

## 8. Desktop license architecture

```text
Desktop client
     |
     | public license/entitlement API
     v
OpenIndieCommerce
     |
     +-- license state
     +-- activation instances
     +-- activation limit
     +-- revocation
```

The client never receives provider secrets.

License/device authorization is a server concern; feature gating is a product concern.

## 9. Trust boundaries

### Trusted server-side secrets

Examples:

```text
OIC_ADMIN_TOKEN
Paddle webhook secret
Dodo API key
Dodo webhook secret
ZPAY key
merchant webhook secrets
entitlement recovery secret
```

These must not be sent to browsers or desktop binaries.

### Public data

Examples:

```text
checkout URL
public merchant pages
customer-facing product details
public server base URL
```

### Untrusted inputs

Treat these as untrusted:

```text
browser redirect
checkout metadata
provider callback body until verified
client IP forwarding headers unless behind trusted proxy
desktop client claims
merchant webhook receiver responses
```

## 10. Admin plane vs public plane

The intended production topology separates the two.

Public routes include:

- checkout;
- provider callbacks;
- public legal/support pages;
- health according to operator policy.

Admin routes include:

```text
/v1/admin/*
```

The production Caddy configuration should prevent arbitrary internet clients from accessing admin routes.

Admin actions should use localhost, SSH tunnel, VPN or another authenticated management path.

## 11. Persistence

v0.1 intentionally uses SQLite.

Reasons:

- one binary;
- low operational overhead;
- strong transactional behavior;
- sufficient for a single-node indie commerce server;
- easy backup/restore story.

Production settings include WAL and a busy timeout.

Online backup tooling is used instead of copying only the main database file while WAL is active.

PostgreSQL is deferred until multi-node/multi-tenant requirements justify the added operational complexity.

## 12. Rate limiting

v0.1 uses in-process limits for selected public high-risk operations.

It assumes the Rust service is behind the trusted Caddy reverse proxy.

This is a single-node protection layer, not a global distributed rate limiter.

A future multi-instance deployment would require an external/shared rate-limit state or an edge-layer mechanism.

## 13. Why the architecture is provider-neutral

Without this layer:

```text
Product A -> Paddle logic
Product B -> Paddle logic
Product C -> ZPAY logic
```

Every product duplicates commerce code.

With OpenIndieCommerce:

```text
Product A -+
Product B -+-> OpenIndieCommerce -> provider adapters
Product C -+
```

Provider changes happen centrally.

This is especially valuable for a software factory launching many small products.

## 14. v0.1 deliberate constraints

The architecture intentionally does not attempt to solve everything.

Deferred/non-goals:

- fund custody;
- card storage;
- becoming an MoR;
- enterprise routing across dozens of processors;
- multi-region active/active infrastructure;
- full metered-billing engine;
- multi-tenant SaaS control plane.

The design goal is a small, auditable foundation that independent developers can operate and understand.
