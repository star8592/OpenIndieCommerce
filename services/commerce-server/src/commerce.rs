use anyhow::{Context, Result};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct CommerceStore {
    db: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: String,
    pub name: String,
    pub description: String,
    pub fulfillment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub id: String,
    pub product_id: String,
    pub provider: String,
    pub currency: String,
    pub unit_amount: i64,
    pub provider_price_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutSession {
    pub id: String,
    pub price_id: String,
    pub email: String,
    pub provider: String,
    pub status: String,
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
    pub metadata: serde_json::Value,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalOrder {
    pub id: String,
    pub provider: String,
    pub provider_order_id: String,
    pub checkout_session_id: Option<String>,
    pub price_id: String,
    pub email: String,
    pub currency: String,
    pub amount: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommerceEvent {
    pub id: String,
    pub event_type: String,
    pub order_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MerchantWebhookEndpoint {
    pub id: String,
    pub url: String,
    #[serde(skip_serializing)]
    pub secret: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct PendingWebhookDelivery {
    pub id: String,
    pub event_id: String,
    pub endpoint_id: String,
    pub url: String,
    pub secret: String,
    pub event_type: String,
    pub payload: String,
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebhookDeliveryStatus {
    pub id: String,
    pub event_id: String,
    pub endpoint_id: String,
    pub status: String,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub delivered_at: Option<i64>,
}

pub const DEFAULT_CHECKOUT_TTL_SECONDS: i64 = 30 * 60;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn random_id(prefix: &str) -> String {
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    format!("{prefix}_{}", hex::encode(b))
}
fn valid_currency(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|b| b.is_ascii_uppercase())
}

impl CommerceStore {
    pub fn memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }
    pub fn open(path: &str) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }
    fn from_connection(connection: Connection) -> Result<Self> {
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS products(id TEXT PRIMARY KEY,name TEXT NOT NULL,description TEXT NOT NULL,fulfillment TEXT NOT NULL,created_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS prices(id TEXT PRIMARY KEY,product_id TEXT NOT NULL REFERENCES products(id),provider TEXT NOT NULL,currency TEXT NOT NULL,unit_amount INTEGER NOT NULL,provider_price_id TEXT,active INTEGER NOT NULL DEFAULT 1,created_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS checkout_sessions(id TEXT PRIMARY KEY,price_id TEXT NOT NULL REFERENCES prices(id),email TEXT NOT NULL,provider TEXT NOT NULL,status TEXT NOT NULL,success_url TEXT,cancel_url TEXT,metadata_json TEXT NOT NULL,created_at INTEGER NOT NULL,expires_at INTEGER);
        CREATE TABLE IF NOT EXISTS commerce_orders(id TEXT PRIMARY KEY,provider TEXT NOT NULL,provider_order_id TEXT NOT NULL,checkout_session_id TEXT REFERENCES checkout_sessions(id),price_id TEXT NOT NULL REFERENCES prices(id),email TEXT NOT NULL,currency TEXT NOT NULL,amount INTEGER NOT NULL,status TEXT NOT NULL,created_at INTEGER NOT NULL,UNIQUE(provider,provider_order_id));
        CREATE TABLE IF NOT EXISTS commerce_events(id TEXT PRIMARY KEY,event_type TEXT NOT NULL,order_id TEXT NOT NULL REFERENCES commerce_orders(id),payload_json TEXT NOT NULL,created_at INTEGER NOT NULL,UNIQUE(event_type,order_id));
        CREATE TABLE IF NOT EXISTS merchant_webhook_endpoints(id TEXT PRIMARY KEY,url TEXT NOT NULL,secret TEXT NOT NULL,active INTEGER NOT NULL DEFAULT 1,created_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS webhook_deliveries(id TEXT PRIMARY KEY,event_id TEXT NOT NULL REFERENCES commerce_events(id),endpoint_id TEXT NOT NULL REFERENCES merchant_webhook_endpoints(id),status TEXT NOT NULL DEFAULT 'pending',attempts INTEGER NOT NULL DEFAULT 0,next_attempt_at INTEGER NOT NULL,last_error TEXT,created_at INTEGER NOT NULL,delivered_at INTEGER,UNIQUE(event_id,endpoint_id));")?;
        let _ = connection.execute(
            "ALTER TABLE checkout_sessions ADD COLUMN expires_at INTEGER",
            [],
        );
        Ok(Self {
            db: Arc::new(Mutex::new(connection)),
        })
    }
    pub fn create_product(
        &self,
        name: &str,
        description: &str,
        fulfillment: &str,
    ) -> Result<Product> {
        anyhow::ensure!(!name.trim().is_empty(), "product name is required");
        anyhow::ensure!(
            matches!(fulfillment, "none" | "entitlement" | "license" | "download"),
            "unsupported fulfillment type"
        );
        let p = Product {
            id: random_id("prod"),
            name: name.trim().into(),
            description: description.trim().into(),
            fulfillment: fulfillment.into(),
        };
        self.db.lock().unwrap().execute("INSERT INTO products(id,name,description,fulfillment,created_at) VALUES(?1,?2,?3,?4,?5)",params![p.id,p.name,p.description,p.fulfillment,now()])?;
        Ok(p)
    }
    pub fn create_price(
        &self,
        product_id: &str,
        provider: &str,
        currency: &str,
        unit_amount: i64,
        provider_price_id: Option<&str>,
    ) -> Result<Price> {
        anyhow::ensure!(unit_amount > 0, "unit amount must be positive");
        anyhow::ensure!(
            valid_currency(currency),
            "currency must be uppercase ISO-4217 style code"
        );
        let exists: bool = self.db.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM products WHERE id=?1)",
            params![product_id],
            |r| r.get(0),
        )?;
        anyhow::ensure!(exists, "product not found");
        let p = Price {
            id: random_id("price"),
            product_id: product_id.into(),
            provider: provider.trim().to_ascii_lowercase(),
            currency: currency.into(),
            unit_amount,
            provider_price_id: provider_price_id.map(str::to_string),
        };
        anyhow::ensure!(!p.provider.is_empty(), "provider is required");
        self.db.lock().unwrap().execute("INSERT INTO prices(id,product_id,provider,currency,unit_amount,provider_price_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![p.id,p.product_id,p.provider,p.currency,p.unit_amount,p.provider_price_id,now()])?;
        Ok(p)
    }
    pub fn product(&self, id: &str) -> Result<Product> {
        self.db
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,name,description,fulfillment FROM products WHERE id=?1",
                params![id],
                |r| {
                    Ok(Product {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        description: r.get(2)?,
                        fulfillment: r.get(3)?,
                    })
                },
            )
            .optional()?
            .context("product not found")
    }

    pub fn checkout_session(&self, id: &str) -> Result<CheckoutSession> {
        let db = self.db.lock().unwrap();
        let mut session = db.query_row(
            "SELECT id,price_id,email,provider,status,success_url,cancel_url,metadata_json,expires_at FROM checkout_sessions WHERE id=?1",
            params![id],
            |r| {
                let raw: String = r.get(7)?;
                let metadata = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
                Ok(CheckoutSession {
                    id: r.get(0)?,
                    price_id: r.get(1)?,
                    email: r.get(2)?,
                    provider: r.get(3)?,
                    status: r.get(4)?,
                    success_url: r.get(5)?,
                    cancel_url: r.get(6)?,
                    metadata,
                    expires_at: r.get(8)?,
                })
            },
        ).optional()?.context("checkout session not found")?;
        if session.status == "open" && session.expires_at.is_some_and(|expires| expires <= now()) {
            db.execute(
                "UPDATE checkout_sessions SET status='expired' WHERE id=?1 AND status='open'",
                params![id],
            )?;
            session.status = "expired".into();
        }
        Ok(session)
    }

    pub fn price(&self, id: &str) -> Result<Price> {
        self.db.lock().unwrap().query_row("SELECT id,product_id,provider,currency,unit_amount,provider_price_id FROM prices WHERE id=?1 AND active=1",params![id],|r|Ok(Price{id:r.get(0)?,product_id:r.get(1)?,provider:r.get(2)?,currency:r.get(3)?,unit_amount:r.get(4)?,provider_price_id:r.get(5)?})).optional()?.context("price not found")
    }
    pub fn create_checkout_session(
        &self,
        price_id: &str,
        email: &str,
        success_url: Option<&str>,
        cancel_url: Option<&str>,
        metadata: serde_json::Value,
    ) -> Result<CheckoutSession> {
        self.create_checkout_session_with_ttl(
            price_id,
            email,
            success_url,
            cancel_url,
            metadata,
            DEFAULT_CHECKOUT_TTL_SECONDS,
        )
    }

    pub fn create_checkout_session_with_ttl(
        &self,
        price_id: &str,
        email: &str,
        success_url: Option<&str>,
        cancel_url: Option<&str>,
        metadata: serde_json::Value,
        ttl_seconds: i64,
    ) -> Result<CheckoutSession> {
        anyhow::ensure!(email.trim().contains('@'), "valid email is required");
        anyhow::ensure!(
            (60..=86_400).contains(&ttl_seconds),
            "checkout TTL must be 60..=86400 seconds"
        );
        let price = self.price(price_id)?;
        let expires_at = now() + ttl_seconds;
        let s = CheckoutSession {
            id: random_id("cs"),
            price_id: price.id,
            email: email.trim().to_ascii_lowercase(),
            provider: price.provider,
            status: "open".into(),
            success_url: success_url.map(str::to_string),
            cancel_url: cancel_url.map(str::to_string),
            metadata,
            expires_at: Some(expires_at),
        };
        let metadata_json = serde_json::to_string(&s.metadata)?;
        self.db.lock().unwrap().execute(
            "INSERT INTO checkout_sessions(id,price_id,email,provider,status,success_url,cancel_url,metadata_json,created_at,expires_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![s.id,s.price_id,s.email,s.provider,s.status,s.success_url,s.cancel_url,metadata_json,now(),expires_at],
        )?;
        Ok(s)
    }

    pub fn register_webhook(
        &self,
        url: &str,
        secret: Option<&str>,
    ) -> Result<MerchantWebhookEndpoint> {
        let url = url.trim();
        anyhow::ensure!(
            url.starts_with("https://")
                || url.starts_with("http://127.0.0.1")
                || url.starts_with("http://localhost"),
            "webhook URL must use HTTPS (localhost HTTP allowed for development)"
        );
        let endpoint = MerchantWebhookEndpoint {
            id: random_id("wh"),
            url: url.into(),
            secret: secret
                .map(str::to_string)
                .unwrap_or_else(crate::webhook::new_secret),
            active: true,
        };
        self.db.lock().unwrap().execute(
            "INSERT INTO merchant_webhook_endpoints(id,url,secret,created_at) VALUES(?1,?2,?3,?4)",
            params![endpoint.id, endpoint.url, endpoint.secret, now()],
        )?;
        Ok(endpoint)
    }

    pub fn list_webhooks(&self) -> Result<Vec<MerchantWebhookEndpoint>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id,url,secret,active FROM merchant_webhook_endpoints ORDER BY created_at,id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(MerchantWebhookEndpoint {
                id: r.get(0)?,
                url: r.get(1)?,
                secret: r.get(2)?,
                active: r.get::<_, i64>(3)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_webhook_active(&self, id: &str, active: bool) -> Result<MerchantWebhookEndpoint> {
        let db = self.db.lock().unwrap();
        let changed = db.execute(
            "UPDATE merchant_webhook_endpoints SET active=?1 WHERE id=?2",
            params![if active { 1 } else { 0 }, id],
        )?;
        anyhow::ensure!(changed == 1, "webhook endpoint not found");
        db.query_row(
            "SELECT id,url,secret,active FROM merchant_webhook_endpoints WHERE id=?1",
            params![id],
            |r| {
                Ok(MerchantWebhookEndpoint {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    secret: r.get(2)?,
                    active: r.get::<_, i64>(3)? != 0,
                })
            },
        )
        .map_err(Into::into)
    }

    pub fn recent_webhook_deliveries(&self, limit: u32) -> Result<Vec<WebhookDeliveryStatus>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id,event_id,endpoint_id,status,attempts,last_error,delivered_at FROM webhook_deliveries ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit.clamp(1, 500)], |r| {
            Ok(WebhookDeliveryStatus {
                id: r.get(0)?,
                event_id: r.get(1)?,
                endpoint_id: r.get(2)?,
                status: r.get(3)?,
                attempts: r.get(4)?,
                last_error: r.get(5)?,
                delivered_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn redeliver_webhook(&self, id: &str) -> Result<WebhookDeliveryStatus> {
        let db = self.db.lock().unwrap();
        let changed = db.execute(
            "UPDATE webhook_deliveries SET status='pending',next_attempt_at=?1,last_error=NULL,delivered_at=NULL WHERE id=?2",
            params![now(), id],
        )?;
        anyhow::ensure!(changed == 1, "webhook delivery not found");
        db.query_row(
            "SELECT id,event_id,endpoint_id,status,attempts,last_error,delivered_at FROM webhook_deliveries WHERE id=?1",
            params![id],
            |r| Ok(WebhookDeliveryStatus { id:r.get(0)?,event_id:r.get(1)?,endpoint_id:r.get(2)?,status:r.get(3)?,attempts:r.get(4)?,last_error:r.get(5)?,delivered_at:r.get(6)? }),
        ).map_err(Into::into)
    }

    pub fn pending_webhook_deliveries(&self, limit: u32) -> Result<Vec<PendingWebhookDelivery>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT d.id,d.event_id,d.endpoint_id,e.url,e.secret,c.event_type,c.payload_json,d.attempts              FROM webhook_deliveries d              JOIN merchant_webhook_endpoints e ON e.id=d.endpoint_id              JOIN commerce_events c ON c.id=d.event_id              WHERE d.status='pending' AND e.active=1 AND d.next_attempt_at<=?1              ORDER BY d.created_at LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![now(), limit], |r| {
            Ok(PendingWebhookDelivery {
                id: r.get(0)?,
                event_id: r.get(1)?,
                endpoint_id: r.get(2)?,
                url: r.get(3)?,
                secret: r.get(4)?,
                event_type: r.get(5)?,
                payload: r.get(6)?,
                attempts: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn mark_webhook_delivered(&self, id: &str) -> Result<()> {
        self.db.lock().unwrap().execute(
            "UPDATE webhook_deliveries SET status='delivered',delivered_at=?1,attempts=attempts+1,last_error=NULL WHERE id=?2",
            params![now(), id],
        )?;
        Ok(())
    }

    pub fn mark_webhook_retry(&self, id: &str, error: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        let attempts: u32 = db.query_row(
            "SELECT attempts FROM webhook_deliveries WHERE id=?1",
            params![id],
            |r| r.get(0),
        )?;
        let delay = 2_i64.pow((attempts + 1).min(8)) * 5;
        db.execute(
            "UPDATE webhook_deliveries SET attempts=attempts+1,next_attempt_at=?1,last_error=?2 WHERE id=?3",
            params![now() + delay, error, id],
        )?;
        Ok(())
    }

    pub fn transition_order(
        &self,
        provider: &str,
        provider_order_id: &str,
        status: &str,
        event_type: &str,
    ) -> Result<(CanonicalOrder, bool, CommerceEvent)> {
        anyhow::ensure!(
            matches!(status, "refunded" | "chargeback"),
            "unsupported order status transition"
        );
        anyhow::ensure!(
            matches!(event_type, "order.refunded" | "order.chargeback"),
            "unsupported commerce event type"
        );
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let mut order:CanonicalOrder=tx.query_row(
            "SELECT id,provider,provider_order_id,checkout_session_id,price_id,email,currency,amount,status FROM commerce_orders WHERE provider=?1 AND provider_order_id=?2",
            params![provider,provider_order_id],
            |r| Ok(CanonicalOrder{id:r.get(0)?,provider:r.get(1)?,provider_order_id:r.get(2)?,checkout_session_id:r.get(3)?,price_id:r.get(4)?,email:r.get(5)?,currency:r.get(6)?,amount:r.get(7)?,status:r.get(8)?})
        ).context("commerce order not found")?;
        let changed = order.status != status;
        if changed {
            tx.execute(
                "UPDATE commerce_orders SET status=?1 WHERE id=?2",
                params![status, order.id],
            )?;
            order.status = status.into();
        }
        let event_id = format!("evt_{}_{}", event_type.replace('.', "_"), order.id);
        let created_at = now();
        let payload = serde_json::to_string(
            &serde_json::json!({"id":event_id,"type":event_type,"createdAt":created_at,"data":{"order":&order}}),
        )?;
        tx.execute("INSERT OR IGNORE INTO commerce_events(id,event_type,order_id,payload_json,created_at) VALUES(?1,?2,?3,?4,?5)",params![event_id,event_type,order.id,payload,created_at])?;
        tx.execute("INSERT OR IGNORE INTO webhook_deliveries(id,event_id,endpoint_id,next_attempt_at,created_at) SELECT 'del_'||lower(hex(randomblob(16))),?1,id,?2,?2 FROM merchant_webhook_endpoints WHERE active=1",params![event_id,created_at])?;
        tx.commit()?;
        let canonical_order_id = order.id.clone();
        Ok((
            order,
            changed,
            CommerceEvent {
                id: event_id,
                event_type: event_type.into(),
                order_id: canonical_order_id,
                created_at,
            },
        ))
    }

    pub fn record_paid_order(
        &self,
        provider: &str,
        provider_order_id: &str,
        session_id: &str,
    ) -> Result<(CanonicalOrder, bool, CommerceEvent)> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        let session: (String, String, String, String, Option<i64>) = tx
            .query_row(
                "SELECT price_id,email,status,metadata_json,expires_at FROM checkout_sessions WHERE id=?1",
                params![session_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .context("checkout session not found")?;
        let price:Price=tx.query_row("SELECT id,product_id,provider,currency,unit_amount,provider_price_id FROM prices WHERE id=?1",params![session.0],|r|Ok(Price{id:r.get(0)?,product_id:r.get(1)?,provider:r.get(2)?,currency:r.get(3)?,unit_amount:r.get(4)?,provider_price_id:r.get(5)?}))?;
        anyhow::ensure!(price.provider == provider, "checkout provider mismatch");
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM commerce_orders WHERE provider=?1 AND provider_order_id=?2",
                params![provider, provider_order_id],
                |r| r.get(0),
            )
            .optional()?;
        let inserted = existing.is_none();
        let order_id = existing.unwrap_or_else(|| random_id("ord"));
        if inserted {
            anyhow::ensure!(session.2 == "open", "checkout session is not open");
            anyhow::ensure!(
                session.4.is_none_or(|expires| expires > now()),
                "checkout session expired"
            );
            tx.execute("INSERT INTO commerce_orders(id,provider,provider_order_id,checkout_session_id,price_id,email,currency,amount,status,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'paid',?9)",params![order_id,provider,provider_order_id,session_id,price.id,session.1,price.currency,price.unit_amount,now()])?;
            tx.execute(
                "UPDATE checkout_sessions SET status='completed' WHERE id=?1",
                params![session_id],
            )?;
        }
        let event_id = format!("evt_paid_{order_id}");
        let created_at = now();
        let order = CanonicalOrder {
            id: order_id.clone(),
            provider: provider.into(),
            provider_order_id: provider_order_id.into(),
            checkout_session_id: Some(session_id.into()),
            price_id: price.id,
            email: session.1,
            currency: price.currency,
            amount: price.unit_amount,
            status: "paid".into(),
        };
        let checkout_metadata: serde_json::Value = serde_json::from_str(&session.3)?;
        let payload = serde_json::to_string(&serde_json::json!({
            "id": event_id,
            "type": "order.paid",
            "createdAt": created_at,
            "data": { "order": &order, "checkoutMetadata": checkout_metadata }
        }))?;
        tx.execute(
            "INSERT OR IGNORE INTO commerce_events(id,event_type,order_id,payload_json,created_at) VALUES(?1,'order.paid',?2,?3,?4)",
            params![event_id, order_id, payload, created_at],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO webhook_deliveries(id,event_id,endpoint_id,next_attempt_at,created_at)              SELECT 'del_'||lower(hex(randomblob(16))),?1,id,?2,?2 FROM merchant_webhook_endpoints WHERE active=1",
            params![event_id, created_at],
        )?;
        tx.commit()?;
        Ok((
            order,
            inserted,
            CommerceEvent {
                id: event_id,
                event_type: "order.paid".into(),
                order_id,
                created_at: now(),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn product_price_checkout_paid_order_round_trip() {
        let s = CommerceStore::memory().unwrap();
        let endpoint = s
            .register_webhook(
                "https://shop.example/webhooks/openindie",
                Some("test-secret"),
            )
            .unwrap();
        assert_eq!(endpoint.url, "https://shop.example/webhooks/openindie");
        let p = s
            .create_product("Report", "PDF report", "download")
            .unwrap();
        let price = s
            .create_price(&p.id, "paddle", "USD", 1900, Some("pri_1"))
            .unwrap();
        let cs = s
            .create_checkout_session(
                &price.id,
                "Buyer@Example.com",
                Some("https://shop.test/ok"),
                None,
                serde_json::json!({"sku":"report-2026"}),
            )
            .unwrap();
        assert_eq!(cs.provider, "paddle");
        let (o, inserted, e) = s.record_paid_order("paddle", "txn_1", &cs.id).unwrap();
        assert!(inserted);
        assert_eq!(o.amount, 1900);
        assert_eq!(o.email, "buyer@example.com");
        assert_eq!(e.event_type, "order.paid");
        let pending = s.pending_webhook_deliveries(10).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].payload.contains("order.paid"));
        assert!(pending[0].payload.contains("report-2026"));
        s.mark_webhook_delivered(&pending[0].id).unwrap();
        assert!(s.pending_webhook_deliveries(10).unwrap().is_empty());
        let (_, inserted2, _) = s.record_paid_order("paddle", "txn_1", &cs.id).unwrap();
        assert!(!inserted2);
    }
    #[test]
    fn refund_transition_is_idempotent_and_queues_event() {
        let s = CommerceStore::memory().unwrap();
        s.register_webhook("https://shop.example/hooks", Some("secret"))
            .unwrap();
        let p = s.create_product("Report", "", "download").unwrap();
        let price = s
            .create_price(&p.id, "paddle", "USD", 1900, Some("pri_1"))
            .unwrap();
        let cs = s
            .create_checkout_session(&price.id, "a@b.com", None, None, serde_json::json!({}))
            .unwrap();
        s.record_paid_order("paddle", "txn_refund", &cs.id).unwrap();
        let (order, changed, event) = s
            .transition_order("paddle", "txn_refund", "refunded", "order.refunded")
            .unwrap();
        assert!(changed);
        assert_eq!(order.status, "refunded");
        assert_eq!(event.event_type, "order.refunded");
        let (_, changed_again, _) = s
            .transition_order("paddle", "txn_refund", "refunded", "order.refunded")
            .unwrap();
        assert!(!changed_again);
        assert_eq!(s.pending_webhook_deliveries(10).unwrap().len(), 2);
    }

    #[test]
    fn webhook_management_and_redelivery_are_recoverable() {
        let s = CommerceStore::memory().unwrap();
        let endpoint = s
            .register_webhook("https://shop.example/hooks", Some("secret"))
            .unwrap();
        assert_eq!(s.list_webhooks().unwrap().len(), 1);
        assert!(!s.set_webhook_active(&endpoint.id, false).unwrap().active);
        let p = s.create_product("Report", "", "download").unwrap();
        let price = s
            .create_price(&p.id, "paddle", "USD", 1900, Some("pri_1"))
            .unwrap();
        let cs = s
            .create_checkout_session(&price.id, "a@b.com", None, None, serde_json::json!({}))
            .unwrap();
        s.record_paid_order("paddle", "txn_disabled", &cs.id)
            .unwrap();
        assert!(s.pending_webhook_deliveries(10).unwrap().is_empty());

        assert!(s.set_webhook_active(&endpoint.id, true).unwrap().active);
        let cs2 = s
            .create_checkout_session(&price.id, "a@b.com", None, None, serde_json::json!({}))
            .unwrap();
        s.record_paid_order("paddle", "txn_enabled", &cs2.id)
            .unwrap();
        let pending = s.pending_webhook_deliveries(10).unwrap();
        assert_eq!(pending.len(), 1);
        s.mark_webhook_delivered(&pending[0].id).unwrap();
        let recent = s.recent_webhook_deliveries(10).unwrap();
        assert_eq!(recent[0].status, "delivered");
        let replay = s.redeliver_webhook(&pending[0].id).unwrap();
        assert_eq!(replay.status, "pending");
        assert_eq!(s.pending_webhook_deliveries(10).unwrap().len(), 1);
    }

    #[test]
    fn checkout_expiry_blocks_new_payment_but_not_idempotent_retry() {
        let s = CommerceStore::memory().unwrap();
        let p = s.create_product("Report", "", "download").unwrap();
        let price = s
            .create_price(&p.id, "paddle", "USD", 1900, Some("pri_1"))
            .unwrap();
        let expired = s
            .create_checkout_session_with_ttl(
                &price.id,
                "a@b.com",
                None,
                None,
                serde_json::json!({}),
                60,
            )
            .unwrap();
        s.db.lock()
            .unwrap()
            .execute(
                "UPDATE checkout_sessions SET expires_at=?1 WHERE id=?2",
                params![now() - 1, expired.id],
            )
            .unwrap();
        assert_eq!(s.checkout_session(&expired.id).unwrap().status, "expired");
        assert!(
            s.record_paid_order("paddle", "txn_expired", &expired.id)
                .is_err()
        );

        let paid = s
            .create_checkout_session(&price.id, "a@b.com", None, None, serde_json::json!({}))
            .unwrap();
        assert!(
            s.record_paid_order("paddle", "txn_paid_retry", &paid.id)
                .unwrap()
                .1
        );
        s.db.lock()
            .unwrap()
            .execute(
                "UPDATE checkout_sessions SET expires_at=?1 WHERE id=?2",
                params![now() - 1, paid.id],
            )
            .unwrap();
        let (_, inserted_again, _) = s
            .record_paid_order("paddle", "txn_paid_retry", &paid.id)
            .unwrap();
        assert!(!inserted_again);
    }

    #[test]
    fn rejects_invalid_money_and_provider_mismatch() {
        let s = CommerceStore::memory().unwrap();
        let p = s.create_product("X", "", "none").unwrap();
        assert!(s.create_price(&p.id, "paddle", "usd", 100, None).is_err());
        let price = s.create_price(&p.id, "paddle", "USD", 100, None).unwrap();
        let cs = s
            .create_checkout_session(&price.id, "a@b.com", None, None, serde_json::json!({}))
            .unwrap();
        assert!(s.record_paid_order("zpay", "z1", &cs.id).is_err());
    }
}
