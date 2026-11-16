# Native CMP CLI (Rust)

Build from sdk/rust with cargo build --release --bin cmp. One Rust executable runs on macOS, Windows and Linux; the package is not published to registries yet.

## Config paths

- Global: ~/.cmp/config.toml, inside ~/.cmp/
- Project: PROJECT_ROOT/.cmp/config.toml, inside that project's .cmp/
- Project overrides global. CMP_CONFIG chooses an explicit file. CMP_ENDPOINT and CMP_REPOSITORY are opt-in environment overrides.
- Both TOML files contain only the endpoint, backend, repository and branch. Refresh credentials are stored in the operating system secure credential vault, never in a tracked project.
- Self-hosted mode accepts loopback HTTP or HTTPS. It currently uses CMP_DEV_TOKEN for the local development server, not a CMP Cloud login. Unencrypted remote HTTP is rejected.

## Interactive onboarding

Running CMP with an interactive terminal opens GitHub-style colored selection menus.
Use **Up/Down arrows**, **Enter** to select, and **Escape** to cancel choices and confirmations. Use **Ctrl-C** to stop the wizard.
Choice-based questions no longer require typing their names:

- Global (~/.cmp) or current-project (./.cmp) setup
- CMP Cloud or self-hosted CMP server
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

cmp setup --scope global|project --backend cloud|local --endpoint URL --repo owner/name
cmp login [--no-browser]
cmp logout
cmp auth status
cmp init --repo owner/name [--create] [--visibility private|public] [--offline]
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

## Browser login and connected apps

CMP CLI binds 127.0.0.1 on an OS-assigned ephemeral port, generates PKCE S256 and random state, and opens CMP Cloud authorization in the browser. The signed-in user explicitly approves read/write/offline scopes. Cloud issues one-time codes, short-lived opaque access tokens and rotating refresh tokens. Tokens are validated by the managed API against the existing RLS-protected PostgreSQL database and never authorize browser account/billing/admin endpoints.

CMP Cloud Settings > Connected apps displays and revokes authorized sessions. Tokens never enter configuration files or MCP host commands. No extra always-on infrastructure was created.

## MCP and agent integration

CMP MCP runs through a JSON-RPC stdio adapter, with tools for repository listing, context head/snapshot, diff, CAS commit, three-way merge and branch creation. Tools use the active authenticated backend. Host setup uses the supported Claude Code project MCP command or project-local Codex TOML/OpenCode JSON. Existing CMP entries are not overwritten. Config removal targets only CMP's own entries; a modified entry is left untouched.

The companion SDK currently captures encrypted development-agent sessions. CMP Cloud session ingestion and automatic hooks are not available; the Rust init wizard does not pretend to install them. Docs import currently creates a source list and a reviewed import prompt. It does not upload documentation or auto-commit unsupported assertions.

## Safety and validation

The CLI uses atomic configuration writes, refuses symlinked config targets, enforces HTTPS except loopback, requires explicit scope on noninteractive setup, and refuses to replace an existing project config without explicit force. It scans bounded documentation extensions, excludes secrets/generated folders and symlink escapes, and does not upload files without review.

Rust unit tests and MCP protocol smoke tests run via Cargo. Cloud OAuth has independent Firebase emulator and restricted-role PostgreSQL end-to-end coverage. Host-specific installation should also be checked in live Claude Code/Codex/OpenCode versions before marking a platform release stable.
