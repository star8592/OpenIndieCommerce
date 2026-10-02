use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, patch, post},
};
use openindiecommerce_server::{
    ActivateRequest, ClaimRequest, DEFAULT_MAX_ACTIVATIONS, InstanceRequest, LicenseStore,
    commerce::{
        CheckoutSession, CommerceStore, DEFAULT_CHECKOUT_TTL_SECONDS, MerchantWebhookEndpoint,
        Price, Product, WebhookDeliveryStatus,
    },
    paddle::{self, PaddleEvent},
    zpay,
};
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    store: LicenseStore,
    commerce: CommerceStore,
    admin_token: Arc<String>,
    entitlement_key_secret: Option<Arc<Vec<u8>>>,
    paddle_webhook_secret: Option<Arc<String>>,
    paddle_client_token: Option<Arc<String>>,
    paddle_signature_tolerance_seconds: u64,
    zpay_pid: Option<Arc<String>>,
    zpay_key: Option<Arc<String>>,
    zpay_submit_url: Arc<String>,
    zpay_public_base_url: Option<Arc<String>>,
    public_rate_limiter: RateLimiter,
}

#[derive(Clone, Default)]
struct RateLimiter {
    windows: Arc<Mutex<HashMap<String, (u64, u32)>>>,
}

impl RateLimiter {
    fn allow_at(&self, key: &str, limit: u32, window_seconds: u64, now: u64) -> bool {
        let bucket = now / window_seconds.max(1);
        let mut windows = self.windows.lock().unwrap();
        if windows.len() > 10_000 {
            windows.retain(|_, (seen_bucket, _)| *seen_bucket >= bucket.saturating_sub(1));
        }
        let entry = windows.entry(key.to_string()).or_insert((bucket, 0));
        if entry.0 != bucket {
            *entry = (bucket, 0);
        }
        if entry.1 >= limit {
            return false;
        }
        entry.1 += 1;
        true
    }

    fn allow(&self, key: &str, limit: u32, window_seconds: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.allow_at(key, limit, window_seconds, now)
    }
}

fn client_identity(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
        })
        .filter(|v| !v.is_empty())
        .unwrap_or("direct")
        .to_string()
}

fn enforce_public_rate_limit(
    state: &AppState,
    headers: &HeaderMap,
    scope: &str,
    limit: u32,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let key = format!("{scope}:{}", client_identity(headers));
    if state.public_rate_limiter.allow(&key, limit, 60) {
        Ok(())
    } else {
        Err(api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded; try again shortly",
        ))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProductRequest {
    name: String,
    description: Option<String>,
    fulfillment: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePriceRequest {
    product_id: String,
    provider: String,
    currency: String,
    unit_amount: i64,
    provider_price_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateCheckoutSessionRequest {
    price_id: String,
    email: String,
    success_url: Option<String>,
    cancel_url: Option<String>,
    metadata: Option<serde_json::Value>,
    expires_in_seconds: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckoutSessionResponse {
    session: CheckoutSession,
    price: Price,
    product: Product,
    checkout_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateMerchantWebhookRequest {
    url: String,
    secret: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MerchantWebhookRegistration {
    id: String,
    url: String,
    secret: String,
    active: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMerchantWebhookRequest {
    active: bool,
}

#[derive(Debug, Deserialize)]
struct ListDeliveriesQuery {
    limit: Option<u32>,
}

#[derive(Debug, Serialize)]
struct Health {
    ok: bool,
}

#[derive(Debug, Clone)]
struct PublicSiteConfig {
    brand_name: String,
    seller_legal_name: String,
    support_email: String,
    support_phone: String,
    refund_days: u32,
}

fn public_site_config() -> Result<PublicSiteConfig, String> {
    let required = |name: &str| {
        env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("{name} is not configured"))
    };
    let refund_days = env::var("OIC_REFUND_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(14);
    Ok(PublicSiteConfig {
        brand_name: env::var("OIC_BRAND_NAME")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "Merchant Store".to_string()),
        seller_legal_name: required("OIC_SELLER_LEGAL_NAME")?,
        support_email: required("OIC_SUPPORT_EMAIL")?,
        support_phone: required("OIC_SUPPORT_PHONE")?,
        refund_days,
    })
}

fn page_shell(title: &str, body: &str, config: &PublicSiteConfig) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{title} · {brand}</title><style>body{{font:16px/1.65 system-ui;max-width:900px;margin:auto;padding:32px 22px;background:#0b1016;color:#e8eef5}}a{{color:#6ee7b7}}nav{{display:flex;gap:18px;flex-wrap:wrap;margin-bottom:36px}}h1{{font-size:42px;line-height:1.05}}h2{{margin-top:34px}}.card{{padding:22px;border:1px solid #263544;border-radius:12px;background:#101821}}.muted{{color:#91a0af}}footer{{margin-top:48px;padding-top:20px;border-top:1px solid #263544;color:#91a0af}}</style></head><body><nav><a href="/">{brand}</a><a href="/terms">Terms</a><a href="/privacy">Privacy</a><a href="/refund">Refunds</a></nav>{body}<footer>{brand} · commerce powered by OpenIndieCommerce</footer></body></html>"#,
        brand = html_escape(&config.brand_name)
    )
}

fn landing_page(config: &PublicSiteConfig) -> String {
    page_shell(
        &config.brand_name,
        &format!(
            r#"<h1>{brand}</h1><p class="muted">Secure checkout and digital fulfillment.</p><div class="card"><h2>Purchases</h2><p>Products, prices, currencies and fulfillment are shown on the checkout link supplied by the merchant.</p><p>Payment credentials are handled by the configured payment provider; this site does not store card details.</p></div><h2>Support</h2><p>Email: <a href="mailto:{email}">{email}</a><br>Phone: {phone}</p><p class="muted">Seller: {seller}</p>"#,
            brand = html_escape(&config.brand_name),
            email = html_escape(&config.support_email),
            phone = html_escape(&config.support_phone),
            seller = html_escape(&config.seller_legal_name)
        ),
        config,
    )
}

fn terms_page(config: &PublicSiteConfig) -> String {
    page_shell(
        "Terms",
        &format!(
            r#"<h1>Terms and Conditions</h1><p>These terms apply to purchases from <strong>{brand}</strong>, sold by <strong>{seller}</strong>.</p><h2>Products and fulfillment</h2><p>The product, price, currency, delivery method and any license or entitlement terms are described on the applicable checkout page or product offer.</p><h2>Payments</h2><p>Payment providers process payment credentials and may act as Merchant of Record where stated at checkout. {brand} does not store card details.</p><h2>Use</h2><p>Customers must use purchased products and services lawfully and comply with any product-specific license terms presented at purchase.</p><h2>Support</h2><p>Contact <a href="mailto:{email}">{email}</a> or {phone}.</p><p>By completing a purchase, you agree to these terms and the linked Refund and Privacy Policies.</p>"#,
            brand = html_escape(&config.brand_name),
            seller = html_escape(&config.seller_legal_name),
            email = html_escape(&config.support_email),
            phone = html_escape(&config.support_phone)
        ),
        config,
    )
}

fn privacy_page(config: &PublicSiteConfig) -> String {
    page_shell(
        "Privacy",
        &format!(
            r#"<h1>Privacy Policy</h1><h2>Commerce data</h2><p>To create and fulfill orders we may process purchase email, checkout metadata, payment-provider order identifiers, product and price identifiers, timestamps and operational security logs.</p><h2>Entitlements</h2><p>If a product uses optional entitlement or software-license features, activation identifiers and license-key hashes may also be processed. Plaintext payment credentials are handled by payment providers, not by this service.</p><h2>Retention and security</h2><p>Records are retained as needed to fulfill purchases, support refunds, prevent abuse and maintain entitlements.</p><h2>Contact</h2><p>Privacy requests: <a href="mailto:{email}">{email}</a>. Seller: {seller}. Phone: {phone}.</p>"#,
            email = html_escape(&config.support_email),
            seller = html_escape(&config.seller_legal_name),
            phone = html_escape(&config.support_phone)
        ),
        config,
    )
}

fn refund_page(config: &PublicSiteConfig) -> String {
    page_shell(
        "Refund Policy",
        &format!(
            r#"<h1>Refund Policy</h1><p>You may request a refund within <strong>{days} days</strong> of purchase by contacting <a href="mailto:{email}">{email}</a>, unless a product-specific policy or applicable consumer law provides different rights.</p><p>Approved refunds may revoke associated downloads, memberships, entitlements or software licenses. Payment providers process the monetary refund according to the checkout rail used for the order.</p><p>Seller: {seller}. Support phone: {phone}.</p>"#,
            days = config.refund_days,
            email = html_escape(&config.support_email),
            seller = html_escape(&config.seller_legal_name),
            phone = html_escape(&config.support_phone)
        ),
        config,
    )
}

async fn site_page(kind: &'static str) -> Result<Html<String>, (StatusCode, Json<ApiError>)> {
    let config = public_site_config().map_err(|e| api_error(StatusCode::SERVICE_UNAVAILABLE, e))?;
    let html = match kind {
        "home" => landing_page(&config),
        "terms" => terms_page(&config),
        "privacy" => privacy_page(&config),
        "refund" => refund_page(&config),
        _ => unreachable!(),
    };
    Ok(Html(html))
}
async fn home() -> Result<Html<String>, (StatusCode, Json<ApiError>)> {
    site_page("home").await
}
async fn terms() -> Result<Html<String>, (StatusCode, Json<ApiError>)> {
    site_page("terms").await
}
async fn privacy() -> Result<Html<String>, (StatusCode, Json<ApiError>)> {
    site_page("privacy").await
}
async fn refund() -> Result<Html<String>, (StatusCode, Json<ApiError>)> {
    site_page("refund").await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueRequest {
    email: String,
    max_activations: Option<u32>,
    order_provider: Option<String>,
    order_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevokeRequest {
    order_provider: String,
    order_id: String,
}

#[derive(Debug, Serialize)]
struct RevokeResponse {
    revoked: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaddleWebhookResponse {
    ok: bool,
    action: &'static str,
    transaction_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

fn secret_value(name: &str) -> anyhow::Result<String> {
    let file_name = format!("{name}_FILE");
    if let Ok(path) = env::var(&file_name)
        && !path.trim().is_empty()
    {
        let value = fs::read_to_string(path.trim())?;
        let value = value.trim().to_string();
        anyhow::ensure!(
            !value.is_empty(),
            "{file_name} points to an empty secret file"
        );
        return Ok(value);
    }
    let value = env::var(name).map_err(|_| anyhow::anyhow!("{name} or {file_name} is required"))?;
    let value = value.trim().to_string();
    anyhow::ensure!(!value.is_empty(), "{name} is empty");
    Ok(value)
}

fn optional_secret_value(name: &str) -> anyhow::Result<Option<String>> {
    let file_name = format!("{name}_FILE");
    if env::var_os(&file_name).is_some() || env::var_os(name).is_some() {
        return secret_value(name).map(Some);
    }
    Ok(None)
}

fn api_error(status: StatusCode, error: impl ToString) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.to_string(),
        }),
    )
}

fn require_admin(headers: &HeaderMap, expected: &str) -> Result<(), (StatusCode, Json<ApiError>)> {
    let supplied = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if supplied == Some(expected) {
        Ok(())
    } else {
        Err(api_error(StatusCode::UNAUTHORIZED, "invalid admin token"))
    }
}

async fn zpay_webhook(
    State(state): State<AppState>,
    Query(params): Query<BTreeMap<String, String>>,
) -> Result<&'static str, (StatusCode, String)> {
    let pid = state.zpay_pid.as_deref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "ZPAY not configured".to_string(),
    ))?;
    let key = state.zpay_key.as_deref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "ZPAY not configured".to_string(),
    ))?;
    zpay::process_commerce_notification(&state.commerce,pid,key,&params)
        .map(|outcome| { tracing::info!(order_id=%outcome.order.id, inserted=outcome.inserted, "processed canonical ZPAY commerce notification"); "success" })
        .map_err(|error|(StatusCode::BAD_REQUEST,error.to_string()))
}

async fn openapi_spec() -> Json<serde_json::Value> {
    Json(
        serde_json::from_str(include_str!("../../../docs/openapi.json"))
            .expect("valid embedded OpenAPI document"),
    )
}

async fn health(
    State(state): State<AppState>,
) -> Result<Json<Health>, (StatusCode, Json<ApiError>)> {
    state
        .store
        .health_check()
        .map(|_| Json(Health { ok: true }))
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error))
}

async fn create_merchant_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateMerchantWebhookRequest>,
) -> ApiResult<MerchantWebhookRegistration> {
    require_admin(&headers, &state.admin_token)?;
    let MerchantWebhookEndpoint {
        id,
        url,
        secret,
        active,
    } = state
        .commerce
        .register_webhook(&request.url, request.secret.as_deref())
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(MerchantWebhookRegistration {
        id,
        url,
        secret,
        active,
    }))
}

async fn list_merchant_webhooks(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Vec<MerchantWebhookEndpoint>> {
    require_admin(&headers, &state.admin_token)?;
    state
        .commerce
        .list_webhooks()
        .map(Json)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
}

async fn update_merchant_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<UpdateMerchantWebhookRequest>,
) -> ApiResult<MerchantWebhookEndpoint> {
    require_admin(&headers, &state.admin_token)?;
    state
        .commerce
        .set_webhook_active(&id, request.active)
        .map(Json)
        .map_err(|e| api_error(StatusCode::NOT_FOUND, e))
}

async fn list_webhook_deliveries(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListDeliveriesQuery>,
) -> ApiResult<Vec<WebhookDeliveryStatus>> {
    require_admin(&headers, &state.admin_token)?;
    state
        .commerce
        .recent_webhook_deliveries(query.limit.unwrap_or(100))
        .map(Json)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
}

async fn redeliver_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<WebhookDeliveryStatus> {
    require_admin(&headers, &state.admin_token)?;
    state
        .commerce
        .redeliver_webhook(&id)
        .map(Json)
        .map_err(|e| api_error(StatusCode::NOT_FOUND, e))
}

async fn dispatch_webhooks_once(store: &CommerceStore, client: &reqwest::Client) -> usize {
    let Ok(deliveries) = store.pending_webhook_deliveries(50) else {
        return 0;
    };
    let count = deliveries.len();
    for delivery in deliveries {
        match openindiecommerce_server::webhook::deliver(
            client,
            &delivery.url,
            &delivery.secret,
            &delivery.event_id,
            &delivery.event_type,
            delivery.payload.as_bytes(),
        )
        .await
        {
            Ok(status) if status.is_success() => {
                let _ = store.mark_webhook_delivered(&delivery.id);
            }
            Ok(status) => {
                let _ = store.mark_webhook_retry(&delivery.id, &format!("HTTP {status}"));
            }
            Err(error) => {
                let _ = store.mark_webhook_retry(&delivery.id, &error.to_string());
            }
        }
    }
    count
}

async fn webhook_dispatch_loop(store: CommerceStore) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("OpenIndieCommerce/0.1")
        .build()
        .expect("webhook HTTP client");
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        tick.tick().await;
        let delivered = dispatch_webhooks_once(&store, &client).await;
        if delivered > 0 {
            tracing::debug!(count = delivered, "processed merchant webhook outbox");
        }
    }
}

async fn create_product(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateProductRequest>,
) -> ApiResult<Product> {
    require_admin(&headers, &state.admin_token)?;
    state
        .commerce
        .create_product(
            &request.name,
            request.description.as_deref().unwrap_or(""),
            &request.fulfillment,
        )
        .map(Json)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
}

async fn create_price(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreatePriceRequest>,
) -> ApiResult<Price> {
    require_admin(&headers, &state.admin_token)?;
    state
        .commerce
        .create_price(
            &request.product_id,
            &request.provider,
            &request.currency,
            request.unit_amount,
            request.provider_price_id.as_deref(),
        )
        .map(Json)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
}

async fn create_checkout_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateCheckoutSessionRequest>,
) -> ApiResult<CheckoutSessionResponse> {
    enforce_public_rate_limit(&state, &headers, "checkout", 60)?;
    let session = state
        .commerce
        .create_checkout_session_with_ttl(
            &request.price_id,
            &request.email,
            request.success_url.as_deref(),
            request.cancel_url.as_deref(),
            request.metadata.unwrap_or_else(|| serde_json::json!({})),
            request
                .expires_in_seconds
                .unwrap_or(DEFAULT_CHECKOUT_TTL_SECONDS),
        )
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    let price = state
        .commerce
        .price(&session.price_id)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    let product = state
        .commerce
        .product(&price.product_id)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    let checkout_url = format!("/checkout/{}", session.id);
    Ok(Json(CheckoutSessionResponse {
        session,
        price,
        product,
        checkout_url,
    }))
}

async fn get_checkout_session(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<CheckoutSessionResponse> {
    let session = state
        .commerce
        .checkout_session(&id)
        .map_err(|e| api_error(StatusCode::NOT_FOUND, e))?;
    let price = state
        .commerce
        .price(&session.price_id)
        .map_err(|e| api_error(StatusCode::NOT_FOUND, e))?;
    let product = state
        .commerce
        .product(&price.product_id)
        .map_err(|e| api_error(StatusCode::NOT_FOUND, e))?;
    let checkout_url = format!("/checkout/{}", session.id);
    Ok(Json(CheckoutSessionResponse {
        session,
        price,
        product,
        checkout_url,
    }))
}

fn money_display(currency: &str, amount: i64) -> String {
    if currency == "JPY" {
        format!("{currency} {amount}")
    } else {
        format!("{currency} {:.2}", amount as f64 / 100.0)
    }
}

fn generic_paddle_checkout_html(
    client_token: &str,
    provider_price_id: &str,
    session: &CheckoutSession,
    product: &Product,
    price: &Price,
) -> String {
    let token_json = serde_json::to_string(client_token).unwrap();
    let price_json = serde_json::to_string(provider_price_id).unwrap();
    let session_json = serde_json::to_string(&session.id).unwrap();
    let email_json = serde_json::to_string(&session.email).unwrap();
    let sandbox = client_token.starts_with("test_");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{name}</title><script src="https://cdn.paddle.com/paddle/v2/paddle.js"></script><style>body{{font:16px system-ui;max-width:680px;margin:60px auto;padding:20px}}button{{padding:12px 18px;font:inherit}}</style></head><body><h1>{name}</h1><p>{description}</p><p><strong>{amount}</strong></p><button id="buy">Continue to secure checkout</button><p>Payment details are handled by Paddle.</p><script>const token={token_json},priceId={price_json},sessionId={session_json},email={email_json};if({sandbox})Paddle.Environment.set('sandbox');Paddle.Initialize({{token}});document.getElementById('buy').onclick=()=>Paddle.Checkout.open({{items:[{{priceId,quantity:1}}],customer:{{email}},customData:{{oic_checkout_session_id:sessionId,oic_email:email}},settings:{{displayMode:'overlay'}}}});</script></body></html>"#,
        name = product.name,
        description = product.description,
        amount = money_display(&price.currency, price.unit_amount)
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn generic_zpay_checkout_html(
    submit_url: &str,
    pid: &str,
    key: &str,
    public_base: &str,
    session: &CheckoutSession,
    product: &Product,
    price: &Price,
) -> Result<String, String> {
    if price.currency != "CNY" {
        return Err("ZPAY hosted checkout requires CNY price".into());
    }
    let amount = zpay::format_cny_minor(price.unit_amount).map_err(|e| e.to_string())?;
    let build = |payment_type: &str| {
        let mut fields = BTreeMap::from([
            ("pid".to_string(), pid.to_string()),
            ("type".to_string(), payment_type.to_string()),
            ("out_trade_no".to_string(), session.id.clone()),
            (
                "notify_url".to_string(),
                format!("{}/v1/webhooks/zpay", public_base.trim_end_matches('/')),
            ),
            (
                "return_url".to_string(),
                session.success_url.clone().unwrap_or_else(|| {
                    format!(
                        "{}/checkout/{}",
                        public_base.trim_end_matches('/'),
                        session.id
                    )
                }),
            ),
            ("name".to_string(), product.name.clone()),
            ("money".to_string(), amount.clone()),
            ("sign_type".to_string(), "MD5".to_string()),
        ]);
        fields.insert("sign".into(), zpay::sign(&fields, key));
        fields
    };
    let form = |label: &str, fields: BTreeMap<String, String>| {
        let inputs = fields
            .into_iter()
            .map(|(k, v)| {
                format!(
                    r#"<input type="hidden" name="{}" value="{}">"#,
                    html_escape(&k),
                    html_escape(&v)
                )
            })
            .collect::<String>();
        format!(
            r#"<form method="POST" action="{}">{}<button type="submit">{}</button></form>"#,
            html_escape(submit_url),
            inputs,
            label
        )
    };
    Ok(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><style>body{{font:16px system-ui;max-width:680px;margin:60px auto;padding:20px}}form{{display:inline-block;margin-right:12px}}button{{padding:12px 18px;font:inherit}}</style></head><body><h1>{}</h1><p>{}</p><p><strong>¥{}</strong></p>{}{}</body></html>"#,
        html_escape(&product.name),
        html_escape(&product.name),
        html_escape(&product.description),
        amount,
        form("支付宝", build("alipay")),
        form("微信支付", build("wxpay"))
    ))
}

async fn hosted_checkout(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Html<String>, (StatusCode, Json<ApiError>)> {
    let session = state
        .commerce
        .checkout_session(&id)
        .map_err(|e| api_error(StatusCode::NOT_FOUND, e))?;
    if session.status != "open" {
        return Err(api_error(
            StatusCode::GONE,
            format!("checkout session is {}", session.status),
        ));
    }
    let price = state
        .commerce
        .price(&session.price_id)
        .map_err(|e| api_error(StatusCode::NOT_FOUND, e))?;
    let product = state
        .commerce
        .product(&price.product_id)
        .map_err(|e| api_error(StatusCode::NOT_FOUND, e))?;
    match price.provider.as_str() {
        "paddle" => {
            let token = state.paddle_client_token.as_deref().ok_or_else(|| {
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Paddle client token is not configured",
                )
            })?;
            let provider_price_id = price.provider_price_id.as_deref().ok_or_else(|| {
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Paddle provider price ID is not configured",
                )
            })?;
            Ok(Html(generic_paddle_checkout_html(
                token,
                provider_price_id,
                &session,
                &product,
                &price,
            )))
        }
        "zpay" => {
            let pid = state.zpay_pid.as_deref().ok_or_else(|| {
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "ZPAY pid is not configured",
                )
            })?;
            let key = state.zpay_key.as_deref().ok_or_else(|| {
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "ZPAY key is not configured",
                )
            })?;
            let base = state.zpay_public_base_url.as_deref().ok_or_else(|| {
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "public base URL is not configured",
                )
            })?;
            generic_zpay_checkout_html(
                &state.zpay_submit_url,
                pid,
                key,
                base,
                &session,
                &product,
                &price,
            )
            .map(Html)
            .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
        }
        _ => {
            let body = format!(
                r#"<h1>{}</h1><p>{}</p><div class="card"><p><strong>{}</strong></p><p>Provider adapter <strong>{}</strong> does not yet have a hosted renderer.</p></div>"#,
                product.name,
                product.description,
                money_display(&price.currency, price.unit_amount),
                price.provider
            );
            {
                let config = public_site_config()
                    .map_err(|e| api_error(StatusCode::SERVICE_UNAVAILABLE, e))?;
                Ok(Html(page_shell("Checkout", &body, &config)))
            }
        }
    }
}

async fn issue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<IssueRequest>,
) -> ApiResult<openindiecommerce_server::IssuedLicense> {
    require_admin(&headers, &state.admin_token)?;
    state
        .store
        .issue(
            &request.email,
            request.max_activations.unwrap_or(DEFAULT_MAX_ACTIVATIONS),
            request.order_provider.as_deref(),
            request.order_id.as_deref(),
        )
        .map(Json)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
}

async fn revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RevokeRequest>,
) -> ApiResult<RevokeResponse> {
    require_admin(&headers, &state.admin_token)?;
    state
        .store
        .revoke_by_order(&request.order_provider, &request.order_id)
        .map(|revoked| Json(RevokeResponse { revoked }))
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
}

async fn claim_license(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ClaimRequest>,
) -> ApiResult<openindiecommerce_server::IssuedLicense> {
    enforce_public_rate_limit(&state, &headers, "license-claim", 30)?;
    state
        .store
        .claim_paid_order(
            &request,
            state
                .entitlement_key_secret
                .as_deref()
                .ok_or_else(|| {
                    api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "entitlement/license module is not configured",
                    )
                })?
                .as_slice(),
        )
        .map(Json)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
}

async fn paddle_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<PaddleWebhookResponse> {
    let secret = state.paddle_webhook_secret.as_deref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Paddle webhook is not configured",
        )
    })?;
    let signature = headers
        .get("Paddle-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "missing Paddle-Signature"))?;
    paddle::verify_signature(
        secret,
        signature,
        &body,
        paddle::unix_now(),
        state.paddle_signature_tolerance_seconds,
    )
    .map_err(|e| api_error(StatusCode::UNAUTHORIZED, e))?;
    let event: PaddleEvent = serde_json::from_slice(&body).map_err(|e| {
        api_error(
            StatusCode::BAD_REQUEST,
            format!("invalid Paddle event: {e}"),
        )
    })?;
    let event_id = event.event_id.clone();
    let Some(outcome) = paddle::process_commerce_event(&state.commerce, &event)
        .map_err(|e| api_error(StatusCode::UNPROCESSABLE_ENTITY, e))?
    else {
        return Ok(Json(PaddleWebhookResponse {
            ok: true,
            action: "ignored",
            transaction_id: None,
        }));
    };
    tracing::info!(%event_id,order_id=%outcome.order.id,inserted=outcome.inserted,"processed canonical Paddle commerce event");
    let action = match outcome.event.event_type.as_str() {
        "order.paid" => {
            if outcome.inserted {
                "paid_recorded"
            } else {
                "paid_already_recorded"
            }
        }
        "order.refunded" => {
            if outcome.inserted {
                "refund_recorded"
            } else {
                "refund_already_recorded"
            }
        }
        "order.chargeback" => {
            if outcome.inserted {
                "chargeback_recorded"
            } else {
                "chargeback_already_recorded"
            }
        }
        _ => "commerce_event_recorded",
    };
    Ok(Json(PaddleWebhookResponse {
        ok: true,
        action,
        transaction_id: Some(outcome.order.provider_order_id),
    }))
}

async fn activate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ActivateRequest>,
) -> ApiResult<openindiecommerce_server::ActivationResponse> {
    enforce_public_rate_limit(&state, &headers, "license-activate", 30)?;
    state
        .store
        .activate(&request)
        .map(Json)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
}

async fn validate(
    State(state): State<AppState>,
    Json(request): Json<InstanceRequest>,
) -> ApiResult<openindiecommerce_server::ValidationResponse> {
    state
        .store
        .validate(&request)
        .map(Json)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
}

async fn deactivate(
    State(state): State<AppState>,
    Json(request): Json<InstanceRequest>,
) -> ApiResult<openindiecommerce_server::ValidationResponse> {
    state
        .store
        .deactivate(&request)
        .map(Json)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let db_path = env::var("OIC_DB").unwrap_or_else(|_| "openindiecommerce.sqlite3".to_string());
    let admin_token = secret_value("OIC_ADMIN_TOKEN")?;
    let entitlement_key_secret = optional_secret_value("OIC_ENTITLEMENT_KEY_SECRET")?;
    if let Some(secret) = entitlement_key_secret.as_deref() {
        anyhow::ensure!(
            secret.len() >= 32,
            "OIC_ENTITLEMENT_KEY_SECRET must be at least 32 bytes"
        );
    }
    let bind = env::var("OIC_BIND").unwrap_or_else(|_| "127.0.0.1:18791".to_string());
    let address: SocketAddr = bind.parse()?;
    let paddle_webhook_secret = optional_secret_value("OIC_PADDLE_WEBHOOK_SECRET")?.map(Arc::new);
    let paddle_client_token = env::var("OIC_PADDLE_CLIENT_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(Arc::new);
    let paddle_signature_tolerance_seconds = env::var("OIC_PADDLE_WEBHOOK_TOLERANCE_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(paddle::DEFAULT_SIGNATURE_TOLERANCE_SECONDS);
    let zpay_pid = env::var("OIC_ZPAY_PID")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(Arc::new);
    let zpay_key = optional_secret_value("OIC_ZPAY_KEY")?.map(Arc::new);
    let zpay_submit_url = Arc::new(
        env::var("OIC_ZPAY_SUBMIT_URL").unwrap_or_else(|_| zpay::DEFAULT_SUBMIT_URL.to_string()),
    );
    let zpay_public_base_url = env::var("OIC_PUBLIC_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(Arc::new);
    let state = AppState {
        store: LicenseStore::open(&db_path)?,
        commerce: CommerceStore::open(&db_path)?,
        admin_token: Arc::new(admin_token),
        entitlement_key_secret: entitlement_key_secret.map(|value| Arc::new(value.into_bytes())),
        paddle_webhook_secret,
        paddle_client_token,
        paddle_signature_tolerance_seconds,
        zpay_pid,
        zpay_key,
        zpay_submit_url,
        zpay_public_base_url,
        public_rate_limiter: RateLimiter::default(),
    };
    tokio::spawn(webhook_dispatch_loop(state.commerce.clone()));
    let app = Router::new()
        .route("/", get(home))
        .route("/terms", get(terms))
        .route("/privacy", get(privacy))
        .route("/refund", get(refund))
        .route("/health", get(health))
        .route("/openapi.json", get(openapi_spec))
        .route(
            "/v1/admin/webhooks",
            get(list_merchant_webhooks).post(create_merchant_webhook),
        )
        .route("/v1/admin/webhooks/{id}", patch(update_merchant_webhook))
        .route("/v1/admin/webhook-deliveries", get(list_webhook_deliveries))
        .route(
            "/v1/admin/webhook-deliveries/{id}/redeliver",
            post(redeliver_webhook),
        )
        .route("/v1/admin/products", post(create_product))
        .route("/v1/admin/prices", post(create_price))
        .route("/v1/checkout/sessions", post(create_checkout_session))
        .route("/v1/checkout/sessions/{id}", get(get_checkout_session))
        .route("/checkout/{id}", get(hosted_checkout))
        .route("/v1/admin/licenses", post(issue))
        .route("/v1/admin/licenses/revoke", post(revoke))
        .route("/v1/licenses/claim", post(claim_license))
        .route("/v1/licenses/activate", post(activate))
        .route("/v1/webhooks/paddle", post(paddle_webhook))
        .route("/v1/webhooks/zpay", get(zpay_webhook))
        .route("/v1/licenses/validate", post(validate))
        .route("/v1/licenses/deactivate", post(deactivate))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "OpenIndieCommerce server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod page_tests {
    use super::*;

    fn config() -> PublicSiteConfig {
        PublicSiteConfig {
            brand_name: "Example Store".into(),
            seller_legal_name: "Example Seller".into(),
            support_email: "support@example.test".into(),
            support_phone: "+1 555 0100".into(),
            refund_days: 14,
        }
    }

    #[test]
    fn public_compliance_pages_are_merchant_generic() {
        let c = config();
        let home = landing_page(&c);
        let terms = terms_page(&c);
        let privacy = privacy_page(&c);
        let refund = refund_page(&c);
        for html in [&home, &terms, &privacy, &refund] {
            assert!(html.contains("Example Store"));
            assert!(html.contains("Example Seller"));
            assert!(html.contains("support@example.test"));
        }
        assert!(terms.contains("Products and fulfillment"));
        assert!(privacy.contains("Commerce data"));
        assert!(refund.contains("14 days"));
        assert!(!home.contains("$19"));
    }

    #[test]
    fn generic_paddle_checkout_uses_canonical_session_metadata() {
        let session = CheckoutSession {
            id: "cs_test".into(),
            price_id: "price_1".into(),
            email: "buyer@example.com".into(),
            provider: "paddle".into(),
            status: "open".into(),
            success_url: None,
            cancel_url: None,
            metadata: serde_json::json!({}),
            expires_at: Some(i64::MAX),
        };
        let product = Product {
            id: "prod_1".into(),
            name: "Report".into(),
            description: "Annual report".into(),
            fulfillment: "download".into(),
        };
        let price = Price {
            id: "price_1".into(),
            product_id: "prod_1".into(),
            provider: "paddle".into(),
            currency: "USD".into(),
            unit_amount: 1900,
            provider_price_id: Some("pri_report".into()),
        };
        let html =
            generic_paddle_checkout_html("test_token", "pri_report", &session, &product, &price);
        assert!(html.contains("oic_checkout_session_id"));
        assert!(html.contains("cs_test"));
        assert!(html.contains("pri_report"));
    }

    #[test]
    fn generic_zpay_checkout_uses_session_as_merchant_order() {
        let session = CheckoutSession {
            id: "cs_cn".into(),
            price_id: "price_cn".into(),
            email: "buyer@example.com".into(),
            provider: "zpay".into(),
            status: "open".into(),
            success_url: None,
            cancel_url: None,
            metadata: serde_json::json!({}),
            expires_at: Some(i64::MAX),
        };
        let product = Product {
            id: "prod_1".into(),
            name: "Course".into(),
            description: "Video course".into(),
            fulfillment: "entitlement".into(),
        };
        let price = Price {
            id: "price_cn".into(),
            product_id: "prod_1".into(),
            provider: "zpay".into(),
            currency: "CNY".into(),
            unit_amount: 9900,
            provider_price_id: None,
        };
        let html = generic_zpay_checkout_html(
            "https://pay.example/submit",
            "pid1",
            "secret",
            "https://shop.example",
            &session,
            &product,
            &price,
        )
        .unwrap();
        assert!(html.contains("cs_cn"));
        assert!(html.contains("99.00"));
        assert!(html.contains("alipay"));
        assert!(html.contains("wxpay"));
    }

    #[test]
    fn rate_limiter_enforces_fixed_window_and_resets() {
        let limiter = RateLimiter::default();
        assert!(limiter.allow_at("checkout:1.2.3.4", 2, 60, 120));
        assert!(limiter.allow_at("checkout:1.2.3.4", 2, 60, 121));
        assert!(!limiter.allow_at("checkout:1.2.3.4", 2, 60, 122));
        assert!(limiter.allow_at("checkout:1.2.3.4", 2, 60, 180));
        assert!(limiter.allow_at("checkout:5.6.7.8", 2, 60, 122));
    }

    #[test]
    fn embedded_openapi_document_is_valid_json_and_v1() {
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/openapi.json")).unwrap();
        assert_eq!(value["openapi"], "3.1.0");
        assert!(value["paths"]["/v1/checkout/sessions"].is_object());
        assert!(value["paths"]["/v1/admin/webhook-deliveries/{id}/redeliver"].is_object());
    }
}
