use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

use crate::commerce::{CanonicalOrder, CommerceEvent, CommerceStore};

type HmacSha256 = Hmac<Sha256>;
pub const LIVE_BASE_URL: &str = "https://live.dodopayments.com";
pub const TEST_BASE_URL: &str = "https://test.dodopayments.com";
pub const DEFAULT_SIGNATURE_TOLERANCE_SECONDS: u64 = 300;

#[derive(Debug, Deserialize)]
pub struct CheckoutResponse {
    pub session_id: String,
    pub checkout_url: String,
}

#[derive(Debug, Serialize)]
struct CheckoutRequest<'a> {
    product_cart: Vec<ProductItem<'a>>,
    customer: Customer<'a>,
    metadata: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    return_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cancel_url: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ProductItem<'a> {
    product_id: &'a str,
    quantity: u32,
}
#[derive(Debug, Serialize)]
struct Customer<'a> {
    email: &'a str,
}

pub struct DodoCheckoutRequest<'a> {
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub product_id: &'a str,
    pub email: &'a str,
    pub oic_session_id: &'a str,
    pub return_url: Option<&'a str>,
    pub cancel_url: Option<&'a str>,
}

pub async fn create_checkout(
    client: &reqwest::Client,
    request: DodoCheckoutRequest<'_>,
) -> Result<CheckoutResponse> {
    anyhow::ensure!(!request.api_key.trim().is_empty(), "Dodo API key is empty");
    anyhow::ensure!(
        !request.product_id.trim().is_empty(),
        "Dodo product ID is empty"
    );
    let body = CheckoutRequest {
        product_cart: vec![ProductItem {
            product_id: request.product_id,
            quantity: 1,
        }],
        customer: Customer {
            email: request.email,
        },
        metadata: BTreeMap::from([(
            "oic_checkout_session_id".into(),
            request.oic_session_id.into(),
        )]),
        return_url: request.return_url,
        cancel_url: request.cancel_url,
    };
    let response = client
        .post(format!(
            "{}/checkouts",
            request.base_url.trim_end_matches('/')
        ))
        .bearer_auth(request.api_key)
        .json(&body)
        .send()
        .await
        .context("Dodo checkout request failed")?;
    let status = response.status();
    let bytes = response.bytes().await?;
    anyhow::ensure!(
        status.is_success(),
        "Dodo checkout failed ({status}): {}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).context("invalid Dodo checkout response")
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn verify_signature(
    secret: &str,
    webhook_id: &str,
    timestamp: &str,
    signature_header: &str,
    raw_body: &[u8],
    now: u64,
    tolerance_seconds: u64,
) -> Result<()> {
    let ts = timestamp
        .parse::<u64>()
        .context("invalid Dodo webhook timestamp")?;
    anyhow::ensure!(
        now.abs_diff(ts) <= tolerance_seconds,
        "Dodo webhook timestamp is outside tolerance"
    );
    let encoded_secret = secret.strip_prefix("whsec_").unwrap_or(secret);
    let key = STANDARD
        .decode(encoded_secret)
        .context("invalid Dodo webhook secret encoding")?;
    let mut signed = format!("{webhook_id}.{timestamp}.").into_bytes();
    signed.extend_from_slice(raw_body);
    let mut mac = HmacSha256::new_from_slice(&key).context("invalid Dodo webhook secret")?;
    mac.update(&signed);
    let expected = STANDARD.encode(mac.finalize().into_bytes());
    let valid = signature_header.split_whitespace().any(|part| {
        let supplied = part.strip_prefix("v1,").unwrap_or(part);
        supplied.as_bytes() == expected.as_bytes()
    });
    anyhow::ensure!(valid, "invalid Dodo webhook signature");
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct DodoEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
}

#[derive(Debug)]
pub struct CommerceDodoOutcome {
    pub order: CanonicalOrder,
    pub inserted: bool,
    pub event: CommerceEvent,
}

#[derive(Debug, Deserialize)]
struct PaymentData {
    payment_id: String,
    currency: String,
    total_amount: i64,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct RefundData {
    payment_id: String,
    status: String,
    is_partial: bool,
}

#[derive(Debug, Deserialize)]
struct DisputeData {
    payment_id: String,
}

pub fn process_commerce_event(
    store: &CommerceStore,
    event: &DodoEvent,
) -> Result<Option<CommerceDodoOutcome>> {
    match event.event_type.as_str() {
        "payment.succeeded" => {
            let data: PaymentData = serde_json::from_value(event.data.clone())?;
            let session_id = data
                .metadata
                .get("oic_checkout_session_id")
                .context("Dodo payment metadata is missing oic_checkout_session_id")?;
            let session = store.checkout_session(session_id)?;
            let price = store.price(&session.price_id)?;
            anyhow::ensure!(
                price.provider == "dodo",
                "checkout session is not a Dodo price"
            );
            anyhow::ensure!(
                price.currency == data.currency,
                "Dodo payment currency mismatch"
            );
            anyhow::ensure!(
                price.unit_amount == data.total_amount,
                "Dodo payment amount mismatch"
            );
            let (order, inserted, event) =
                store.record_paid_order("dodo", &data.payment_id, session_id)?;
            Ok(Some(CommerceDodoOutcome {
                order,
                inserted,
                event,
            }))
        }
        "refund.succeeded" => {
            let data: RefundData = serde_json::from_value(event.data.clone())?;
            if data.status != "succeeded" || data.is_partial {
                return Ok(None);
            }
            let (order, inserted, event) =
                store.transition_order("dodo", &data.payment_id, "refunded", "order.refunded")?;
            Ok(Some(CommerceDodoOutcome {
                order,
                inserted,
                event,
            }))
        }
        "dispute.lost" => {
            let data: DisputeData = serde_json::from_value(event.data.clone())?;
            let (order, inserted, event) = store.transition_order(
                "dodo",
                &data.payment_id,
                "chargeback",
                "order.chargeback",
            )?;
            Ok(Some(CommerceDodoOutcome {
                order,
                inserted,
                event,
            }))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, id: &str, timestamp: u64, body: &[u8]) -> String {
        let key = STANDARD
            .decode(secret.strip_prefix("whsec_").unwrap())
            .unwrap();
        let mut msg = format!("{id}.{timestamp}.").into_bytes();
        msg.extend_from_slice(body);
        let mut mac = HmacSha256::new_from_slice(&key).unwrap();
        mac.update(&msg);
        format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn checkout_payload_carries_oic_session_and_customer() {
        let body = CheckoutRequest {
            product_cart: vec![ProductItem {
                product_id: "pdt_1",
                quantity: 1,
            }],
            customer: Customer {
                email: "buyer@example.com",
            },
            metadata: BTreeMap::from([("oic_checkout_session_id".into(), "cs_123".into())]),
            return_url: Some("https://shop.example/success"),
            cancel_url: Some("https://shop.example/cancel"),
        };
        let value = serde_json::to_value(body).unwrap();
        assert_eq!(value["product_cart"][0]["product_id"], "pdt_1");
        assert_eq!(value["customer"]["email"], "buyer@example.com");
        assert_eq!(value["metadata"]["oic_checkout_session_id"], "cs_123");
        assert_eq!(value["return_url"], "https://shop.example/success");
        assert_eq!(value["cancel_url"], "https://shop.example/cancel");
    }

    #[test]
    fn verifies_standard_webhook_signature_and_replay_window() {
        let secret = format!(
            "whsec_{}",
            STANDARD.encode(b"0123456789abcdef0123456789abcdef")
        );
        let body = br#"{"type":"payment.succeeded"}"#;
        let sig = sign(&secret, "msg_1", 1_000, body);
        verify_signature(&secret, "msg_1", "1000", &sig, body, 1_100, 300).unwrap();
        assert!(verify_signature(&secret, "msg_1", "1000", &sig, b"tampered", 1_100, 300).is_err());
        assert!(verify_signature(&secret, "msg_1", "1000", &sig, body, 1_301, 300).is_err());
    }

    #[test]
    fn payment_succeeded_becomes_canonical_paid_order() {
        let store = CommerceStore::memory().unwrap();
        let product = store.create_product("Report", "", "download").unwrap();
        let price = store
            .create_price(&product.id, "dodo", "USD", 1900, Some("pdt_1"))
            .unwrap();
        let session = store
            .create_checkout_session(
                &price.id,
                "buyer@example.com",
                None,
                None,
                serde_json::json!({}),
            )
            .unwrap();
        let event: DodoEvent = serde_json::from_value(serde_json::json!({
            "type":"payment.succeeded",
            "data":{"payment_id":"pay_1","currency":"USD","total_amount":1900,"metadata":{"oic_checkout_session_id":session.id}}
        })).unwrap();
        let out = process_commerce_event(&store, &event).unwrap().unwrap();
        assert!(out.inserted);
        assert_eq!(out.order.provider, "dodo");
        assert_eq!(out.event.event_type, "order.paid");
        let again = process_commerce_event(&store, &event).unwrap().unwrap();
        assert!(!again.inserted);
    }

    #[test]
    fn amount_mismatch_and_partial_refund_do_not_overgrant_or_revoke() {
        let store = CommerceStore::memory().unwrap();
        let product = store.create_product("Report", "", "download").unwrap();
        let price = store
            .create_price(&product.id, "dodo", "USD", 1900, Some("pdt_1"))
            .unwrap();
        let session = store
            .create_checkout_session(
                &price.id,
                "buyer@example.com",
                None,
                None,
                serde_json::json!({}),
            )
            .unwrap();
        let bad: DodoEvent = serde_json::from_value(serde_json::json!({"type":"payment.succeeded","data":{"payment_id":"pay_bad","currency":"USD","total_amount":1800,"metadata":{"oic_checkout_session_id":session.id}}})).unwrap();
        assert!(process_commerce_event(&store, &bad).is_err());
        let paid: DodoEvent = serde_json::from_value(serde_json::json!({"type":"payment.succeeded","data":{"payment_id":"pay_ok","currency":"USD","total_amount":1900,"metadata":{"oic_checkout_session_id":session.id}}})).unwrap();
        process_commerce_event(&store, &paid).unwrap();
        let partial: DodoEvent = serde_json::from_value(serde_json::json!({"type":"refund.succeeded","data":{"payment_id":"pay_ok","status":"succeeded","is_partial":true}})).unwrap();
        assert!(process_commerce_event(&store, &partial).unwrap().is_none());
    }
}
