import http from 'node:http';
import crypto from 'node:crypto';

const OIC_BASE = process.env.OIC_BASE ?? 'http://127.0.0.1:18791';
const PRICE_ID = process.env.OIC_PRICE_ID;
const WEBHOOK_SECRET = process.env.OIC_WEBHOOK_SECRET;

if (!PRICE_ID) throw new Error('OIC_PRICE_ID is required');
if (!WEBHOOK_SECRET) throw new Error('OIC_WEBHOOK_SECRET is required');

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on('data', chunk => chunks.push(chunk));
    req.on('end', () => resolve(Buffer.concat(chunks)));
    req.on('error', reject);
  });
}

function verifyWebhook(header, body) {
  const parts = Object.fromEntries(
    String(header ?? '').split(';').map(x => x.split('=', 2))
  );
  const timestamp = Number(parts.t);
  if (!Number.isFinite(timestamp)) return false;
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
  const response = await fetch(`${OIC_BASE}/v1/checkout/sessions`, {
    method: 'POST',
    headers: {'content-type': 'application/json'},
    body: JSON.stringify({
      priceId: PRICE_ID,
      email,
      successUrl: 'http://localhost:3000/success',
      cancelUrl: 'http://localhost:3000/',
      metadata: {userId: `demo:${email}`}
    })
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}
const server = http.createServer(async (req, res) => {
  try {
    if (req.method === 'GET' && req.url === '/') {
      res.setHeader('content-type', 'text/html; charset=utf-8');
      res.end(`<form method="POST" action="/buy">
        <input name="email" type="email" required placeholder="you@example.com">
        <button>Buy</button>
      </form>`);
      return;
    }

    if (req.method === 'POST' && req.url === '/buy') {
      const form = new URLSearchParams((await readBody(req)).toString());
      const checkout = await createCheckout(form.get('email'));
      res.writeHead(303, {location: OIC_BASE + checkout.checkoutUrl});
      res.end();
      return;
    }

    if (req.method === 'GET' && req.url === '/success') {
      res.end('Payment submitted. Fulfillment is confirmed by webhook.');
      return;
    }
    if (req.method === 'POST' && req.url === '/webhooks/openindie') {
      const body = await readBody(req);
      if (!verifyWebhook(req.headers['openindie-signature'], body)) {
        res.writeHead(401); res.end('bad signature'); return;
      }
      const event = JSON.parse(body);
      if (event.type === 'order.paid') {
        const {order, checkoutMetadata} = event.data;
        console.log('FULFILL', order.id, checkoutMetadata);
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

server.listen(3000, () => {
  console.log('Demo store: http://localhost:3000');
});
