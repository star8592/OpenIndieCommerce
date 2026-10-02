# OpenIndieCommerce Documentation

Use this page as the documentation index.

## Start here

| You want to... | Read |
|---|---|
| understand the project | [`../README.md`](../README.md) |
| read in Chinese | [`../README.zh-CN.md`](../README.zh-CN.md) |
| run it locally from zero | [`QUICKSTART.md`](QUICKSTART.md) |
| connect an independent web store | [`WEB_INTEGRATION.md`](WEB_INTEGRATION.md) |
| sell desktop software / licenses | [`DESKTOP_SOFTWARE.md`](DESKTOP_SOFTWARE.md) |
| deploy to production | [`DEPLOYMENT.md`](DEPLOYMENT.md) |
| configure Paddle / Dodo / ZPAY | [`PROVIDER_ONBOARDING.md`](PROVIDER_ONBOARDING.md) |
| debug a failure | [`TROUBLESHOOTING.md`](TROUBLESHOOTING.md) |
| use the HTTP API | [`API.md`](API.md) |
| generate clients/tools | [`openapi.json`](openapi.json) |
| understand compatibility rules | [`API_VERSIONING.md`](API_VERSIONING.md) |
| understand architecture | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| see planned work | [`ROADMAP.md`](ROADMAP.md) |

## Recommended learning path

### Web independent-site developer

```text
README
  -> QUICKSTART
  -> WEB_INTEGRATION
  -> examples/web-store
  -> PROVIDER_ONBOARDING
  -> DEPLOYMENT
```

### Desktop software developer

```text
README
  -> QUICKSTART
  -> DESKTOP_SOFTWARE
  -> API/OpenAPI
  -> PROVIDER_ONBOARDING
  -> DEPLOYMENT
```

### Operator

```text
DEPLOYMENT
  -> deploy/README
  -> PROVIDER_ONBOARDING
  -> TROUBLESHOOTING
```

### Contributor

```text
ARCHITECTURE
  -> API
  -> API_VERSIONING
  -> ROADMAP
  -> tests / CI
```

## Important conceptual boundaries

OpenIndieCommerce is:

- a commerce orchestration layer;
- a canonical order/event layer;
- a signed webhook delivery layer;
- optionally an entitlement/license layer.

OpenIndieCommerce is not:

- a payment processor;
- a bank;
- a fund custodian;
- a Merchant of Record;
- a card vault;
- tax or legal advice.

## Source examples

The current reference application is:

[`../examples/web-store`](../examples/web-store)

The first real desktop consumer is BigFileViewer in its separate repository.
