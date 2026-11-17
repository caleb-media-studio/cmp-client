# M1 Core and native CLI interoperability handoff

**Role:** CLI/SDK, Protocol, and Core interoperability specialist
**Scope:** `cmp-protocol` issue #5, `cmp-server` issue #5, and `cmp-client` issue #15
**Status:** Separate PRs awaiting Master approval; none merged or deployed.

## Source branches

- Protocol `feat/m1-canonical-local-20261116`, commit `fbe3a4a385173d55271aa5446348739aa2163fc0`, PR #6
- Core `feat/m1-canonical-local-20261116`, commit `12e14758fdae182578eda95a30cc04c3c0f6ba66`, PR #6
- Client `feat/m1-canonical-local-20261116`, dedicated client PR opened after native test verification

## Implementation

1. **Protocol:** public canonical repository identity `owner/name`, single REST path parameter
   `local%2Fname`, Core reference validation schema and shared valid/invalid
   fixtures, legacy flat alias compatibility and matching `.cmp` manifest identity.
   The `local` namespace is not a security principal.
2. **Core:** REST, RPC/MCP and `.cmp` use `local/name` publicly. Internally,
   SQLite repositories, commit ancestry and event logs retain original flat
   `name` keys. Existing flat HTTP/RPC requests are deprecated local-only aliases.
   Raw encoded traversal, wrong namespace and double-encoded route segments are
   rejected before they can redirect a repository lookup. Production safety
   gates stay unchanged: `CMP_ENV=development`, loopback-only binding, a
   bearer token of at least 32 chars and no public production deployment.
3. **Client:** native CLI and TypeScript SDK talk to actual Core over loopback.
   `scripts/verify-cli-core.mjs` launches the compiled Core server with
   a disposable SQLite database and invokes a separately compiled real CMP
   Rust binary, not a mock. Old flat records are seeded before server start
   and checked again after restart. `scripts/verify-local.mjs` now uses
   canonical IDs and checks the `.cmp` manifest/snapshot via TypeScript SDK.
   Protocol types are pinned to the Protocol feature branch SHA pending Master
   merge sequencing.

## Actual test evidence

- Protocol `pnpm test && pnpm build`: six TypeScript tests, schema/fixture
  conformance and build passed.
- Core `pnpm test`: 14/14 tests passed, including SQLite legacy repository
  persistence, canonical route/RPC, invalid owner, malformed path, raw HTTP
  traversal, CAS and standalone compiled process restart.
- Rust CLI and SDK `cargo test --locked --bin cmp --tests`: 44/44 passed;
  strict Clippy with `-D warnings`: pass.
- Client `pnpm build`, `pnpm test`, `pnpm protocol:check`: passed,
  with protocol types mirrored and pinned to PR #6 commit.
- Client native E2E with real Core and SQLite: PASS `cmp setup`, `cmp init`
  existing and `--create`, `cmp repo list/create/view/clone`, `cmp doctor`,
  head, branches, commits, snapshots, diffs, merge, stale CAS 409, auth 401,
  `.cmp` identity and snapshot, historical flat storage and restart.
- TypeScript `CMP_CORE_SERVER_MODULE=... pnpm verify:local`: PASS on actual
  Core feature build, including canonical repository listing, metadata and
  TypeScript SDK `.cmp` manifest/refs/snapshot. Other JS tests and protocol
  mirror checks passed.
- The native CLI test is executable only when both `CMP_CLI_BINARY` and
  `CMP_CORE_SERVER_MODULE` point to real compiled artifacts; it exits with
  a configuration error if absent. It does not use a mock fallback.

## Reproduction from separate checked-out worktrees

```bash
# Build the Core feature branch using its own isolated worktree.
cd /path/to/cmp-server
pnpm install --frozen-lockfile
pnpm build

# Build the native Rust CLI from the client feature branch.
cd /path/to/cmp-client
cargo build --locked --manifest-path sdk/rust/Cargo.toml --bin cmp

# Run true CLI subprocesses against real Core and SQLite, all disposable.
CMP_CLI_BINARY=/absolute/path/to/cmp-client/sdk/rust/target/debug/cmp \
CMP_CORE_SERVER_MODULE=/absolute/path/to/cmp-server/dist/server.js \
pnpm verify:cli-core

# Verify the TypeScript SDK against the same Core build.
CMP_CORE_SERVER_MODULE=/absolute/path/to/cmp-server/dist/server.js \
pnpm verify:local
```

Adjust the native binary path if `CARGO_TARGET_DIR` is set. All test files
and SQLite databases are under OS temporary storage and removed on teardown.
The test uses only a newly created local development bearer token.

## Security and compatibility

- No migration of existing SQLite databases, Cloud project, production
  network, tenant data, credentials or customer keys.
- Repository strings never authorize a request. The Core development bearer
  check precedes all protected repository and transport routes.
- Legacy flat aliases are intentionally supported **only** by self-hosted
  Core, and canonical names are always returned by new public responses.
- The `.cmp` URL is CMP JSON context repository transport, not Git HTTP.
- Client's protocol mirror points at Protocol PR #6 commit until Master
  merges it. After squash merge, confirm the mirror's hashes against the
  merged Protocol `main` commit and update the pin if needed.
- This does **not** authorize a Core production launch or assume Cloud OAuth
  is compatible with the local development token.

## Remaining M1 integration requirement

Backend + Security must still supply the disposable Firebase emulator and
PostgreSQL two-tenant fixture requested on cmp-cloud PR #55 before Cloud
`cmp doctor`, denied/revoked membership and opt-in streaming can be tested.
No live Cloud user credentials or customer data should substitute for it.

## Merge order requested from Master

Protocol PR #6, Core PR #6, then Client PR. Re-run client protocol mirror
verification and true native interoperability after any changes to those PR
heads. Do not auto-merge.
