import http from 'node:http';
import crypto from 'node:crypto';

const OIC_BASE = process.env.OIC_BASE ?? 'http://127.0.0.1:18791';
const PRICE_ID = process.env.OIC_PRICE_ID;
const WEBHOOK_SECRET = process.env.OIC_WEBHOOK_SECRET;
const PORT = Number(process.env.PORT ?? 3000);

if (!PRICE_ID) throw new Error('OIC_PRICE_ID is required');
if (!WEBHOOK_SECRET) throw new Error('OIC_WEBHOOK_SECRET is required');

const entitlements = new Map(); // demo only: userId -> canonical order id
const processedEvents = new Set();

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on('data', chunk => chunks.push(chunk));
    req.on('end', () => resolve(Buffer.concat(chunks)));
    req.on('error', reject);
  });
}

function verifyWebhook(header, body, now = Math.floor(Date.now() / 1000)) {
  const parts = Object.fromEntries(
    String(header ?? '').split(';').map(x => x.split('=', 2))
  );
  const timestamp = Number(parts.t);
  if (!Number.isFinite(timestamp) || Math.abs(now - timestamp) > 300) return false;
  const expected = crypto
    .createHmac('sha256', WEBHOOK_SECRET)
    .update(`${timestamp}.`)
    .update(body)
    .digest('hex');
  const supplied = String(parts.v1 ?? '');
  if (supplied.length !== expected.length) return false;
  return crypto.timingSafeEqual(Buffer.from(supplied), Buffer.from(expected));
}

async function createCheckout(email) {
  const userId = `demo:${email.toLowerCase()}`;
  const response = await fetch(`${OIC_BASE}/v1/checkout/sessions`, {
    method: 'POST',
    headers: {'content-type': 'application/json'},
    body: JSON.stringify({
      priceId: PRICE_ID,
      email,
      successUrl: `http://localhost:${PORT}/success?user=${encodeURIComponent(userId)}`,
      cancelUrl: `http://localhost:${PORT}/`,
      metadata: {userId, sku: 'demo-report-2026'},
      expiresInSeconds: 1800
    })
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}

function html(res, body, status = 200) {
  res.writeHead(status, {'content-type': 'text/html; charset=utf-8'});
  res.end(body);
}

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url, `http://localhost:${PORT}`);
    if (req.method === 'GET' && url.pathname === '/') {
      html(res, `<h1>Independent report store</h1><form method="POST" action="/buy">
        <input name="email" type="email" required placeholder="you@example.com">
        <button>Buy report</button>
      </form>`);
      return;
    }

    if (req.method === 'POST' && url.pathname === '/buy') {
      const form = new URLSearchParams((await readBody(req)).toString());
      const email = String(form.get('email') ?? '').trim();
      const checkout = await createCheckout(email);
      res.writeHead(303, {location: OIC_BASE + checkout.checkoutUrl});
      res.end();
      return;
    }

    if (req.method === 'GET' && url.pathname === '/success') {
      const userId = url.searchParams.get('user');
      const orderId = userId && entitlements.get(userId);
      if (!orderId) {
        html(res, '<h1>Payment received or pending</h1><p>Fulfillment waits for the signed payment webhook. Refresh shortly.</p>');
        return;
      }
      html(res, `<h1>Purchase complete</h1><p>Order ${orderId}</p><p><a href="/download?user=${encodeURIComponent(userId)}">Download your report</a></p>`);
      return;
    }

    if (req.method === 'GET' && url.pathname === '/download') {
      const userId = url.searchParams.get('user');
      if (!userId || !entitlements.has(userId)) {
        res.writeHead(403); res.end('purchase required'); return;
      }
      res.setHeader('content-type', 'text/plain; charset=utf-8');
      res.setHeader('content-disposition', 'attachment; filename="demo-report.txt"');
      res.end('This is a protected digital product delivered after order.paid.\n');
      return;
    }

    if (req.method === 'POST' && url.pathname === '/webhooks/openindie') {
      const body = await readBody(req);
      if (!verifyWebhook(req.headers['openindie-signature'], body)) {
        res.writeHead(401); res.end('bad signature'); return;
      }
      const event = JSON.parse(body);
      if (!processedEvents.has(event.id)) {
        if (event.type === 'order.paid') {
          const {order, checkoutMetadata} = event.data;
          if (checkoutMetadata?.userId) {
            entitlements.set(checkoutMetadata.userId, order.id);
          }
        } else if (event.type === 'order.refunded' || event.type === 'order.chargeback') {
          for (const [userId, orderId] of entitlements) {
            if (orderId === event.data.order.id) entitlements.delete(userId);
          }
        }
        processedEvents.add(event.id);
      }
      res.end('ok');
      return;
    }

    res.writeHead(404); res.end('not found');
  } catch (error) {
    console.error(error);
    res.writeHead(500); res.end('server error');
  }
});

server.listen(PORT, () => {
  console.log(`Demo store: http://localhost:${PORT}`);
});
