# CMP Client: Core and optional Cloud API contracts

Source status: Clean-history release candidate, still private.

The public CMP Client implements the Core HTTP repository operations for
both self-hosted CMP Server and CMP Cloud.

Cloud-only API wrappers (agent presence, coordination, archives and WSS)
remain public client code under packages/sdk/src. Their type-only models
are kept in packages/sdk/src/cloud instead of the normative CMP Protocol
package. They do not contain proprietary Cloud backend services.

All optional methods check /v1/capabilities before issuing Cloud API requests.
Unavailable capabilities throw UnsupportedCMPCapability. A self-hosted Core
endpoint does not receive agent IDs/tokens, archive events or Connect data
because a Cloud-only operation is gated before the RPC request.

The Client may expose a Cloud Connect caller only once the deployed Cloud
endpoint and supported request schema are available and tested. Do not
claim speculative methods have been deployed.

The client-side host companion is Cloud-facing and optional. All collection
must respect both remote policy and local consent; no binary injection, hidden
model reasoning capture or bulk scanning of private user data.

Core protocol sources are mirrored from cmp-protocol using the
protocol:sync/protocol:check commands, with pinned source hash. Cloud-specific
client type declarations are not normative Core schemas.

Language support is provisional. Python, Go and Rust wrappers for optional
Cloud methods are client adapters, not independent backend implementations.
Cross-language backend test suites for those services belong with private
Cloud integration testing, not with the self-hosted Core server.
