# API versioning policy

OpenIndieCommerce uses explicit major API paths such as `/v1/...`.

## Compatibility rules

Within v1, releases may add endpoints, optional request fields, response fields, event fields, providers and enum values where clients are expected to ignore unknown values. Existing required fields, meanings, authentication rules and event semantics must not be changed incompatibly.

A change that removes or renames a field, changes money semantics, changes signature construction, changes an endpoint from optional to required behavior, or otherwise breaks a conforming v1 client requires a new major path such as `/v2`.

Provider callback endpoints are versioned with the commerce server but provider signature verification follows the upstream provider protocol.

Merchant webhook event types are stable contracts. New event types may be added in v1; existing event types keep their documented meaning.
