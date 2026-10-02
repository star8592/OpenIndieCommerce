# Desktop Software Integration

This guide explains how to use OpenIndieCommerce for paid desktop software without embedding payment-provider secrets in the application.

BigFileViewer is the first real consumer of this architecture.

## 1. Design goal

A desktop application should know how to:

- open a purchase page;
- show Free / Trial / Pro state;
- activate a purchased entitlement/license;
- validate cached authorization;
- deactivate the current device;
- survive temporary offline periods according to the product's policy.

It should **not** know:

- Paddle server credentials;
- Dodo API keys;
- ZPAY merchant key;
- provider webhook secrets;
- payout credentials.

## 2. Recommended architecture

```text
Desktop application
      |
      | Buy Pro
      v
OpenIndieCommerce Checkout
      |
      v
Payment Provider
      |
      | signed provider webhook
      v
OpenIndieCommerce
      |
      | entitlement / claim / license
      v
Desktop activation
```

Provider replacement then happens server-side.

Your Windows/macOS/Linux release does not need a new payment integration simply because you move from one provider to another.

## 3. Keep commerce and product entitlement separate

A useful product model is:

```text
Free
Trial
Pro
```

The application decides what features each tier enables.

OpenIndieCommerce provides evidence that the buyer owns an entitlement/license; your application enforces its own feature policy.

Example:

```text
Free:
  small files
  basic preview
  literal search

Trial:
  full features for 14 days

Pro:
  full features
  large files
  advanced search
  tail/follow
```

Do not hard-code provider-specific product logic into feature checks.

Bad:

```text
if paddle_transaction_exists:
    unlock_pro()
```

Better:

```text
if entitlement_tier == "pro":
    unlock_pro()
```

## 4. Purchase flow

The application can expose a `Buy Pro` action that opens a public OpenIndieCommerce checkout URL in the user's browser.

Recommended UX:

```text
Buy Pro
  -> default browser
  -> HTTPS commerce page
  -> payment provider
  -> purchase confirmation
  -> return to app
  -> user claims/activates license
```

The application must not consider the browser return redirect proof of payment.

Only the server-side verified payment can create the entitlement/license.

## 5. License delivery options

There are two broad models.

### User enters a license key

Good for:

- desktop utilities;
- offline-friendly software;
- users without an account system.

Flow:

```text
payment
-> license generated/claimable
-> user receives key
-> user enters key in app
-> app activates current device
```

### Account-based entitlement

Good for:

- products that already have login/accounts;
- SaaS + desktop combinations.

Flow:

```text
payment metadata includes internal user ID
-> order.paid
-> entitlement attached to account
-> desktop logs in
-> server confirms entitlement
```

OpenIndieCommerce should remain provider-neutral in both cases.

## 6. Device activation

A common one-time desktop license policy is:

```text
1 license
up to N active devices
```

Recommended semantics:

- activation creates a device/instance record;
- validation checks that the instance is still authorized;
- deactivate releases the device slot;
- activation-limit checks must be atomic server-side;
- refund/chargeback revokes authorization.

Do not implement the activation count only in client-side code.

## 7. Offline use

A desktop tool should not require continuous network access unless the product genuinely needs it.

Recommended pattern:

```text
activation requires network
periodic validation uses network when available
successful validation is cached locally
short outages do not immediately lock the product
long-expired validation eventually falls back according to product policy
```

The exact offline grace period is a product decision, not a payment-provider decision.

Store local license state with restrictive file permissions where supported.

Do not log the plaintext license key.

## 8. Product metadata

When creating a checkout, metadata can connect a purchase to product context:

```json
{
  "product":"bigfileviewer",
  "edition":"pro-v1",
  "campaign":"alpha-launch"
}
```

If your application has an account:

```json
{
  "userId":"usr_123",
  "product":"desktop-pro"
}
```

Do not place secrets or sensitive local-machine data in checkout metadata.

## 9. Refund and chargeback behavior

Define this before launch.

Example policy:

```text
order.paid
-> Pro entitlement active

order.refunded
-> entitlement revoked
-> existing instances fail next validation

order.chargeback
-> entitlement revoked
-> existing instances fail next validation
```

The client should show a clear state such as:

```text
License needs attention
Purchase was refunded or revoked
```

rather than crashing or silently disabling unrelated local data.

## 10. Migration between payment providers

This is one of the main reasons to use OpenIndieCommerce.

Old design:

```text
Desktop -> Paddle-specific API
```

Provider change requires a desktop update.

OpenIndieCommerce design:

```text
Desktop -> OpenIndieCommerce entitlement/license API
                     |
             Paddle / Dodo / ...
```

A provider change can be server-side only.

Existing users should continue to validate against the same product entitlement model.

## 11. Release configuration

Desktop release builds should contain only public/non-secret configuration, for example:

```text
checkout base URL
license API base URL
product/edition identifier
```

Do not compile these into the binary:

```text
admin token
provider API key
provider webhook secret
merchant webhook secret
entitlement recovery secret
```

## 12. First production acceptance test

Before public desktop sales:

1. install the exact release build a customer will receive;
2. click Buy Pro;
3. complete a provider test/sandbox checkout;
4. verify server receives provider webhook;
5. verify canonical `order.paid`;
6. obtain/claim license;
7. activate device #1;
8. validate device #1;
9. activate up to device limit;
10. confirm one extra activation is rejected;
11. deactivate one instance;
12. confirm the slot becomes reusable;
13. test refund/revocation;
14. confirm revoked entitlement stops validating;
15. repeat with one low-value real transaction before public launch.

## 13. Operational rule

The desktop app is a **consumer** of commerce infrastructure, not the owner of it.

Keep provider adapters, webhook verification, order database, payout configuration and production deployment inside OpenIndieCommerce.

That keeps every product repository smaller and makes future products reuse the same commercial foundation.
