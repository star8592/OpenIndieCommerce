#!/usr/bin/env python3
import argparse, html, json, os, re, sys
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

PROVIDERS = ("paddle", "dodo", "zpay")


class AcceptanceError(RuntimeError):
    pass


def request_json(method, url, token=None, payload=None):
    headers = {"Accept": "application/json"}
    data = None
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if payload is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(payload).encode()
    req = Request(url, data=data, headers=headers, method=method)
    try:
        with urlopen(req, timeout=20) as response:
            return json.loads(response.read().decode())
    except HTTPError as error:
        raise AcceptanceError(
            f"{method} {url} -> HTTP {error.code}: {error.read().decode(errors='replace')}"
        ) from error
    except URLError as error:
        raise AcceptanceError(f"{method} {url} failed: {error.reason}") from error


def fetch_text(url):
    try:
        with urlopen(url, timeout=25) as response:
            return response.read().decode(errors="replace")
    except HTTPError as error:
        raise AcceptanceError(
            f"GET {url} -> HTTP {error.code}: {error.read().decode(errors='replace')}"
        ) from error
    except URLError as error:
        raise AcceptanceError(f"GET {url} failed: {error.reason}") from error


def provider_map(doc):
    return {row["provider"]: row for row in doc.get("providers", [])}


def readiness_report(status, providers):
    rows = []
    for provider in providers:
        rail = status.get(provider)
        rows.append(
            {
                "provider": provider,
                "configured": bool(rail and rail.get("configured")),
                "mode": rail.get("mode") if rail else None,
            }
        )
    ready = sum(1 for row in rows if row["configured"])
    if ready == len(rows):
        overall = "ALL_PROVIDER_CONFIG_READY"
    elif ready:
        overall = "PARTIAL"
    else:
        overall = "BLOCKED"
    return {"status": overall, "providers": rows}


def dodo_checkout_url(page):
    match = re.search(r'<a href="([^"]+)">Continue</a>', page)
    if not match:
        raise AcceptanceError(
            "Dodo hosted checkout page did not expose the provider checkout URL"
        )
    return html.unescape(match.group(1))


def assert_checkout_page(provider, page):
    markers = {
        "paddle": ("Paddle.Checkout.open", "Continue to secure checkout"),
        "zpay": ("支付宝", "微信支付"),
    }
    required = markers.get(provider, ())
    missing = [marker for marker in required if marker not in page]
    if missing:
        raise AcceptanceError(
            f"{provider} hosted checkout page is missing expected markers: {missing}"
        )


def default_provider_product_id(provider):
    if provider == "dodo":
        return os.environ.get("OIC_DODO_PRODUCT_ID")
    if provider == "paddle":
        return os.environ.get("OIC_PADDLE_PRICE_ID")
    return None


def default_currency(provider):
    return "CNY" if provider == "zpay" else "USD"


def create_fixture(base, token, provider, email, amount, currency, provider_product_id):
    product = request_json(
        "POST",
        f"{base}/v1/admin/products",
        token,
        {
            "name": "OIC acceptance fixture",
            "description": f"Disposable {provider} acceptance product",
            "fulfillment": "none",
        },
    )
    price_payload = {
        "productId": product["id"],
        "provider": provider,
        "currency": currency,
        "unitAmount": amount,
    }
    if provider_product_id:
        price_payload["providerPriceId"] = provider_product_id
    price = request_json("POST", f"{base}/v1/admin/prices", token, price_payload)
    session = request_json(
        "POST",
        f"{base}/v1/checkout/sessions",
        payload={
            "priceId": price["id"],
            "email": email,
            "metadata": {"acceptance": f"{provider}-acceptance"},
        },
    )
    return product, price, session


def main():
    parser = argparse.ArgumentParser(
        description="OpenIndieCommerce provider acceptance harness"
    )
    parser.add_argument(
        "--base-url",
        default=os.environ.get("OIC_ACCEPTANCE_BASE_URL", "http://127.0.0.1:18791"),
    )
    parser.add_argument(
        "--provider",
        choices=[*PROVIDERS, "all"],
        default="dodo",
    )
    parser.add_argument(
        "--email",
        default=os.environ.get("OIC_ACCEPTANCE_EMAIL", "buyer@example.test"),
    )
    parser.add_argument("--provider-product-id")
    parser.add_argument("--currency")
    parser.add_argument(
        "--amount",
        type=int,
        default=100,
        help="minor units, default 100 = USD 1.00 / CNY 1.00",
    )
    parser.add_argument("--allow-live", action="store_true")
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="validate service/provider configuration without creating checkout objects",
    )
    args = parser.parse_args()

    base = args.base_url.rstrip("/")
    token = os.environ.get("OIC_ACCEPTANCE_ADMIN_TOKEN") or os.environ.get(
        "OIC_ADMIN_TOKEN"
    )
    if not token:
        raise AcceptanceError(
            "set OIC_ACCEPTANCE_ADMIN_TOKEN (or OIC_ADMIN_TOKEN)"
        )

    request_json("GET", f"{base}/health")
    status = provider_map(
        request_json("GET", f"{base}/v1/admin/providers", token)
    )

    if args.provider == "all":
        if not args.dry_run:
            raise AcceptanceError("--provider all requires --dry-run")
        report = readiness_report(status, PROVIDERS)
        print(json.dumps(report, indent=2))
        return 0 if report["status"] == "ALL_PROVIDER_CONFIG_READY" else 2

    rail = status.get(args.provider)
    if not rail or not rail.get("configured"):
        print(
            json.dumps(
                {
                    "status": "BLOCKED",
                    "provider": args.provider,
                    "reason": "provider is not fully configured",
                },
                indent=2,
            )
        )
        return 2

    mode = rail.get("mode")
    if args.dry_run:
        print(
            json.dumps(
                {
                    "status": "PROVIDER_CONFIG_READY",
                    "provider": args.provider,
                    "mode": mode,
                },
                indent=2,
            )
        )
        return 0

    if args.provider == "dodo" and mode == "live" and not args.allow_live:
        raise AcceptanceError(
            "refusing live Dodo acceptance run without --allow-live"
        )

    provider_product_id = args.provider_product_id or default_provider_product_id(
        args.provider
    )
    if args.provider in ("dodo", "paddle") and not provider_product_id:
        variable = (
            "OIC_DODO_PRODUCT_ID" if args.provider == "dodo" else "OIC_PADDLE_PRICE_ID"
        )
        raise AcceptanceError(
            f"set {variable} or pass --provider-product-id"
        )

    currency = args.currency or default_currency(args.provider)
    product, price, session = create_fixture(
        base,
        token,
        args.provider,
        args.email,
        args.amount,
        currency,
        provider_product_id,
    )
    hosted_url = base + session["checkoutUrl"]
    page = fetch_text(hosted_url)

    result = {
        "provider": args.provider,
        "mode": mode,
        "productId": product["id"],
        "priceId": price["id"],
        "sessionId": session["session"]["id"],
        "oicHostedCheckoutUrl": hosted_url,
    }
    if args.provider == "dodo":
        result["status"] = "READY_FOR_TEST_PAYMENT"
        result["providerCheckoutUrl"] = dodo_checkout_url(page)
    else:
        assert_checkout_page(args.provider, page)
        result["status"] = "READY_FOR_BROWSER_TEST_PAYMENT"

    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except AcceptanceError as error:
        print(
            json.dumps({"status": "ERROR", "error": str(error)}, indent=2),
            file=sys.stderr,
        )
        sys.exit(1)
