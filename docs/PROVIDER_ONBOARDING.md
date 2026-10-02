# Provider Onboarding Runbook

This runbook turns OpenIndieCommerce provider setup into explicit, repeatable release gates. Do not mark a rail production-verified until a real transaction and payout/settlement have both succeeded.

## 1. Dodo Payments — overseas MoR backup

Best first target for a mainland-China individual developer.

Official China onboarding currently advertises support for Chinese identity documents and individual registration without enterprise verification. The published flow is: product information -> identity verification (ID + selfie) -> payout bank account.

1. Create the Dodo merchant account and finish identity verification.
2. Create a one-time Dodo product and record its `product_id`.
3. Create test API credentials and a Standard Webhooks endpoint for `/v1/webhooks/dodo`.
4. Configure OpenIndieCommerce with `OIC_DODO_ENV=test`, the API key and webhook secret using `_FILE` secrets in production-like environments.
5. Run the fail-closed readiness check:

```bash
export OIC_ACCEPTANCE_ADMIN_TOKEN='...'
python3 scripts/provider_acceptance.py \
  --base-url https://commerce.example.com \
  --provider dodo \
  --dry-run
```

6. Create an actual test Checkout Session:

```bash
export OIC_DODO_PRODUCT_ID='pdt_...'
python3 scripts/provider_acceptance.py \
  --base-url https://commerce.example.com \
  --provider dodo \
  --email acceptance@example.com \
  --amount 100
```

7. Complete the provider checkout and verify `payment.succeeded -> order.paid`.
8. Verify merchant webhook delivery, entitlement/license fulfillment if enabled, full refund revocation, and `dispute.lost -> order.chargeback`.
9. Switch to `OIC_DODO_ENV=live` only after test mode passes.
10. Complete one low-value real transaction and confirm the first payout reaches the configured destination.

Official reference: https://dodopayments.com/zh

## 2. Paddle — overseas primary MoR

Paddle currently states that individuals/sole traders do not need business identification, but the seller still completes identity verification. Live selling also requires domain review. Sandbox and live are separate environments with separate credentials and catalog data.

1. Create both Sandbox and Live accounts; begin Live verification early while integrating in Sandbox.
2. In Sandbox, create the Product/Price, client-side token and notification destination for `/v1/webhooks/paddle`.
3. Use Sandbox for success, decline, 3DS, refund, email and webhook retry testing. Sandbox does not require website approval.
4. For Live domain review, publish an HTTPS site that clearly shows product/service description, pricing, included deliverables, Terms, Refund Policy, Privacy Policy, seller/brand identity and buyer support contact details.
5. Submit the checkout domain for review and complete identity verification.
6. Recreate the required Product/Price, credentials and notification destination in Live; Sandbox resources are not shared with Live.
7. Complete one low-value real transaction and verify `transaction.completed -> order.paid`.
8. Complete a full approved refund and verify `order.refunded` plus entitlement revocation.
9. Confirm the first payout reaches the configured destination.

Official references:
- https://developer.paddle.com/sdks/sandbox/
- https://developer.paddle.com/build/set-up-checklist/
- https://www.paddle.com/help/start/account-verification/what-is-domain-verification
- https://www.paddle.com/help/start/account-verification/what-is-identity-verification

## 3. ZPAY — mainland-China direct rail

ZPAY currently advertises individual payment access without a business license for supported channels and states that settlement is handled through official Alipay/WeChat channels. It is a direct domestic collection rail, not a Merchant of Record; tax/compliance responsibilities therefore differ from an overseas MoR.

1. Register the ZPAY account and apply for the required Alipay and/or WeChat channel.
2. Obtain the merchant `pid` and key; store the key only server-side (`OIC_ZPAY_KEY_FILE` preferred).
3. Configure the public HTTPS base URL so ZPAY can reach `/v1/webhooks/zpay`.
4. Complete one low-value hosted checkout.
5. Verify callback signature, `TRADE_SUCCESS`, exact amount, merchant PID, duplicate callback idempotency and canonical `order.paid`.
6. The callback handler must return `success` after successful processing because ZPAY retries failed or non-success callbacks.
7. Confirm the real settlement reaches the configured domestic destination.

Official references:
- https://zpayz.cn/
- https://api.zpayz.cn/doc.html

## Production rule

A provider is `implemented` when adapter tests pass. It is `sandbox-verified` when a real provider test transaction reaches the canonical OpenIndieCommerce order flow. It is `production-verified` only after a real transaction, refund/reversal path where applicable, and real payout/settlement have been observed.

Never put provider API keys, signing secrets, payout credentials or identity documents in Git.