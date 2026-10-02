# Roadmap

## v0.1 alpha
- [x] provider-neutral Product / Price model
- [x] `POST /v1/checkout/sessions`
- [x] canonical Order model
- [x] canonical `order.paid`, `order.refunded`, `order.chargeback`
- [x] Paddle hosted-checkout + signed webhook adapter
- [x] Dodo Payments Checkout Sessions + Standard Webhooks adapter
- [x] ZPAY Alipay / WeChat hosted-checkout + signed callback adapter
- [x] reliable outbound merchant webhook outbox
- [x] optional entitlement/license module
- [x] hosted payment links
- [x] merchant compliance-page templates
- [x] systemd / Caddy / backup deployment templates
- [x] minimal zero-dependency Node web-store example

## Before v0.1 stable
- [ ] real Paddle sandbox end-to-end transaction
- [ ] real Paddle live merchant onboarding + payout validation
- [ ] real ZPAY merchant transaction + settlement validation
- [x] webhook endpoint management/list/disable API
- [x] event replay / manual redelivery endpoint
- [x] checkout-session expiry
- [x] rate limiting / abuse protection
- [x] API versioning policy and OpenAPI specification

## Later
- Creem adapter
- subscriptions and recurring entitlements
- coupons / promotion codes
- optional Stripe/PayPal direct PSP adapters
- PostgreSQL and multi-tenant hosted control plane
- admin dashboard
