# OpenIndieCommerce 中文说明

OpenIndieCommerce 是一个面向独立开发者、小团队和软件工厂的**自托管商业基础设施层**。

它的目标不是替代 Paddle、Dodo、支付宝、微信或其他支付机构，而是让你的产品只接一次统一商业接口，之后可以替换支付通道，而不用把订单、Webhook、退款、授权逻辑全部重写一遍。

> 当前状态：**Alpha**。核心订单、Checkout、Webhook、Entitlement/License、部署模板、三条支付轨和 CI 已经实现并通过测试。但某个 Provider/地区只有在“真实交易 + 真实结算/提现”验证完成后，才应该被标记为 Production Verified。

## 1. 它解决什么问题

如果你做的是：

- Web 独立站；
- SaaS；
- Windows/macOS/Linux 桌面软件；
- PDF/报告/课程/数据包销售；
- API 权限；
- 会员/订阅类产品；
- 中国大陆开发者做全球销售；

你很快会遇到：

```text
海外客户 -> Paddle/Dodo
国内客户 -> 支付宝/微信/ZPAY
桌面软件 -> License
Web 网站 -> Webhook + 发货
退款 -> 撤销权限
换支付平台 -> 又重写一次
```

OpenIndieCommerce 把这些共同部分统一成：

```text
Product
Price
Checkout Session
Canonical Order
order.paid
order.refunded
order.chargeback
Merchant Webhook
Entitlement / License
```

支付平台只负责完成支付，OpenIndieCommerce 负责把各平台的差异翻译成同一套内部语义。

## 2. 架构

```text
你的独立站 / SaaS / 桌面软件
              |
              v
      OpenIndieCommerce
   Product / Price / Checkout
   Canonical Order / Events
   Merchant Webhook
   Entitlement / License
              |
       +------+------+ 
       |      |      |
    Paddle   Dodo   ZPAY
     海外     海外   中国大陆
```

它不托管资金，不保存银行卡信息，也不是支付机构。

## 3. 当前支持的支付轨

| Provider | 用途 | 当前状态 |
|---|---|---|
| Paddle | 海外主 MoR | Adapter 已实现 |
| Dodo Payments | 海外备用 MoR | Adapter 已实现 |
| ZPAY | 中国大陆支付宝/微信 | Adapter 已实现 |

支付平台开户、Sandbox/Test、Live、真实结算验证，请看：

[`docs/PROVIDER_ONBOARDING.md`](docs/PROVIDER_ONBOARDING.md)

## 4. 先选你的使用场景

### 场景 A：Web 独立站

推荐阅读顺序：

1. [`docs/QUICKSTART.md`](docs/QUICKSTART.md)
2. [`docs/WEB_INTEGRATION.md`](docs/WEB_INTEGRATION.md)
3. [`examples/web-store/README.md`](examples/web-store/README.md)

核心模式：

```text
浏览器
  -> 你的站点后端
  -> OpenIndieCommerce 创建 Checkout Session
  -> 用户进入支付页面
  -> Provider 支付成功
  -> Provider Webhook 到 OIC
  -> OIC 验签
  -> order.paid
  -> OIC 签名 Merchant Webhook
  -> 你的站点发货
```

**绝不能因为用户浏览器跳回 success 页面就认为已付款。**

真正的发货依据必须是经过验签的 `order.paid`。

### 场景 B：桌面付费软件

推荐阅读：

[`docs/DESKTOP_SOFTWARE.md`](docs/DESKTOP_SOFTWARE.md)

桌面软件中不要内置 Paddle/Dodo/ZPAY 的 API Key。

正确架构：

```text
Desktop App
  -> 打开 Buy Pro
  -> OIC Checkout
  -> 支付成功
  -> Entitlement / License
  -> activate
  -> validate
  -> deactivate
```

BigFileViewer 是当前第一个真实消费者。

### 场景 C：软件工厂

OpenIndieCommerce 可以作为所有产品共享的商业底座：

```text
产品 A -> OIC
产品 B -> OIC
产品 C -> OIC
Web 报告站 -> OIC
SaaS -> OIC
```

新产品不需要重新做支付、退款、Webhook、License、备份等基础设施。

## 5. 10 分钟本地启动

### 依赖

- Rust stable
- Linux/macOS/WSL 开发环境
- curl
- jq（推荐）

克隆：

```bash
git clone https://github.com/star8592/OpenIndieCommerce.git
cd OpenIndieCommerce
```

设置最小本地环境：

```bash
export OIC_DB=/tmp/openindiecommerce.sqlite3
export OIC_ADMIN_TOKEN='development-admin-token-change-me'
export OIC_BIND=127.0.0.1:18791

export OIC_BRAND_NAME='Example Store'
export OIC_SELLER_LEGAL_NAME='Example Seller'
export OIC_SUPPORT_EMAIL='support@example.com'
export OIC_SUPPORT_PHONE='+1 555 0100'
```

启动：

```bash
cargo run -p openindiecommerce-server
```

另开终端：

```bash
curl -s http://127.0.0.1:18791/health | jq
```

预期：

```json
{
  "ok": true
}
```

查看支付 Provider 状态：

```bash
curl -s \
  -H 'Authorization: Bearer development-admin-token-change-me' \
  http://127.0.0.1:18791/v1/admin/providers | jq
```

这时即使没有配置支付平台，也可以先测试 Product / Price / Checkout Session 核心 API。

完整步骤：[`docs/QUICKSTART.md`](docs/QUICKSTART.md)

## 6. 创建第一个商品

```bash
curl -s -X POST http://127.0.0.1:18791/v1/admin/products \
  -H 'Authorization: Bearer development-admin-token-change-me' \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "2026 AI Market Report",
    "description": "PDF + 数据附录",
    "fulfillment": "download"
  }' | jq
```

记下返回的 `id`。

## 7. 创建价格

金额统一使用最小货币单位：

```text
USD 19.00 -> 1900
CNY 99.00 -> 9900
```

### Paddle

```json
{
  "productId": "prod_...",
  "provider": "paddle",
  "currency": "USD",
  "unitAmount": 1900,
  "providerPriceId": "pri_..."
}
```

### Dodo

```json
{
  "productId": "prod_...",
  "provider": "dodo",
  "currency": "USD",
  "unitAmount": 1900,
  "providerPriceId": "pdt_..."
}
```

### ZPAY

```json
{
  "productId": "prod_...",
  "provider": "zpay",
  "currency": "CNY",
  "unitAmount": 9900
}
```

## 8. 创建 Checkout Session

```bash
curl -s -X POST http://127.0.0.1:18791/v1/checkout/sessions \
  -H 'Content-Type: application/json' \
  -d '{
    "priceId": "price_...",
    "email": "buyer@example.com",
    "successUrl": "https://shop.example/order/success",
    "cancelUrl": "https://shop.example/cart",
    "metadata": {
      "userId": "user_123",
      "campaign": "launch"
    }
  }' | jq
```

返回：

```text
checkoutUrl = /checkout/cs_...
```

浏览器打开：

```text
http://127.0.0.1:18791/checkout/cs_...
```

如果 Provider 尚未配置，到真正支付步骤会返回配置错误；这是正常的。

## 9. Provider 配置

### Paddle Sandbox

```bash
export OIC_PADDLE_CLIENT_TOKEN='test_...'
export OIC_PADDLE_WEBHOOK_SECRET='...'
```

每个 Paddle Price 还需要保存 Paddle `pri_...`。

### Dodo Test

```bash
export OIC_DODO_ENV=test
export OIC_DODO_API_KEY='...'
export OIC_DODO_WEBHOOK_SECRET='whsec_...'
```

每个 Dodo Price 保存对应 `pdt_...`。

测试通过前不要切：

```bash
OIC_DODO_ENV=live
```

### ZPAY

```bash
export OIC_PUBLIC_BASE_URL='https://commerce.example.com'
export OIC_ZPAY_PID='...'
export OIC_ZPAY_KEY='...'
```

生产环境不要把 key 明文写入 Git 或脚本，优先使用 `*_FILE`。

## 10. Web 独立站发货

注册 Merchant Webhook：

```bash
curl -s -X POST http://127.0.0.1:18791/v1/admin/webhooks \
  -H 'Authorization: Bearer development-admin-token-change-me' \
  -H 'Content-Type: application/json' \
  -d '{
    "url": "https://shop.example/api/openindie/webhook"
  }' | jq
```

返回的 `whsec_...` 只显示一次，应保存到你的站点服务端。

OpenIndieCommerce 发送：

```http
OpenIndie-Event-Id: evt_...
OpenIndie-Event-Type: order.paid
OpenIndie-Signature: t=...;v1=...
```

收到 `order.paid` 后，你的站点才执行：

- 开通会员；
- 生成下载链接；
- 增加 API 权限；
- 发报告；
- 创建软件 License；
- 解锁 SaaS Pro。

退款或 chargeback 时撤销对应权益。

完整示例：[`examples/web-store`](examples/web-store)

## 11. 生产部署

推荐结构：

```text
Internet
  -> Caddy HTTPS
  -> 127.0.0.1:18791
  -> OpenIndieCommerce
  -> SQLite WAL
```

推荐目录：

```text
/opt/openindiecommerce/bin/openindiecommerce-server
/etc/openindiecommerce/openindiecommerce.env
/etc/openindiecommerce/secrets/*
/var/lib/openindiecommerce/openindiecommerce.sqlite3
/var/backups/openindiecommerce/
```

原则：

- Rust 服务只监听 localhost；
- Caddy 对外提供 HTTPS；
- `/v1/admin/*` 不应暴露公网；
- secret 用 `0600` 文件；
- 上线前运行 `deploy/scripts/preflight.sh`；
- 开启 SQLite 在线备份；
- 定期验证备份并做恢复演练。

详见：[`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md)

## 12. 支付轨验收

统一检查：

```bash
export OIC_ACCEPTANCE_ADMIN_TOKEN='...'

python3 scripts/provider_acceptance.py \
  --base-url https://commerce.example.com \
  --provider all \
  --dry-run
```

单独检查：

```bash
--provider paddle
--provider dodo
--provider zpay
```

不要因为 Adapter 单测通过就宣布“支付已上线”。

真正的生产 gate：

```text
Adapter 测试通过
-> Sandbox/Test 真支付
-> Provider Webhook 验证
-> canonical order.paid
-> Merchant Webhook
-> 真实发货
-> refund/chargeback
-> Live 小额交易
-> 第一次真实 payout/settlement
```

## 13. 安全原则

- Provider API Key 永远留在服务端；
- 浏览器和桌面程序不能携带支付 secret；
- success URL 不能证明付款；
- Webhook 必须验签；
- 金额与币种必须复核；
- callback 必须幂等；
- admin API 不对公网开放；
- License Key 不应作为普通日志内容打印；
- 生产 secret 不进 Git；
- OpenIndieCommerce 不保存银行卡信息。

## 14. 文档索引

| 文档 | 说明 |
|---|---|
| [`docs/QUICKSTART.md`](docs/QUICKSTART.md) | 从零启动 |
| [`docs/WEB_INTEGRATION.md`](docs/WEB_INTEGRATION.md) | Web 独立站接入 |
| [`docs/DESKTOP_SOFTWARE.md`](docs/DESKTOP_SOFTWARE.md) | 桌面软件收费/授权 |
| [`docs/API.md`](docs/API.md) | API 说明 |
| [`docs/openapi.json`](docs/openapi.json) | OpenAPI 3.1 |
| [`docs/PROVIDER_ONBOARDING.md`](docs/PROVIDER_ONBOARDING.md) | Paddle/Dodo/ZPAY 开户验收 |
| [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md) | 正式部署 |
| [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md) | 排错 |
| [`deploy/README.md`](deploy/README.md) | 部署文件说明 |
| [`examples/web-store`](examples/web-store) | 可运行独立站例子 |

## 15. 开发自检

```bash
cargo fmt --all -- --check
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 -m unittest discover -s scripts/tests -v
node --check examples/web-store/server.mjs
git diff --check
```

## 16. 当前非目标

v0.1 暂时不做：

- 资金托管；
- 自己成为 MoR；
- 保存银行卡数据；
- 一次接几十个 PSP；
- 企业级复杂 billing；
- 多租户托管控制台；
- 自动税务/法律结论。

目标是先把**独立开发者最常用、最容易重复造轮子的商业基础设施**做小、做稳、做透明。

## License

Apache-2.0。详见 [`LICENSE`](LICENSE)。
