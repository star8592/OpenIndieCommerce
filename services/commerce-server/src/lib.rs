pub mod commerce;
pub mod paddle;
pub mod webhook;
pub mod zpay;

use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub const DEFAULT_MAX_ACTIVATIONS: u32 = 3;

#[derive(Clone)]
pub struct LicenseStore {
    db: Arc<Mutex<Connection>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedLicense {
    pub license_key: String,
    pub email: String,
    pub max_activations: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateRequest {
    pub license_key: String,
    pub email: String,
    pub instance_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationResponse {
    pub valid: bool,
    pub instance_id: String,
    pub activation_usage: u32,
    pub activation_limit: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRequest {
    pub license_key: String,
    pub instance_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRequest {
    pub email: String,
    pub claim_token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResponse {
    pub valid: bool,
    pub activation_usage: u32,
    pub activation_limit: u32,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn normalize_email(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}
fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '-')
        .flat_map(char::to_uppercase)
        .collect()
}
fn key_hash(value: &str) -> Vec<u8> {
    Sha256::digest(normalize_key(value).as_bytes()).to_vec()
}

fn claim_token_hash(value: &str) -> Vec<u8> {
    Sha256::digest(value.trim().as_bytes()).to_vec()
}

fn random_hex(bytes: usize) -> String {
    let mut raw = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut raw);
    hex::encode_upper(raw)
}

fn new_license_key() -> String {
    let body = random_hex(20);
    format!(
        "OIC1-{}-{}-{}-{}-{}",
        &body[0..8],
        &body[8..16],
        &body[16..24],
        &body[24..32],
        &body[32..40]
    )
}

fn derived_license_key(
    secret: &[u8],
    provider: &str,
    order_id: &str,
    email: &str,
) -> Result<String> {
    anyhow::ensure!(
        secret.len() >= 32,
        "license key derivation secret must be at least 32 bytes"
    );
    let mut mac =
        HmacSha256::new_from_slice(secret).context("invalid license derivation secret")?;
    mac.update(provider.trim().as_bytes());
    mac.update(&[0]);
    mac.update(order_id.trim().as_bytes());
    mac.update(&[0]);
    mac.update(normalize_email(email).as_bytes());
    let raw = mac.finalize().into_bytes();
    let body = hex::encode_upper(&raw[..20]);
    Ok(format!(
        "OIC1-{}-{}-{}-{}-{}",
        &body[0..8],
        &body[8..16],
        &body[16..24],
        &body[24..32],
        &body[32..40]
    ))
}

fn new_instance_id() -> String {
    format!("inst_{}", random_hex(16).to_ascii_lowercase())
}

impl LicenseStore {
    pub fn memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn open(path: &str) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON;
             PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS licenses(
               id INTEGER PRIMARY KEY,
               key_hash BLOB NOT NULL UNIQUE,
               email TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'active',
               max_activations INTEGER NOT NULL,
               order_provider TEXT,
               order_id TEXT,
               created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS activations(
               id TEXT PRIMARY KEY,
               license_id INTEGER NOT NULL REFERENCES licenses(id),
               instance_name TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               last_seen INTEGER NOT NULL,
               deactivated_at INTEGER
             );
             CREATE TABLE IF NOT EXISTS pending_orders(
               provider TEXT NOT NULL,
               order_id TEXT NOT NULL,
               email TEXT NOT NULL,
               claim_token_hash BLOB NOT NULL UNIQUE,
               amount_minor INTEGER NOT NULL,
               status TEXT NOT NULL DEFAULT 'pending',
               created_at INTEGER NOT NULL,
               PRIMARY KEY(provider, order_id)
             );
             CREATE TABLE IF NOT EXISTS paid_orders(
               provider TEXT NOT NULL,
               order_id TEXT NOT NULL,
               email TEXT NOT NULL,
               claim_token_hash BLOB NOT NULL UNIQUE,
               status TEXT NOT NULL DEFAULT 'paid',
               license_id INTEGER REFERENCES licenses(id),
               created_at INTEGER NOT NULL,
               PRIMARY KEY(provider, order_id)
             );
             CREATE INDEX IF NOT EXISTS idx_activations_license ON activations(license_id);
             CREATE UNIQUE INDEX IF NOT EXISTS idx_licenses_order ON licenses(order_provider,order_id)
               WHERE order_provider IS NOT NULL AND order_id IS NOT NULL;",
        )?;
        Ok(Self {
            db: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn health_check(&self) -> Result<()> {
        let value: i64 = self
            .db
            .lock()
            .unwrap()
            .query_row("SELECT 1", [], |row| row.get(0))?;
        anyhow::ensure!(value == 1, "database health check failed");
        Ok(())
    }

    pub fn create_pending_order(
        &self,
        provider: &str,
        order_id: &str,
        email: &str,
        claim_token: &str,
        amount_minor: i64,
    ) -> Result<()> {
        let provider = provider.trim();
        let order_id = order_id.trim();
        let email = normalize_email(email);
        anyhow::ensure!(!provider.is_empty(), "payment provider is required");
        anyhow::ensure!(!order_id.is_empty(), "order ID is required");
        anyhow::ensure!(email.contains('@'), "valid customer email is required");
        anyhow::ensure!(claim_token.trim().len() >= 24, "claim token is too short");
        anyhow::ensure!(amount_minor > 0, "payment amount must be positive");
        self.db.lock().unwrap().execute(
            "INSERT INTO pending_orders(provider,order_id,email,claim_token_hash,amount_minor,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![provider, order_id, email, claim_token_hash(claim_token), amount_minor, now()],
        )?;
        Ok(())
    }

    pub fn confirm_pending_order(
        &self,
        provider: &str,
        order_id: &str,
        amount_minor: i64,
    ) -> Result<bool> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let row: Option<(String, Vec<u8>, i64, String)> = tx.query_row(
            "SELECT email,claim_token_hash,amount_minor,status FROM pending_orders WHERE provider=?1 AND order_id=?2",
            params![provider.trim(), order_id.trim()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        let (email, claim_hash, expected_amount, status) =
            row.context("pending payment order not found")?;
        anyhow::ensure!(expected_amount == amount_minor, "payment amount mismatch");
        anyhow::ensure!(
            status == "pending" || status == "paid",
            "payment order is not payable"
        );
        let changed = tx.execute(
            "INSERT OR IGNORE INTO paid_orders(provider,order_id,email,claim_token_hash,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![provider.trim(), order_id.trim(), email, claim_hash, now()],
        )?;
        tx.execute(
            "UPDATE pending_orders SET status='paid' WHERE provider=?1 AND order_id=?2",
            params![provider.trim(), order_id.trim()],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }

    pub fn record_paid_order(
        &self,
        provider: &str,
        order_id: &str,
        email: &str,
        claim_token: &str,
    ) -> Result<bool> {
        let provider = provider.trim();
        let order_id = order_id.trim();
        let email = normalize_email(email);
        anyhow::ensure!(!provider.is_empty(), "payment provider is required");
        anyhow::ensure!(!order_id.is_empty(), "order ID is required");
        anyhow::ensure!(email.contains('@'), "valid customer email is required");
        anyhow::ensure!(claim_token.trim().len() >= 24, "claim token is too short");
        let changed = self.db.lock().unwrap().execute(
            "INSERT OR IGNORE INTO paid_orders(provider,order_id,email,claim_token_hash,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![provider, order_id, email, claim_token_hash(claim_token), now()],
        )?;
        Ok(changed == 1)
    }

    pub fn claim_paid_order(
        &self,
        request: &ClaimRequest,
        key_secret: &[u8],
    ) -> Result<IssuedLicense> {
        let email = normalize_email(&request.email);
        let claim_hash = claim_token_hash(&request.claim_token);
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let row: Option<(String, String, String, String, Option<i64>)> = tx
            .query_row(
                "SELECT provider,order_id,email,status,license_id FROM paid_orders WHERE claim_token_hash=?1",
                params![claim_hash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        let (provider, order_id, stored_email, status, license_id) =
            row.context("paid order not found")?;
        anyhow::ensure!(status == "paid", "order is not eligible for license claim");
        anyhow::ensure!(stored_email == email, "purchase email does not match");
        let key = derived_license_key(key_secret, &provider, &order_id, &email)?;

        if let Some(license_id) = license_id {
            let stored_hash: Vec<u8> = tx
                .query_row(
                    "SELECT key_hash FROM licenses WHERE id=?1 AND status='active'",
                    params![license_id],
                    |row| row.get(0),
                )
                .context("claimed license is not active")?;
            anyhow::ensure!(
                stored_hash == key_hash(&key),
                "license recovery secret does not match existing license"
            );
            tx.commit()?;
            return Ok(IssuedLicense {
                license_key: key,
                email,
                max_activations: DEFAULT_MAX_ACTIVATIONS,
            });
        }

        tx.execute(
            "INSERT INTO licenses(key_hash,email,max_activations,order_provider,order_id,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![key_hash(&key), email, DEFAULT_MAX_ACTIVATIONS, provider, order_id, now()],
        )?;
        let new_license_id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE paid_orders SET license_id=?1 WHERE provider=?2 AND order_id=?3 AND license_id IS NULL",
            params![new_license_id, provider, order_id],
        )?;
        tx.commit()?;
        Ok(IssuedLicense {
            license_key: key,
            email,
            max_activations: DEFAULT_MAX_ACTIVATIONS,
        })
    }

    pub fn issue(
        &self,
        email: &str,
        max_activations: u32,
        provider: Option<&str>,
        order_id: Option<&str>,
    ) -> Result<IssuedLicense> {
        let email = normalize_email(email);
        anyhow::ensure!(email.contains('@'), "valid customer email is required");
        anyhow::ensure!(
            max_activations > 0 && max_activations <= 100,
            "activation limit must be 1..=100"
        );
        if let (Some(provider), Some(order_id)) = (provider, order_id) {
            let exists: bool = self.db.lock().unwrap().query_row(
                "SELECT EXISTS(SELECT 1 FROM licenses WHERE order_provider=?1 AND order_id=?2)",
                params![provider.trim(), order_id.trim()],
                |row| row.get(0),
            )?;
            anyhow::ensure!(!exists, "order already has a license");
        }
        for _ in 0..4 {
            let key = new_license_key();
            let result = self.db.lock().unwrap().execute(
                "INSERT INTO licenses(key_hash,email,max_activations,order_provider,order_id,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![key_hash(&key), email, max_activations, provider, order_id, now()],
            );
            if result.is_ok() {
                return Ok(IssuedLicense {
                    license_key: key,
                    email,
                    max_activations,
                });
            }
        }
        anyhow::bail!("could not generate a unique license key")
    }

    fn license_row(&self, key: &str) -> Result<Option<(i64, String, String, u32)>> {
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT id,email,status,max_activations FROM licenses WHERE key_hash=?1",
            params![key_hash(key)],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(Into::into)
    }

    fn active_count(&self, license_id: i64) -> Result<u32> {
        let db = self.db.lock().unwrap();
        let value: u32 = db.query_row(
            "SELECT COUNT(*) FROM activations WHERE license_id=?1 AND deactivated_at IS NULL",
            params![license_id],
            |row| row.get(0),
        )?;
        Ok(value)
    }

    pub fn revoke_by_order(&self, provider: &str, order_id: &str) -> Result<bool> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let order_changed = tx.execute(
            "UPDATE paid_orders SET status='revoked' WHERE provider=?1 AND order_id=?2 AND status='paid'",
            params![provider.trim(), order_id.trim()],
        )?;
        let license_changed = tx.execute(
            "UPDATE licenses SET status='revoked' WHERE order_provider=?1 AND order_id=?2 AND status='active'",
            params![provider.trim(), order_id.trim()],
        )?;
        tx.commit()?;
        Ok(order_changed > 0 || license_changed > 0)
    }

    pub fn activate(&self, request: &ActivateRequest) -> Result<ActivationResponse> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let key = key_hash(&request.license_key);
        let row: Option<(i64, String, String, u32)> = tx
            .query_row(
                "SELECT id,email,status,max_activations FROM licenses WHERE key_hash=?1",
                params![key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let (license_id, email, status, limit) = row.context("license key not found")?;
        anyhow::ensure!(status == "active", "license is not active");
        anyhow::ensure!(
            email == normalize_email(&request.email),
            "purchase email does not match"
        );
        let usage: u32 = tx.query_row(
            "SELECT COUNT(*) FROM activations WHERE license_id=?1 AND deactivated_at IS NULL",
            params![license_id],
            |row| row.get(0),
        )?;
        anyhow::ensure!(usage < limit, "activation limit reached");
        let instance_id = new_instance_id();
        tx.execute(
            "INSERT INTO activations(id,license_id,instance_name,created_at,last_seen) VALUES(?1,?2,?3,?4,?4)",
            params![instance_id, license_id, request.instance_name.trim(), now()],
        )?;
        tx.commit()?;
        Ok(ActivationResponse {
            valid: true,
            instance_id,
            activation_usage: usage + 1,
            activation_limit: limit,
        })
    }

    pub fn validate(&self, request: &InstanceRequest) -> Result<ValidationResponse> {
        let (license_id, _, status, limit) = self
            .license_row(&request.license_key)?
            .context("license key not found")?;
        anyhow::ensure!(status == "active", "license is not active");
        let changed = self.db.lock().unwrap().execute(
            "UPDATE activations SET last_seen=?1 WHERE id=?2 AND license_id=?3 AND deactivated_at IS NULL",
            params![now(), request.instance_id, license_id],
        )?;
        anyhow::ensure!(changed == 1, "license instance is not active");
        Ok(ValidationResponse {
            valid: true,
            activation_usage: self.active_count(license_id)?,
            activation_limit: limit,
        })
    }

    pub fn deactivate(&self, request: &InstanceRequest) -> Result<ValidationResponse> {
        let (license_id, _, status, limit) = self
            .license_row(&request.license_key)?
            .context("license key not found")?;
        anyhow::ensure!(status == "active", "license is not active");
        let changed = self.db.lock().unwrap().execute(
            "UPDATE activations SET deactivated_at=?1 WHERE id=?2 AND license_id=?3 AND deactivated_at IS NULL",
            params![now(), request.instance_id, license_id],
        )?;
        anyhow::ensure!(changed == 1, "license instance is not active");
        Ok(ValidationResponse {
            valid: true,
            activation_usage: self.active_count(license_id)?,
            activation_limit: limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_activate_validate_deactivate_round_trip() {
        let store = LicenseStore::memory().unwrap();
        let issued = store
            .issue("Buyer@Example.com", 3, Some("paddle"), Some("txn_1"))
            .unwrap();
        assert!(issued.license_key.starts_with("OIC1-"));
        let activation = store
            .activate(&ActivateRequest {
                license_key: issued.license_key.clone(),
                email: "buyer@example.com".into(),
                instance_name: "desktop-a".into(),
            })
            .unwrap();
        assert_eq!(activation.activation_usage, 1);
        assert!(
            store
                .validate(&InstanceRequest {
                    license_key: issued.license_key.clone(),
                    instance_id: activation.instance_id.clone()
                })
                .unwrap()
                .valid
        );
        assert_eq!(
            store
                .deactivate(&InstanceRequest {
                    license_key: issued.license_key.clone(),
                    instance_id: activation.instance_id
                })
                .unwrap()
                .activation_usage,
            0
        );
    }

    #[test]
    fn activation_limit_is_enforced_and_slot_can_be_reused() {
        let store = LicenseStore::memory().unwrap();
        let issued = store.issue("a@b.com", 1, None, None).unwrap();
        let first = store
            .activate(&ActivateRequest {
                license_key: issued.license_key.clone(),
                email: issued.email.clone(),
                instance_name: "one".into(),
            })
            .unwrap();
        assert!(
            store
                .activate(&ActivateRequest {
                    license_key: issued.license_key.clone(),
                    email: issued.email.clone(),
                    instance_name: "two".into()
                })
                .is_err()
        );
        store
            .deactivate(&InstanceRequest {
                license_key: issued.license_key.clone(),
                instance_id: first.instance_id,
            })
            .unwrap();
        assert!(
            store
                .activate(&ActivateRequest {
                    license_key: issued.license_key,
                    email: issued.email,
                    instance_name: "two".into()
                })
                .is_ok()
        );
    }

    #[test]
    fn concurrent_activation_cannot_exceed_limit() {
        use std::{
            sync::{Arc, Barrier},
            thread,
        };

        let store = LicenseStore::memory().unwrap();
        let issued = store.issue("a@b.com", 1, None, None).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for name in ["one", "two"] {
            let store = store.clone();
            let barrier = barrier.clone();
            let key = issued.license_key.clone();
            let email = issued.email.clone();
            handles.push(thread::spawn(move || {
                barrier.wait();
                store.activate(&ActivateRequest {
                    license_key: key,
                    email,
                    instance_name: name.into(),
                })
            }));
        }
        barrier.wait();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);
    }

    #[test]
    fn paid_order_claim_is_recoverable_without_storing_plaintext() {
        let store = LicenseStore::memory().unwrap();
        let token = "claim-token-fixture-not-secret-01";
        assert!(
            store
                .record_paid_order("paddle", "txn_claim", "Buyer@Example.com", token)
                .unwrap()
        );
        assert!(
            !store
                .record_paid_order("paddle", "txn_claim", "Buyer@Example.com", token)
                .unwrap()
        );
        let issued = store
            .claim_paid_order(
                &ClaimRequest {
                    email: "buyer@example.com".into(),
                    claim_token: token.into(),
                },
                b"claim-token-fixture-not-secret-01",
            )
            .unwrap();
        assert!(issued.license_key.starts_with("OIC1-"));
        assert!(
            store
                .claim_paid_order(
                    &ClaimRequest {
                        email: "buyer@example.com".into(),
                        claim_token: token.into(),
                    },
                    b"WRONGWRONGWRONGWRONGWRONGWRONGWR"
                )
                .is_err()
        );
    }

    #[test]
    fn refund_before_claim_blocks_claim() {
        let store = LicenseStore::memory().unwrap();
        let token = "claim-token-fixture-not-secret-02";
        store
            .record_paid_order("paddle", "txn_refund_before_claim", "a@b.com", token)
            .unwrap();
        assert!(
            store
                .revoke_by_order("paddle", "txn_refund_before_claim")
                .unwrap()
        );
        assert!(
            store
                .claim_paid_order(
                    &ClaimRequest {
                        email: "a@b.com".into(),
                        claim_token: token.into(),
                    },
                    b"claim-token-fixture-not-secret-01"
                )
                .is_err()
        );
    }

    #[test]
    fn provider_order_is_idempotent_and_refund_can_revoke() {
        let store = LicenseStore::memory().unwrap();
        let issued = store
            .issue("a@b.com", 3, Some("paddle"), Some("txn_1"))
            .unwrap();
        assert!(
            store
                .issue("a@b.com", 3, Some("paddle"), Some("txn_1"))
                .is_err()
        );
        let active = store
            .activate(&ActivateRequest {
                license_key: issued.license_key.clone(),
                email: issued.email.clone(),
                instance_name: "one".into(),
            })
            .unwrap();
        assert!(store.revoke_by_order("paddle", "txn_1").unwrap());
        assert!(
            store
                .validate(&InstanceRequest {
                    license_key: issued.license_key.clone(),
                    instance_id: active.instance_id
                })
                .is_err()
        );
        assert!(
            store
                .activate(&ActivateRequest {
                    license_key: issued.license_key,
                    email: issued.email,
                    instance_name: "two".into()
                })
                .is_err()
        );
    }

    #[test]
    fn wrong_email_and_wrong_instance_are_rejected() {
        let store = LicenseStore::memory().unwrap();
        let issued = store.issue("a@b.com", 3, None, None).unwrap();
        assert!(
            store
                .activate(&ActivateRequest {
                    license_key: issued.license_key.clone(),
                    email: "x@y.com".into(),
                    instance_name: "x".into()
                })
                .is_err()
        );
        assert!(
            store
                .validate(&InstanceRequest {
                    license_key: issued.license_key,
                    instance_id: "inst_missing".into()
                })
                .is_err()
        );
    }
}
