#!/usr/bin/env python3
import argparse, html, json, os, re, sys
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

class AcceptanceError(RuntimeError): pass

def request_json(method, url, token=None, payload=None):
    headers = {'Accept': 'application/json'}
    data = None
    if token:
        headers['Authorization'] = f'Bearer {token}'
    if payload is not None:
        headers['Content-Type'] = 'application/json'
        data = json.dumps(payload).encode()
    req = Request(url, data=data, headers=headers, method=method)
    try:
        with urlopen(req, timeout=20) as r:
            return json.loads(r.read().decode())
    except HTTPError as e:
        raise AcceptanceError(f'{method} {url} -> HTTP {e.code}: {e.read().decode(errors="replace")}')
    except URLError as e:
        raise AcceptanceError(f'{method} {url} failed: {e.reason}')

def fetch_text(url):
    try:
        with urlopen(url, timeout=25) as r:
            return r.read().decode(errors='replace')
    except HTTPError as e:
        raise AcceptanceError(f'GET {url} -> HTTP {e.code}: {e.read().decode(errors="replace")}')
    except URLError as e:
        raise AcceptanceError(f'GET {url} failed: {e.reason}')

def provider_map(doc):
    return {row['provider']: row for row in doc.get('providers', [])}

def dodo_checkout_url(page):
    match = re.search(r'<a href="([^"]+)">Continue</a>', page)
    if not match:
        raise AcceptanceError('Dodo hosted checkout page did not expose the provider checkout URL')
    return html.unescape(match.group(1))

def main():
    ap = argparse.ArgumentParser(description='OpenIndieCommerce provider acceptance harness')
    ap.add_argument('--base-url', default=os.environ.get('OIC_ACCEPTANCE_BASE_URL', 'http://127.0.0.1:18791'))
    ap.add_argument('--provider', choices=['dodo'], default='dodo')
    ap.add_argument('--email', default=os.environ.get('OIC_ACCEPTANCE_EMAIL', 'buyer@example.test'))
    ap.add_argument('--provider-product-id', default=os.environ.get('OIC_DODO_PRODUCT_ID'))
    ap.add_argument('--currency', default='USD')
    ap.add_argument('--amount', type=int, default=100, help='minor units, default 100 = USD 1.00')
    ap.add_argument('--allow-live', action='store_true')
    ap.add_argument('--dry-run', action='store_true', help='validate service/provider configuration without creating checkout objects')
    args = ap.parse_args()
    base = args.base_url.rstrip('/')
    token = os.environ.get('OIC_ACCEPTANCE_ADMIN_TOKEN') or os.environ.get('OIC_ADMIN_TOKEN')
    if not token:
        raise AcceptanceError('set OIC_ACCEPTANCE_ADMIN_TOKEN (or OIC_ADMIN_TOKEN)')
    request_json('GET', f'{base}/health')
    status = provider_map(request_json('GET', f'{base}/v1/admin/providers', token))
    rail = status.get(args.provider)
    if not rail or not rail.get('configured'):
        print(json.dumps({'status':'BLOCKED','provider':args.provider,'reason':'provider is not fully configured'}, indent=2))
        return 2
    if args.provider == 'dodo':
        mode = rail.get('mode')
        if args.dry_run:
            print(json.dumps({'status':'PROVIDER_CONFIG_READY','provider':'dodo','mode':mode}, indent=2))
            return 0
        if mode == 'live' and not args.allow_live:
            raise AcceptanceError('refusing live Dodo acceptance run without --allow-live')
        if not args.provider_product_id:
            raise AcceptanceError('set OIC_DODO_PRODUCT_ID or pass --provider-product-id')
        product = request_json('POST', f'{base}/v1/admin/products', token, {
            'name':'OIC acceptance fixture','description':'Disposable Dodo acceptance product','fulfillment':'none'})
        price = request_json('POST', f'{base}/v1/admin/prices', token, {
            'productId':product['id'],'provider':'dodo','currency':args.currency,
            'unitAmount':args.amount,'providerPriceId':args.provider_product_id})
        session = request_json('POST', f'{base}/v1/checkout/sessions', payload={
            'priceId':price['id'],'email':args.email,'metadata':{'acceptance':'dodo-test'}})
        page = fetch_text(base + session['checkoutUrl'])
        checkout = dodo_checkout_url(page)
        print(json.dumps({'status':'READY_FOR_TEST_PAYMENT','provider':'dodo','mode':mode,
            'productId':product['id'],'priceId':price['id'],'sessionId':session['session']['id'],
            'providerCheckoutUrl':checkout}, indent=2))
        return 0
    raise AcceptanceError(f'unsupported provider: {args.provider}')

if __name__ == '__main__':
    try: sys.exit(main())
    except AcceptanceError as e:
        print(json.dumps({'status':'ERROR','error':str(e)}, indent=2), file=sys.stderr); sys.exit(1)
