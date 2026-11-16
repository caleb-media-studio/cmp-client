# CMP Client

Public-source client SDKs and CLI for the **Context Management Protocol**.
This is an audited clean-history repository candidate, still private until
the release audit is finished.

CMP Client supports both:
- CMP Core: versioned repositories, record snapshots, commits, branches and
  merges over HTTP, on a self-hosted CMP Server or CMP Cloud.
- Optional CMP Cloud: managed token providers, WebSocket subscriptions, agent
  coordination, and policy-authorized session APIs. Public SDK wrappers
  contain no proprietary Cloud server implementation.

Cloud-only methods check server-advertised capabilities and fail explicitly
on an unsupported self-hosted server. Core commands never require Cloud OAuth,
WebSocket connections, an organization account or session credentials.

## SDKs

Development implementations: TypeScript/JavaScript, Python, Go and Rust.
Java, Kotlin, C#, PHP, Ruby and Swift are planned, not shipped.

The TypeScript package includes Core HTTP, an optional Cloud API client,
host integration adapters and encrypted local outbox helpers. Host-specific
capture only runs under explicitly authorized CMP Cloud policies.

## Development

    pnpm install --frozen-lockfile
    pnpm build
    pnpm test
    pnpm verify:local
    pnpm protocol:check

Place CMP Protocol and CMP Server beside this project for local Core
compatibility tests. The full agent/archives integration suite moved out of
Core self-hosted testing. Only an actually deployed and authorized Cloud
service may validate Cloud streaming, coordination and sessions.

Canonical Core types: https://github.com/caleb-media-studio/cmp-protocol
Self-hosted Core server: https://github.com/caleb-media-studio/cmp-server
Managed Cloud: https://cloud.cmp.chat

Status: pre-alpha and not a published package.
