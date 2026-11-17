# CMP CLI/SDK M1 handoff to Master

- **Role:** CLI/SDK specialist (top-left ChatGPT Web conversation)
- **Milestone:** M1 Trustworthy Context Repository, P1 client reliability
- **Repository:** `caleb-media-studio/cmp-client`
- **Worktree:** `/home/caleb/Development/cmp/.worktrees/cmp-client-m1-cli-reliability-20261116`
- **Branch:** `fix/m1-cli-reliability-20261116`
- **Pull request:** https://github.com/caleb-media-studio/cmp-client/pull/14 (open, pending Master review)

## Changed files and verified implementation

- `sdk/rust/src/bin/cmp.rs`: `cmp doctor` exits nonzero for Core authentication failures, missing required Core capabilities, malformed repository lists, inaccessible selected repositories and missing branch heads. Local/offline diagnostics check real on-device repositories instead of claiming availability unconditionally. `doctor --offline` on Cloud/local-server config skips network and explicitly labels the result as skipped.
- `sdk/rust/tests/cli_doctor_reliability.rs`: eight mock HTTP/offline regression tests, including HTTP 401, required capabilities, hidden repository, correct branch, empty branch head, no Cloud credentials touched, and absent local repository.
- `scripts/verify-local.mjs`: optional `CMP_CORE_SERVER_MODULE=/absolute/path/to/cmp-server/dist/server.js` path for independent client worktree verification; original sibling Core location remains the default.
- `docs/rust-cli.md`: diagnostic and test command documentation.
- `docs/orchestration/HANDOFF.md`: this checkpoint.

## Compatibility matrix

| Component | Verified behavior | Outstanding constraint |
| --- | --- | --- |
| `cmp init`, local store | Offline init and persistence pass; no Git initialization | Core HTTP flat ID namespace mismatch below |
| Core repo operations | Local CAS, branch, merge, snapshot, and diff regression tests pass | Master must align Core repository identifiers with CLI |
| `cmp doctor` | Correct fail-closed exit status, repository authorization and head checks | No live customer/Cloud credentials were used |
| MCP installation | Existing Codex/OpenCode config preservation and stdio smoke tests pass | Claude Code integration not installed in user's live projects by this PR |
| Offline streaming | Opt-in-only, failed upload outbox, retry and conflict rejection test pass | No production streaming authorization validation |
| Credential persistence | Existing Argon2id encrypted-file and token-safe tests pass | Keychain/Cloud OAuth implementation unchanged, not live-retaken here |
| TypeScript SDK | Build, protocol fixtures, capability gating and Core HTTP conformance pass | Cloud-only services intentionally not part of public Core |
| Cloud tenant boundaries | Not certified by this client PR | Backend + Security owns negative RLS/revocation tests |

## Tests executed

- `cargo test --locked --bin cmp --tests`: **41 passed, zero failed**, including eight new doctor tests.
- `cargo clippy --locked --bin cmp --tests -- -D warnings`: pass.
- `pnpm build` and `pnpm test`: pass; five protocol tests, event replay and 10-language SDK matrix.
- `CMP_CORE_SERVER_MODULE=<built-core-server>/dist/server.js pnpm verify:local`: pass, public Core HTTP and unsupported-Cloud-capability checks.
- `pnpm protocol:check`: pinned manifest matches; verify exact upstream repo HEAD separately if available.

## Security and tenant checks

All new HTTP tests use loopback, disposable fake tokens and isolated temporary directories. Unauthorized repository listing produces an error exit code. Diagnostic output does not include bearer credentials. `doctor --offline` on Cloud mode makes no OAuth request or credential-file access. No private records, live Cloud tokens, tenant tables, SQL, UI, DNS, key rotation, production deployment or user environment modifications were used.

**These are client-side checks, not Cloud tenant-isolation certification.** Cloud must enforce per-request authorization, including private evidence, across REST, GraphQL, realtime and MCP.

## Cross-team API mismatch requiring Master decision

**Public self-hosted Core and CLI disagree about repository IDs.** The reference Core server accepts flat IDs such as `core-test` (and rejects slashes). Native `cmp init` requires `owner/name`, matching CMP Cloud. The existing public SDK Core conformance test creates `core-test`. Master and Protocol must decide whether Core adopts canonical namespaced identifiers or the CLI adds an explicitly separate flat-name Core mode. This engineer made **no server endpoint or protocol changes**.

No additional undocumented Cloud APIs were invented. Cloud streaming still relies on the documented Core HTTP operations, and a production authenticated integration fixture is outstanding.

## Production risk and blockers

No migration or deployment impact. **Behavior change:** scripts that previously saw `cmp doctor` succeed after an API 401 or wrong repository will correctly receive a failing exit code. The existing prefix `Local context store: available` remains for output compatibility.

**Blocker:** Master decision on Core flat IDs versus Cloud owner/repo namespace before claiming complete native CLI self-hosted interoperability.

**Next:** Master review and merge decision for this isolated PR; Backend + Security to supply an authorized Cloud tenant fixture; CLI specialist to test read-only production diagnostics and explicitly authorized streaming without changing CMP Cloud APIs.
