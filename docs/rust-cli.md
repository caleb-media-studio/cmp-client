# Native CMP CLI (Rust)

Build from sdk/rust with cargo build --release --bin cmp. One Rust executable runs on macOS, Windows and Linux; the package is not published to registries yet.

## Config paths

- Global: ~/.cmp/config.toml, inside ~/.cmp/
- Project: PROJECT_ROOT/.cmp/config.toml, inside that project's .cmp/
- Project overrides global. CMP_CONFIG chooses an explicit file. CMP_ENDPOINT and CMP_REPOSITORY are opt-in environment overrides.
- TOML stores backend, endpoint, repository, branch and the explicit `streaming` boolean. The offline store lives at `<config-directory>/state/`, not in Git metadata.
- Refresh tokens never live in TOML. After OAuth, the user chooses an OS credential vault, a passphrase-encrypted file (`~/.cmp/credentials.enc`), or no persistence. The encrypted fallback uses Argon2id and XChaCha20-Poly1305; the decryption password is never stored in the project.
- Self-hosted mode accepts loopback HTTP or HTTPS. It currently uses CMP_DEV_TOKEN for the local development server, not a CMP Cloud login. Unencrypted remote HTTP is rejected.

## Interactive onboarding

Running CMP with an interactive terminal opens GitHub-style colored selection menus.
Use **Up/Down arrows**, **Enter** to select, and **Escape** to cancel choices and confirmations. Use **Ctrl-C** to stop the wizard.
Choice-based questions no longer require typing their names:

- Global (~/.cmp) or current-project (./.cmp) setup
- CMP Cloud, self-hosted CMP server or embedded local/offline storage, with an explicit streaming opt-in
- Selecting from accessible context repositories
- Offering to create a missing repository, then private or public visibility
- Selecting Claude Code, Codex, or OpenCode for MCP installation

Repository names and custom server URLs are still text inputs. Automation retains all
previous --no-interactive flags and explicitly selected option values. Running
cmp setup --scope project and then cmp init is safe and does not require --force
when the existing settings match.

CMP always writes .cmp/config.toml for its project metadata. It never creates
.git/ or requires a git init. Developers may independently maintain a Git repository
in the same directory if desired.

## Implemented commands

cmp setup --scope global|project --backend cloud|local|offline --endpoint URL --repo owner/name [--streaming|--no-streaming]
cmp login [--no-browser] [--credential-store keychain|encrypted|none]
cmp logout
cmp auth status
cmp init --repo owner/name [--create] [--visibility private|public] [--offline] [--streaming|--no-streaming]
cmp stream status
cmp stream push
cmp status
cmp doctor [--offline]
cmp config show
cmp repo list
cmp repo create owner/name [--visibility private|public]
cmp head [branch]
cmp snapshot [ref]
cmp branch create name [from]
cmp commit changes.json
cmp diff [from] [to]
cmp merge source [target]
cmp mcp serve
cmp agent list
cmp agent install claude|codex|opencode
cmp agent remove claude|codex|opencode

The CMP repository is versioned context and evidence, not the same as a Git repository. Commit uses a bounded JSON file and an optimistic branch-head check. Repository creation defaults to private.

## Embedded offline context and streaming privacy

Choose `cmp setup --scope project --backend offline --no-interactive` (or **Local / Offline** from the interactive menu) to keep context records, branch heads and immutable snapshots on-device in `.cmp/state/`. Context commands and `cmp mcp serve` operate against this embedded store without a server. Offline merges currently support fast-forward ancestry only. The CLI never creates `.git/`.

No network request is made for context operations when `streaming = false`. The CLI creates `.cmp/.gitignore` with `state/` to help protect local records from accidental Git commits. **Local records are owner-private on Unix but not encrypted at rest**. Never put credentials or private secrets in context records.

Streaming is opt-in in `cmp setup` or `cmp init`, or via `--streaming`. When enabled, new local changes are saved before an authenticated upload attempt. Failed operations remain in `.cmp/state/stream-outbox.json`; run `cmp stream status` or `cmp stream push` to inspect and retry. Known remote branch divergence stops uploads instead of overwriting remote changes. `--no-streaming` disables further uploads.

**Current limitation:** enabling streaming does not retroactively backfill earlier offline context history. That requires an explicit migration flow. CMP Cloud uses `cmp login`; self-hosted development servers use `CMP_DEV_TOKEN`. There is no background synchronization daemon when the CLI is closed.

## Browser login and connected apps

CMP CLI binds 127.0.0.1 on an OS-assigned ephemeral port, generates PKCE S256 and random state, and opens CMP Cloud authorization in the browser. The signed-in user explicitly approves read/write/offline scopes. Cloud issues one-time codes, short-lived opaque access tokens and rotating refresh tokens. Tokens are validated by the managed API against the existing RLS-protected PostgreSQL database and never authorize browser account/billing/admin endpoints.

CMP Cloud Settings > Connected apps displays and revokes authorized sessions. After the OAuth callback returns, the CLI asks **"Would you like to save in keychain?"**. On macOS, CMP reads and updates the Keychain through Apple’s signed `/usr/bin/security` utility, so **Always Allow** persists across ad-hoc CMP CLI rebuilds. Older native-keyring entries can be reused without another login, though macOS may require one approval for `/usr/bin/security` when switching. This follows GitHub CLI’s approach but carries the same security tradeoff: other local processes can invoke that trusted system utility. Choose encrypted-file storage if you prefer narrower access. Choose a passphrase-encrypted file when a system credential vault is unsupported or unavailable, or choose no persistence. For noninteractive login, explicitly select `--credential-store encrypted|keychain|none`; otherwise persistence defaults to none. When encrypted-file storage is selected, normal interactive CLI commands prompt for its passphrase. Headless MCP integrations require `CMP_CREDENTIAL_PASSPHRASE` in the parent process environment and must not store it in tracked configuration or MCP command arguments. Tokens never enter project configuration. No extra always-on infrastructure was created.

## MCP and agent integration

CMP MCP runs through a JSON-RPC stdio adapter, with tools for repository listing, context head/snapshot, diff, CAS commit, three-way merge and branch creation. Tools use the active authenticated backend. Host setup uses the supported Claude Code project MCP command or project-local Codex TOML/OpenCode JSON. Existing CMP entries are not overwritten. Config removal targets only CMP's own entries; a modified entry is left untouched.

The companion SDK currently captures encrypted development-agent sessions. CMP Cloud session ingestion and automatic hooks are not available; the Rust init wizard does not pretend to install them. Docs import currently creates a source list and a reviewed import prompt. It does not upload documentation or auto-commit unsupported assertions.

## Safety and validation

The CLI uses atomic configuration writes, refuses symlinked config targets, enforces HTTPS except loopback, requires explicit scope on noninteractive setup, and refuses to replace an existing project config without explicit force. It scans bounded documentation extensions, excludes secrets/generated folders and symlink escapes, and does not upload files without review.

Rust unit tests and MCP protocol smoke tests run via Cargo. Cloud OAuth has independent Firebase emulator and restricted-role PostgreSQL end-to-end coverage. Host-specific installation should also be checked in live Claude Code/Codex/OpenCode versions before marking a platform release stable.

## GitHub CLI-style CMP commands and .cmp transport (v1)

CMP uses familiar nested commands and help categories, but **CMP is versioned context**, not Git source control. CMP does not implement Git clone, Git packfiles, Git checkouts, GitHub issues, or GitHub Actions.

```bash
cmp --help
cmp help repo
cmp repo --help
cmp auth login
cmp auth status
cmp repo create owner/repo --private
cmp repo list
cmp repo view owner/repo
cmp repo view owner/repo --web
cmp repo clone https://cloud.cmp.chat/owner/repo.cmp
cmp repo set-default owner/repo
cmp branch list
cmp config list
cmp config get branch
cmp config set branch feature
cmp api /v1/repos
```

The `.cmp` URL is the canonical CMP repository transport identity. The v1 JSON endpoint supports manifest, refs, snapshot and CAS commit operations. SDKs should validate the same origin before using a remote URL. The Rust SDK refuses bearer-token redirects to external hosts.

**Clone is snapshot-only:** it creates a new directory containing `.cmp/config.toml` and a local `.cmp/state` snapshot and records the remote head. It does not create `.git/`, does not rewrite existing files, does not upload anything, does not clone full commit ancestry and **leaves streaming off**. The user can enable remote streaming later through `cmp config set streaming true`; remote-head checks still apply.

The Core reference server uses `/local/repo.cmp` for its flat, development-only namespace, while CMP Cloud uses `/owner/repo.cmp`. The proprietary Cloud edition requires its matching route to be deployed. A returned 404 before Cloud deployment is not proof the repository is missing.

Non-CMP GitHub commands, including issue, pull request, Codespaces, workflow, release, and GitHub organization management, are not aliases for CMP repository operations and are intentionally not advertised as supported until Cloud services exist.
