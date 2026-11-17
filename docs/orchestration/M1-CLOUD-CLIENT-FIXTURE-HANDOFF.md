# M1 CMP Client Cloud fixture verification: HANDOFF TO MASTER

**Repository:** `caleb-media-studio/cmp-client`
**Branch:** `test/m1-cloud-disposable-fixture-20261117`
**PR/SHA:** Filled in from the published review branch after final tests.
**Scope:** Client issue #15, Cloud authentication and explicitly opt-in streaming.
**Non-scope:** Cloud production, Cloud incident #61, migrations, PR #55 deployment, package releases.
**Status:** Client-only runnable harness + tests. Real Cloud multi-tenant results remain BLOCKED until Backend supplies the fixture contract below.

## Source evidence / precise API contract

- Merged Protocol `b2f6a600c22c`, Core `eca09ba6b4ef3`, Client
  `e21d9a01841e` are the already completed Core interoperability baseline.
- Current CMP Cloud `/v1/capabilities` exposes **global** protocol capabilities;
  `/v1/repos`, `/v1/repos/{encoded-owner%2Frepo}/head` and commits use a
  separately authenticated OAuth access token from
  `POST /api/oauth/token` with `grant_type=refresh_token`,
  `client_id=cmp-cli`. The refresh grant rotates on each exchange.
- Do not confuse the CLI's `/v1/capabilities` with Cloud PR #55's
  **browser-only** `/api/hub/repositories/:owner/:repo/capabilities`.
  That proposal uses Firebase ID tokens and v2 UI metadata and must not
  be substituted for native CLI endpoints. The Cloud global capabilities
  endpoint is currently a public feature matrix; `cmp doctor` verifies
  actual authenticated repository listing and selected branch access afterward.
- Cloud global writes may be disabled even when reads work. This PR adjusts
  Cloud doctor to demand read operations and still verify member list/HEAD;
  Core doctor keeps its stricter full Core capabilities requirement.

## Fixture prerequisites for Backend + Security

Backend owns and runs a **real disposable** local Cloud API with actual
PostgreSQL 17, Firebase Auth emulator, real CMP OAuth exchange/rotation and
Next.js route handlers from its approved test branch. A route stub or mocked
HTTP success cannot close this milestone. No customer data or tenant tokens
may be copied into the fixture.

1. Seed two distinct namespaces `test-alpha` and `test-beta`. Create
   `test-alpha/owner-writable`, `test-alpha/reader-readable`,
   `test-beta/private-repository`, `test-alpha/revocable-member`,
   `test-alpha/stream-empty`, `test-alpha/revocable-grant`. The stream
   repository must have a valid `main` head and **zero records**.
   These exact names are illustrative: the manifest may contain alternate
   disposable owner/name values. Each should be private.
2. Issue distinct **real disposable CMP CLI OAuth grants** through the
   Firebase-emulator-backed authorization-code PKCE exchange, not arbitrary
   bearer tokens. Supply rotated-capable `cmpr_` refresh tokens only via
   these process environment variables, never in the committed manifest:
   `CMP_FIXTURE_REFRESH_OWNER`, `_READER`, `_OUTSIDER`,
   `_REVOKED_MEMBER`, `_REVOKED_GRANT`, `_EXPIRED` (all with
   `CMP_FIXTURE_REFRESH` prefix). An expired token must be known expired
   to the backend before verification. The granted users must have
   active membership/roles matching the repository map, other than the
   expired grant.
3. Owner must be able to read/write alpha's owner and stream repos. Reader
   may read alpha's reader repo but may **not** commit. Outsider has access
   to beta's private repo, but must not list or read alpha's private repo.
   The revocable member and revocable OAuth principals must initially
   pass a selected-repository `cmp doctor` before revocation.
4. Provide a local executable `CMP_CLOUD_FIXTURE_CONTROL`. This is an
   **out-of-band fixture controller**; it is NOT an exposed Cloud API.
   It consumes one JSON object from stdin and replies `{"ok":true}` on
   stdout, without tokens/PII. Required actions:

   | Input JSON | Required disposable fixture behavior |
   |---|---|
   | `{"action":"revokeMembership","principal":"revokedMember","repository":"test-alpha/revocable-member"}` | Revoke current SQL membership; already issued grant must lose repo access |
   | `{"action":"revokeOAuth","principal":"revokedGrant"}` | Revoke the OAuth authorization/grant; next refresh must fail 401 |
   | `{"action":"failNextCommit","repository":"test-alpha/stream-empty","status":503}` | Return one transient 503 to the next authorized commit on this repo, **without writing**; following retry succeeds |

   Controller must reject unrecognized actions and repositories. The CLI
   harness uses a byte-forwarding local HTTP counting proxy and does not
   require backend request counters or a new privileged HTTP route.
5. Write a local manifest under a disposable directory using
   `scripts/fixtures/cloud-fixture.example.json` as the template, with
   `origin=http://127.0.0.1:<ephemeral-port>`,
   `firebaseProjectId` **not** `cmp-cloud-1`,
   `kind=firebase-auth-emulator-postgresql`,
   `disposable=true`, `realCloudHttp=true`.
   No credentials appear in the manifest. Backend must provide safe
   setup/teardown and certify that its Next.js instance is not live.

## Run commands for other agents (copy/paste after fixture exists)

```bash
# cmp-client on an isolated checkout/worktree of this PR; no production binary
pnpm install --frozen-lockfile
pnpm verify:cloud:preflight

# Build a throwaway test-only CLI. Normal published builds do NOT contain the
# "fixture seed-refresh" command or localhost delegated OAuth allowance.
cargo build --locked --features cloud-fixture-tests \
  --manifest-path sdk/rust/Cargo.toml --bin cmp

# Backend first starts Docker/Postgres, Firebase Auth emulator and real local
# CMP Cloud Next.js app, creates the manifest + controller and exports six
# disposable refresh grants in environment; do not paste tokens in chat.
export CMP_CLOUD_FIXTURE_MANIFEST=/absolute/disposable/fixture.json
export CMP_CLOUD_FIXTURE_CONTROL=/absolute/disposable/fixture-control
export CMP_CLOUD_FIXTURE_CLI="$(pwd)/sdk/rust/target/debug/cmp"
# If CARGO_TARGET_DIR is set, use that exact target/debug/cmp instead.

pnpm verify:cloud:fixture
```

Running `pnpm verify:cloud:fixture` without the manifest, controller and
actual feature-gated native CLI **exits nonzero with BLOCKED**. No simulated
HTTP mode or production fallback is available.

### What the real runner verifies

- Happy-path authenticated `cmp doctor` after actual refresh rotation and
  protected `/v1/repos` / selected `head`.
- Expired refresh rejection, revoked grant rejection, independent revoked
  SQL membership denial, outsider cross-tenant read denial, and reader
  commit denial without changing existing records.
- Disposable encrypted refresh grant created with real
  Argon2id/XChaCha20-Poly1305, no plaintext in the file, Unix restricted
  permissions. Fixture-only seeding **never calls the system Keychain**.
- No outbound HTTP from an offline repository while streaming disabled.
  Enabling streaming alone does not backfill older local context. A new
  commit explicitly streams to the **real fixture**.
- A one-time 503 leaves a pending upload; explicit `cmp stream push`
  succeeds when the backend returns. A divergent remote HEAD blocks
  uploads, preserves queued local edits and cannot be silently retried
  across conflict. Disabling streaming leaves pending edits untouched and
  does not upload subsequent local edits.
- The local counting HTTP proxy **forwards bytes to the real Cloud Next.js
  server**, does not synthesize success, and refuses off-loopback redirects.

## Tests: do not conflate evidence levels

**Executed without the Cloud fixture:** Node manifest security preflight,
no-fixture nonzero/blocked regression, Cloud doctor read-only Rust unit test,
feature-gated loopback/mismatched-session tests, existing Rust
encrypted-file/streaming tests, protocol snapshot and TypeScript tests,
strict Clippy, plus native macOS disposable Keychain tests when run.
These establish test harness behavior, not actual Cloud tenant isolation.

**Blocked pending Backend fixture:** all six real Cloud actor grants,
Firebase Auth/SQL membership/revocation integration, actual expired token,
least-privilege real Cloud writes, cross-tenant API denial and actual
remote streaming 503/CAS retry. The runner must not be marked PASS from
preflight, mocked HTTP, or Cloud production credentials.

## Security findings / review caveats

- Source Cloud /v1/capabilities reports global feature flags, not per-repo
  permission. Native doctor checks actual protected repository APIs.
  This PR makes read-only Cloud runtimes valid while preserving strict Core.
- A normal CMP CLI build **cannot** redirect delegated OAuth refresh
  tokens to a nonproduction origin. Only explicit Cargo feature
  `cloud-fixture-tests`, exact numeric 127.0.0.1 origin, isolated
  TMPDIR-backed `CMP_HOME`, restrictive file marker and session ID can
  enable local fixture transport. Normal builds exclude all fixture
  credential seeding. Never publish a fixture-feature binary.
- Fixture refresh grants are stored only in temporary encrypted storage;
  fixture seeding never deletes/reads real macOS Keychain entries.
  `cmp-client` PR #18 for stable macOS Keychain trust is independent,
  still requires its own merge/review and interactive UI QA. Windows
  Credential Manager behavior is not verified here.
- Cloud PR #55 remains a **draft** with separate migration/deployment
  approvals and the previously documented required CI/runner gates.
  Backend incident #61 takes priority. Do not touch/merge/deploy Cloud
  or use a non-disposable tenant to sidestep this dependency.

## HANDOFF TO MASTER

**Current decision:** review Client-only fixture harness and tests;
do not merge, publish or install. Backend + Security should produce only
the above disposable fixture and return exact route status evidence and
safe teardown, after its current priority allows. Once provided, run the
real harness and report outcomes per actor/operation, then request Master
review before any Client merge.

**Next assignment after fixture delivery:** demonstrate real Cloud
least-privilege + revoked membership/OAuth + retry/HEAD behavior with
the compiled feature-gated CLI against disposable PostgreSQL/Firebase;
then add a separate native interactive Keychain permission QA report
and confirm the disposition of PR #18.
