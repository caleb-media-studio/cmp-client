# CMP Client SDKs

TypeScript/JavaScript, Python, Go and Rust Core clients are implemented as
development reference SDKs. Java, Kotlin, C#, PHP, Ruby and Swift remain
planned. See support-matrix.json for the current development matrix.

The baseline is the Core HTTP repository API: create/list, get head,
snapshots, branches, CAS commit, diff and merge. Cloud features are
optional and are not implemented by self-hosted CMP Server.

Some SDKs contain Cloud-only agent/session API adapters. These are
public client wrappers, not hosted server implementations. Use feature
discovery and appropriate Cloud authentication before using them. The
TypeScript client throws UnsupportedCMPCapability on unsupported endpoints.

No SDK should automatically connect to WSS, upload sessions or use Cloud
managed identity to perform a basic Core repository operation.
