# CMP CLI / SDK: M1 issue #15 checkpoint

- **Role:** Top-left CMP Client CLI/SDK specialist
- **Milestone:** M1 Trustworthy Context, issue #15
- **Repository:** `caleb-media-studio/cmp-client`
- **Worktree:** `/home/caleb/Development/cmp/.worktrees/cmp-client-m1-integration-20261116`
- **Branch:** `test/m1-cli-core-cloud-credentials-20261116`
- **PR:** Published separately for Master review. Do not auto-merge or deploy.

## Verified in this client-only checkpoint

1. **Encrypted credential fallback** uses the same on-disk helper for
   initial encrypted save, subsequent load and refresh-token rotation. It
   preserves the current versioned file format, passphrase requirements,
   Argon2id encryption and XChaCha20-Poly1305 with authenticated data.
2. **Security regression tests** use temporary files, never the live vault.
   Test plaintext absence, rotating tokens, wrong password, ciphertext
   corruption, restricted Unix file mode 0600 and symlink overwrite refusal.
3. **Offline streaming disable** regression verifies an existing outbox
   remains untouched, new local commits do not enqueue when streaming is off,
   and explicitly re-enabling streaming does not automatically upload.
4. **Existing M1 CLI diagnostics** from merged PR #14 remain unchanged:
   unauthenticated Core access and invalid branch heads fail closed.

## Tests / results

- `cargo test --locked --bin cmp --tests`: **44 passed**, including
  14 CMP unit tests, 8 doctor tests, 8 offline tests, 8 CLI review tests,
  2 clone tests and 4 Rust library tests.
- `cargo clippy --locked --bin cmp --tests -- -D warnings`: PASS.
- `pnpm build` and `pnpm test`: PASS.
- `CMP_PROTOCOL_SOURCE=<approved-checkout> pnpm protocol:check`: PASS;
  pinned protocol snapshot matches authoritative checkout.
- `CMP_CORE_SERVER_MODULE=<previously-built-Core> pnpm verify:local`:
  PASS for the existing Core HTTP behavior, not canonical namespace
  interoperability.
- **Not tested:** authenticated Cloud production, cross-tenant/revoked-member
  Cloud fixture, current macOS Keychain Always Allow interactive prompt,
  Windows credential manager, canonical Core `local/name` REST adapter.

## Compatibility matrix

| Case | Status |
| --- | --- |
| Offline embedded persistence, branches and CAS | Existing tests PASS |
| Opt-in offline streaming, retry/conflict, disable | PASS, new disabled-mode test |
| Encrypted credential fallback on disk | PASS, new isolated tests |
| macOS stable Keychain helper | Existing disposable test; not rerun in this Linux checkpoint |
| Core flat-ID REST SDK operations | PASS against current built Core |
| Core canonical `local/name` REST | BLOCKED on cmp-server issue #5 |
| Cloud OAuth, revocation and least-privilege reads | BLOCKED on Cloud fixture |
| User/customer secrets and production mutations | Never accessed or performed |

## Cross-team contract / blockers

- **Approved:** [cmp-protocol issue #5](https://github.com/caleb-media-studio/cmp-protocol/issues/5)
  canonical `owner/name`; self-hosted Core and embedded offline reserve
  `local/name`. Namespaces do not grant authorization.
- **Core team:** [cmp-server issue #5](https://github.com/caleb-media-studio/cmp-server/issues/5)
  needs to implement normalization while retaining historical SQLite flat
  keys. Do not add client-side flat-name aliases or invent endpoints.
- **Cloud Backend + Security:** [cmp-cloud PR #55](https://github.com/caleb-media-studio/cmp-cloud/pull/55)
  has a Master request for a disposable Firebase emulator/PostgreSQL fixture
  with two tenant identities, permitted/denied/revoked membership, and
  OAuth/CLI token capability tests. Not yet delivered in this checkpoint.

## Security, production and compatibility risk

No backend source, UI, migrations, secrets, production environments or
customer data touched. No released CMP binary replaced. Encrypted file
format is unchanged. The new credential file helpers are internal and
reuse existing secure atomic writes; no new exposed CLI or API surface.

## Next steps for Master

1. Review this scoped `cmp-client` PR and keep it distinct from Core naming.
2. Wait for the Core adapter and then run a **real** `cmp init --repo local/name`
   create/list/head/branch/commit/CAS/snapshot/diff/persistence conformance test.
3. Coordinate a disposable Cloud fixture, then test authenticated `cmp doctor`,
   401/403, revoked-member isolation and explicit streaming, with no live
   production/customer credentials.
4. On supported interactive macOS/Windows hosts, validate OS keychain prompt
   and persistence using only uniquely named disposable test entries.
