use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use rand::RngCore;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MerchantWebhookEvent<T> {
    pub id: String,
    pub event_type: String,
    pub created_at: i64,
    pub data: T,
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn new_secret() -> String {
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    format!("whsec_{}", hex::encode(b))
}

pub fn signature(secret: &str, timestamp: i64, body: &[u8]) -> Result<String> {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).context("invalid webhook secret")?;
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    Ok(format!(
        "t={timestamp};v1={}",
        hex::encode(mac.finalize().into_bytes())
    ))
}

pub fn verify(secret: &str, header: &str, body: &[u8], now: i64, tolerance: i64) -> Result<()> {
    let mut ts = None;
    let mut sigs = Vec::new();
    for part in header.split(';') {
        if let Some((k, v)) = part.trim().split_once('=') {
            match k {
                "t" => ts = Some(v.parse::<i64>()?),
                "v1" => sigs.push(hex::decode(v)?),
                _ => {}
            }
        }
    }
    let ts = ts.context("missing webhook timestamp")?;
    anyhow::ensure!(
        (now - ts).abs() <= tolerance,
        "webhook signature timestamp outside tolerance"
    );
    let mut payload = ts.to_string().into_bytes();
    payload.push(b'.');
    payload.extend_from_slice(body);
    anyhow::ensure!(!sigs.is_empty(), "missing webhook signature");
    for sig in sigs {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
        mac.update(&payload);
        if mac.verify_slice(&sig).is_ok() {
            return Ok(());
        }
    }
    anyhow::bail!("invalid webhook signature")
}

pub async fn deliver(
    client: &reqwest::Client,
    url: &str,
    secret: &str,
    event_id: &str,
    event_type: &str,
    body: &[u8],
) -> Result<StatusCode> {
    let ts = now_unix();
    let sig = signature(secret, ts, body)?;
    let response = client
        .post(url)
        .header("content-type", "application/json")
        .header("OpenIndie-Event-Id", event_id)
        .header("OpenIndie-Event-Type", event_type)
        .header("OpenIndie-Signature", sig)
        .body(body.to_vec())
        .send()
        .await?;
    Ok(response.status())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signs_and_verifies_and_rejects_tamper() {
        let body = br#"{"id":"evt_1"}"#;
        let h = signature("secret", 1000, body).unwrap();
        verify("secret", &h, body, 1003, 5).unwrap();
        assert!(verify("secret", &h, b"changed", 1003, 5).is_err());
        assert!(verify("secret", &h, body, 1006, 5).is_err());
    }
    #[test]
    fn secret_has_public_prefix_and_high_entropy_body() {
        let s = new_secret();
        assert!(s.starts_with("whsec_"));
        assert!(s.len() > 60);
    }
}
