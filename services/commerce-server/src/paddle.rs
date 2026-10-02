use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::Value;
use sha2::Sha256;

use crate::{
    LicenseStore,
    commerce::{CanonicalOrder, CommerceEvent, CommerceStore},
};

type HmacSha256 = Hmac<Sha256>;

pub const DEFAULT_SIGNATURE_TOLERANCE_SECONDS: u64 = 5;

#[derive(Debug, Deserialize)]
pub struct PaddleEvent {
    pub event_id: String,
    pub event_type: String,
    pub data: Value,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PaddleOutcome {
    Ignored,
    PaidRecorded {
        inserted: bool,
        transaction_id: String,
    },
    Revoked {
        revoked: bool,
        transaction_id: String,
    },
}

#[derive(Debug, Deserialize)]
struct TransactionData {
    id: String,
    status: String,
    custom_data: Option<Value>,
    items: Vec<TransactionItem>,
}

#[derive(Debug, Deserialize)]
struct TransactionItem {
    price: TransactionPrice,
}

#[derive(Debug, Deserialize)]
struct TransactionPrice {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AdjustmentData {
    action: String,
    status: String,
    #[serde(rename = "type")]
    adjustment_type: Option<String>,
    transaction_id: String,
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn verify_signature(
    secret: &str,
    signature_header: &str,
    raw_body: &[u8],
    now: u64,
    tolerance_seconds: u64,
) -> Result<()> {
    anyhow::ensure!(!secret.is_empty(), "Paddle webhook secret is empty");
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in signature_header.split(';') {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        match key {
            "ts" => {
                timestamp = Some(
                    value
                        .parse::<u64>()
                        .context("invalid Paddle signature timestamp")?,
                )
            }
            "h1" => signatures.push(hex::decode(value).context("invalid Paddle signature hex")?),
            _ => {}
        }
    }
    let timestamp = timestamp.context("Paddle-Signature is missing ts")?;
    anyhow::ensure!(!signatures.is_empty(), "Paddle-Signature is missing h1");
    anyhow::ensure!(
        now.abs_diff(timestamp) <= tolerance_seconds,
        "Paddle webhook signature timestamp is outside tolerance"
    );

    let mut signed_payload = timestamp.to_string().into_bytes();
    signed_payload.push(b':');
    signed_payload.extend_from_slice(raw_body);

    let mut valid = false;
    for signature in signatures {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .context("invalid Paddle webhook secret")?;
        mac.update(&signed_payload);
        if mac.verify_slice(&signature).is_ok() {
            valid = true;
            break;
        }
    }
    anyhow::ensure!(valid, "invalid Paddle webhook signature");
    Ok(())
}

#[derive(Debug)]
pub struct CommercePaddleOutcome {
    pub order: CanonicalOrder,
    pub inserted: bool,
    pub event: CommerceEvent,
}

pub fn process_commerce_event(
    store: &CommerceStore,
    event: &PaddleEvent,
) -> Result<Option<CommercePaddleOutcome>> {
    match event.event_type.as_str() {
        "transaction.completed" => {
            let data: TransactionData = serde_json::from_value(event.data.clone())?;
            if data.status != "completed" {
                return Ok(None);
            }
            let custom = data
                .custom_data
                .context("Paddle transaction is missing custom_data")?;
            let session_id = custom
                .get("oic_checkout_session_id")
                .and_then(Value::as_str)
                .context("Paddle custom_data is missing oic_checkout_session_id")?;
            let session = store.checkout_session(session_id)?;
            let price = store.price(&session.price_id)?;
            anyhow::ensure!(
                price.provider == "paddle",
                "checkout session is not a Paddle price"
            );
            let expected = price
                .provider_price_id
                .as_deref()
                .context("Paddle price is missing provider_price_id")?;
            anyhow::ensure!(
                data.items.iter().any(|item| item.price.id == expected),
                "Paddle transaction does not contain the expected price"
            );
            let (order, inserted, event) =
                store.record_paid_order("paddle", &data.id, session_id)?;
            Ok(Some(CommercePaddleOutcome {
                order,
                inserted,
                event,
            }))
        }
        "adjustment.created" | "adjustment.updated" => {
            let data: AdjustmentData = serde_json::from_value(event.data.clone())?;
            let transition = if data.action == "refund"
                && data.status == "approved"
                && data.adjustment_type.as_deref() == Some("full")
            {
                Some(("refunded", "order.refunded"))
            } else if data.action == "chargeback" {
                Some(("chargeback", "order.chargeback"))
            } else {
                None
            };
            let Some((status, event_type)) = transition else {
                return Ok(None);
            };
            let (order, inserted, event) =
                store.transition_order("paddle", &data.transaction_id, status, event_type)?;
            Ok(Some(CommercePaddleOutcome {
                order,
                inserted,
                event,
            }))
        }
        _ => Ok(None),
    }
}

pub fn process_event(
    store: &LicenseStore,
    expected_price_id: &str,
    event: PaddleEvent,
) -> Result<PaddleOutcome> {
    match event.event_type.as_str() {
        "transaction.completed" => {
            let data: TransactionData = serde_json::from_value(event.data)?;
            if data.status != "completed"
                || !data
                    .items
                    .iter()
                    .any(|item| item.price.id == expected_price_id)
            {
                return Ok(PaddleOutcome::Ignored);
            }
            let custom = data
                .custom_data
                .context("Paddle transaction is missing custom_data")?;
            let email = custom
                .get("oic_email")
                .and_then(Value::as_str)
                .context("Paddle custom_data is missing oic_email")?;
            let claim_token = custom
                .get("oic_claim_token")
                .and_then(Value::as_str)
                .context("Paddle custom_data is missing oic_claim_token")?;
            let inserted = store.record_paid_order("paddle", &data.id, email, claim_token)?;
            Ok(PaddleOutcome::PaidRecorded {
                inserted,
                transaction_id: data.id,
            })
        }
        "adjustment.created" | "adjustment.updated" => {
            let data: AdjustmentData = serde_json::from_value(event.data)?;
            let revoke_refund = data.action == "refund"
                && data.status == "approved"
                && data.adjustment_type.as_deref() == Some("full");
            let revoke_chargeback = data.action == "chargeback";
            if !(revoke_refund || revoke_chargeback) {
                return Ok(PaddleOutcome::Ignored);
            }
            let revoked = store.revoke_by_order("paddle", &data.transaction_id)?;
            Ok(PaddleOutcome::Revoked {
                revoked,
                transaction_id: data.transaction_id,
            })
        }
        _ => Ok(PaddleOutcome::Ignored),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClaimRequest, InstanceRequest};

    fn signature(secret: &str, timestamp: u64, body: &[u8]) -> String {
        let mut payload = timestamp.to_string().into_bytes();
        payload.push(b':');
        payload.extend_from_slice(body);
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(&payload);
        format!(
            "ts={timestamp};h1={}",
            hex::encode(mac.finalize().into_bytes())
        )
    }

    #[test]
    fn verifies_raw_body_signature_and_rejects_tamper_or_replay() {
        let body = br#"{"event_type":"transaction.completed"}"#;
        let header = signature("whsec_test", 1_000, body);
        verify_signature("whsec_test", &header, body, 1_003, 5).unwrap();
        assert!(verify_signature("whsec_test", &header, b"changed", 1_003, 5).is_err());
        assert!(verify_signature("whsec_test", &header, body, 1_006, 5).is_err());
    }

    #[test]
    fn completed_transaction_records_claimable_order_once() {
        let store = LicenseStore::memory().unwrap();
        let token = "claim-token-fixture-not-secret-01";
        let event: PaddleEvent = serde_json::from_value(serde_json::json!({
            "event_id":"evt_test",
            "event_type":"transaction.completed",
            "data":{
                "id":"txn_paid",
                "status":"completed",
                "custom_data":{"oic_email":"buyer@example.com","oic_claim_token":token},
                "items":[{"price":{"id":"pri_bfv"}}]
            }
        }))
        .unwrap();
        assert_eq!(
            process_event(&store, "pri_bfv", event).unwrap(),
            PaddleOutcome::PaidRecorded {
                inserted: true,
                transaction_id: "txn_paid".into()
            }
        );
        let duplicate: PaddleEvent = serde_json::from_value(serde_json::json!({
            "event_id":"evt_test_retry",
            "event_type":"transaction.completed",
            "data":{"id":"txn_paid","status":"completed","custom_data":{"oic_email":"buyer@example.com","oic_claim_token":token},"items":[{"price":{"id":"pri_bfv"}}]}
        })).unwrap();
        assert_eq!(
            process_event(&store, "pri_bfv", duplicate).unwrap(),
            PaddleOutcome::PaidRecorded {
                inserted: false,
                transaction_id: "txn_paid".into()
            }
        );
        assert!(
            store
                .claim_paid_order(
                    &ClaimRequest {
                        email: "buyer@example.com".into(),
                        claim_token: token.into()
                    },
                    b"claim-token-fixture-not-secret-01"
                )
                .is_ok()
        );
    }

    #[test]
    fn wrong_price_is_ignored_and_full_approved_refund_revokes() {
        let store = LicenseStore::memory().unwrap();
        let token = "claim-token-fixture-not-secret-02";
        let wrong: PaddleEvent = serde_json::from_value(serde_json::json!({
            "event_id":"evt_wrong","event_type":"transaction.completed",
            "data":{"id":"txn_wrong","status":"completed","custom_data":{"oic_email":"a@b.com","oic_claim_token":token},"items":[{"price":{"id":"pri_other"}}]}
        })).unwrap();
        assert_eq!(
            process_event(&store, "pri_bfv", wrong).unwrap(),
            PaddleOutcome::Ignored
        );

        store
            .record_paid_order("paddle", "txn_refund", "a@b.com", token)
            .unwrap();
        let issued = store
            .claim_paid_order(
                &ClaimRequest {
                    email: "a@b.com".into(),
                    claim_token: token.into(),
                },
                b"claim-token-fixture-not-secret-01",
            )
            .unwrap();
        let active = store
            .activate(&crate::ActivateRequest {
                license_key: issued.license_key.clone(),
                email: "a@b.com".into(),
                instance_name: "one".into(),
            })
            .unwrap();
        let refund: PaddleEvent = serde_json::from_value(serde_json::json!({
            "event_id":"evt_refund","event_type":"adjustment.created",
            "data":{"action":"refund","status":"approved","type":"full","transaction_id":"txn_refund"}
        })).unwrap();
        assert_eq!(
            process_event(&store, "pri_bfv", refund).unwrap(),
            PaddleOutcome::Revoked {
                revoked: true,
                transaction_id: "txn_refund".into()
            }
        );
        assert!(
            store
                .validate(&InstanceRequest {
                    license_key: issued.license_key,
                    instance_id: active.instance_id
                })
                .is_err()
        );
    }

    #[test]
    fn generic_checkout_session_becomes_canonical_paid_order() {
        let store = CommerceStore::memory().unwrap();
        let product = store
            .create_product("Report", "Digital report", "download")
            .unwrap();
        let price = store
            .create_price(&product.id, "paddle", "USD", 1900, Some("pri_generic"))
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
        let event: PaddleEvent=serde_json::from_value(serde_json::json!({
            "event_id":"evt_generic","event_type":"transaction.completed",
            "data":{"id":"txn_generic","status":"completed","custom_data":{"oic_checkout_session_id":session.id},"items":[{"price":{"id":"pri_generic"}}]}
        })).unwrap();
        let outcome = process_commerce_event(&store, &event).unwrap().unwrap();
        assert!(outcome.inserted);
        assert_eq!(outcome.order.amount, 1900);
        assert_eq!(outcome.event.event_type, "order.paid");
    }

    #[test]
    fn generic_full_refund_becomes_canonical_refund_event() {
        let store = CommerceStore::memory().unwrap();
        let product = store.create_product("Report", "", "download").unwrap();
        let price = store
            .create_price(&product.id, "paddle", "USD", 1900, Some("pri_generic"))
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
        let paid:PaddleEvent=serde_json::from_value(serde_json::json!({"event_id":"evt_paid","event_type":"transaction.completed","data":{"id":"txn_refund_generic","status":"completed","custom_data":{"oic_checkout_session_id":session.id},"items":[{"price":{"id":"pri_generic"}}]}})).unwrap();
        process_commerce_event(&store, &paid).unwrap().unwrap();
        let refund:PaddleEvent=serde_json::from_value(serde_json::json!({"event_id":"evt_refund","event_type":"adjustment.created","data":{"action":"refund","status":"approved","type":"full","transaction_id":"txn_refund_generic"}})).unwrap();
        let outcome = process_commerce_event(&store, &refund).unwrap().unwrap();
        assert!(outcome.inserted);
        assert_eq!(outcome.order.status, "refunded");
        assert_eq!(outcome.event.event_type, "order.refunded");
    }

    #[test]
    fn partial_or_pending_refund_does_not_revoke() {
        let store = LicenseStore::memory().unwrap();
        for (status, kind) in [("approved", "partial"), ("pending_approval", "full")] {
            let event: PaddleEvent = serde_json::from_value(serde_json::json!({
                "event_id":"evt_ignore","event_type":"adjustment.updated",
                "data":{"action":"refund","status":status,"type":kind,"transaction_id":"txn_any"}
            }))
            .unwrap();
            assert_eq!(
                process_event(&store, "pri_bfv", event).unwrap(),
                PaddleOutcome::Ignored
            );
        }
    }
}
