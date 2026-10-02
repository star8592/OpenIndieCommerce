# Architecture

```text
Website / Desktop / SaaS
        |
        v
OpenIndieCommerce
  checkout sessions
  canonical orders
  webhook normalization
  refunds / chargebacks
  entitlements / licenses
        |
        +--> Paddle / MoR
        +--> ZPAY / China direct rail
        +--> future Dodo / Creem / Stripe
```

Provider adapters translate provider-specific APIs into canonical commerce events. The application integrates once with OpenIndieCommerce.
