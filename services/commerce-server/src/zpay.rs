use std::collections::BTreeMap;

use anyhow::{Context, Result};

use crate::{
    LicenseStore,
    commerce::{CanonicalOrder, CommerceEvent, CommerceStore},
};

pub const DEFAULT_SUBMIT_URL: &str = "https://zpayz.cn/submit.php";

#[derive(Debug, PartialEq, Eq)]
pub struct NotificationOutcome {
    pub inserted: bool,
    pub order_id: String,
}

pub fn sign(params: &BTreeMap<String, String>, merchant_key: &str) -> String {
    let canonical = params
        .iter()
        .filter(|(key, value)| *key != "sign" && *key != "sign_type" && !value.is_empty())
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    format!("{:x}", md5::compute(format!("{canonical}{merchant_key}")))
}

pub fn verify_signature(params: &BTreeMap<String, String>, merchant_key: &str) -> Result<()> {
    let supplied = params
        .get("sign")
        .context("ZPAY notification is missing sign")?;
    anyhow::ensure!(
        supplied.eq_ignore_ascii_case(&sign(params, merchant_key)),
        "invalid ZPAY notification signature"
    );
    Ok(())
}

pub fn parse_cny_minor(value: &str) -> Result<i64> {
    let value = value.trim();
    anyhow::ensure!(!value.is_empty(), "empty CNY amount");
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let frac = parts.next();
    anyhow::ensure!(parts.next().is_none(), "invalid CNY amount");
    anyhow::ensure!(
        !whole.is_empty() && whole.chars().all(|c| c.is_ascii_digit()),
        "invalid CNY amount"
    );
    let yuan: i64 = whole.parse()?;
    let cents = match frac {
        None => 0,
        Some(v) if v.len() == 1 && v.chars().all(|c| c.is_ascii_digit()) => v.parse::<i64>()? * 10,
        Some(v) if v.len() == 2 && v.chars().all(|c| c.is_ascii_digit()) => v.parse::<i64>()?,
        _ => anyhow::bail!("invalid CNY amount"),
    };
    yuan.checked_mul(100)
        .and_then(|v| v.checked_add(cents))
        .context("CNY amount overflow")
}

pub fn format_cny_minor(value: i64) -> Result<String> {
    anyhow::ensure!(value > 0, "CNY amount must be positive");
    Ok(format!("{}.{:02}", value / 100, value % 100))
}

#[derive(Debug)]
pub struct CommerceNotificationOutcome {
    pub order: CanonicalOrder,
    pub inserted: bool,
    pub event: CommerceEvent,
}

pub fn process_commerce_notification(
    store: &CommerceStore,
    expected_pid: &str,
    merchant_key: &str,
    params: &BTreeMap<String, String>,
) -> Result<CommerceNotificationOutcome> {
    verify_signature(params, merchant_key)?;
    anyhow::ensure!(
        params.get("pid").map(String::as_str) == Some(expected_pid),
        "ZPAY pid mismatch"
    );
    anyhow::ensure!(
        params.get("trade_status").map(String::as_str) == Some("TRADE_SUCCESS"),
        "ZPAY trade is not successful"
    );
    let session_id = params
        .get("out_trade_no")
        .context("ZPAY notification is missing out_trade_no")?;
    let provider_order_id = params
        .get("trade_no")
        .context("ZPAY notification is missing trade_no")?;
    let amount = parse_cny_minor(
        params
            .get("money")
            .context("ZPAY notification is missing money")?,
    )?;
    let session = store.checkout_session(session_id)?;
    let price = store.price(&session.price_id)?;
    anyhow::ensure!(
        price.provider == "zpay",
        "checkout session is not a ZPAY price"
    );
    anyhow::ensure!(price.currency == "CNY", "ZPAY checkout requires CNY price");
    anyhow::ensure!(price.unit_amount == amount, "ZPAY amount mismatch");
    let (order, inserted, event) =
        store.record_paid_order("zpay", provider_order_id, session_id)?;
    Ok(CommerceNotificationOutcome {
        order,
        inserted,
        event,
    })
}

pub fn process_notification(
    store: &LicenseStore,
    expected_pid: &str,
    merchant_key: &str,
    params: &BTreeMap<String, String>,
) -> Result<NotificationOutcome> {
    verify_signature(params, merchant_key)?;
    anyhow::ensure!(
        params.get("pid").map(String::as_str) == Some(expected_pid),
        "ZPAY pid mismatch"
    );
    anyhow::ensure!(
        params.get("trade_status").map(String::as_str) == Some("TRADE_SUCCESS"),
        "ZPAY trade is not successful"
    );
    let order_id = params
        .get("out_trade_no")
        .context("ZPAY notification is missing out_trade_no")?;
    let amount = parse_cny_minor(
        params
            .get("money")
            .context("ZPAY notification is missing money")?,
    )?;
    let inserted = store.confirm_pending_order("zpay", order_id, amount)?;
    Ok(NotificationOutcome {
        inserted,
        order_id: order_id.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClaimRequest;

    #[test]
    fn signature_sorts_fields_and_ignores_sign_fields_and_empty_values() {
        let mut p = BTreeMap::from([
            ("b".to_string(), "2".to_string()),
            ("a".to_string(), "1".to_string()),
            ("empty".to_string(), String::new()),
            ("sign_type".to_string(), "MD5".to_string()),
        ]);
        let expected = format!("{:x}", md5::compute("a=1&b=2secret"));
        assert_eq!(sign(&p, "secret"), expected);
        p.insert("sign".into(), expected);
        verify_signature(&p, "secret").unwrap();
    }

    #[test]
    fn cny_amount_parser_is_exact() {
        assert_eq!(parse_cny_minor("19").unwrap(), 1900);
        assert_eq!(parse_cny_minor("19.9").unwrap(), 1990);
        assert_eq!(parse_cny_minor("19.90").unwrap(), 1990);
        assert!(parse_cny_minor("19.901").is_err());
        assert!(parse_cny_minor("19x").is_err());
    }

    #[test]
    fn signed_success_notification_is_idempotent_and_claimable() {
        let store = LicenseStore::memory().unwrap();
        let claim = "zpay-claim-token-fixture-1234567890";
        store
            .create_pending_order("zpay", "202610020001", "buyer@example.com", claim, 9900)
            .unwrap();
        let mut p = BTreeMap::from([
            ("pid".into(), "merchant1".into()),
            ("out_trade_no".into(), "202610020001".into()),
            ("trade_no".into(), "zp123".into()),
            ("trade_status".into(), "TRADE_SUCCESS".into()),
            ("money".into(), "99.00".into()),
            ("type".into(), "alipay".into()),
            ("name".into(), "OpenIndieCommerce Pro v1".into()),
            ("sign_type".into(), "MD5".into()),
        ]);
        p.insert("sign".into(), sign(&p, "merchant-secret"));
        assert!(
            process_notification(&store, "merchant1", "merchant-secret", &p)
                .unwrap()
                .inserted
        );
        assert!(
            !process_notification(&store, "merchant1", "merchant-secret", &p)
                .unwrap()
                .inserted
        );
        assert!(
            store
                .claim_paid_order(
                    &ClaimRequest {
                        email: "buyer@example.com".into(),
                        claim_token: claim.into()
                    },
                    b"claim-token-fixture-not-secret-01"
                )
                .is_ok()
        );
    }

    #[test]
    fn generic_zpay_session_becomes_canonical_paid_order() {
        let store = CommerceStore::memory().unwrap();
        let product = store
            .create_product("Course", "Video course", "entitlement")
            .unwrap();
        let price = store
            .create_price(&product.id, "zpay", "CNY", 9900, None)
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
        let mut p = BTreeMap::from([
            ("pid".into(), "merchant1".into()),
            ("out_trade_no".into(), session.id.clone()),
            ("trade_no".into(), "zp_generic_1".into()),
            ("trade_status".into(), "TRADE_SUCCESS".into()),
            ("money".into(), "99.00".into()),
            ("type".into(), "alipay".into()),
            ("name".into(), "Course".into()),
            ("sign_type".into(), "MD5".into()),
        ]);
        p.insert("sign".into(), sign(&p, "merchant-secret"));
        let outcome =
            process_commerce_notification(&store, "merchant1", "merchant-secret", &p).unwrap();
        assert!(outcome.inserted);
        assert_eq!(outcome.order.amount, 9900);
        assert_eq!(outcome.event.event_type, "order.paid");
        let again =
            process_commerce_notification(&store, "merchant1", "merchant-secret", &p).unwrap();
        assert!(!again.inserted);
    }

    #[test]
    fn wrong_amount_or_signature_is_rejected() {
        let store = LicenseStore::memory().unwrap();
        store
            .create_pending_order(
                "zpay",
                "42",
                "a@b.com",
                "claim-token-fixture-long-enough",
                9900,
            )
            .unwrap();
        let mut p = BTreeMap::from([
            ("pid".into(), "merchant1".into()),
            ("out_trade_no".into(), "42".into()),
            ("trade_status".into(), "TRADE_SUCCESS".into()),
            ("money".into(), "98.00".into()),
        ]);
        p.insert("sign".into(), sign(&p, "secret"));
        assert!(process_notification(&store, "merchant1", "secret", &p).is_err());
        p.insert("sign".into(), "bad".into());
        assert!(process_notification(&store, "merchant1", "secret", &p).is_err());
    }
}
