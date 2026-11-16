# CMP PHP SDK

**Status: planned, not implemented or published.**

Official priority: Phase 2. Package identifier (proposed): `cmpchat/sdk`.
Runtime baseline: PHP 8.3+.
Initial transport: HTTP RPC with canonical CMP wire JSON; WebSocket where supported.
No placeholder class may be promoted as a functional SDK. Implement the shared conformance fixtures first, then package tests, secure authentication/token management, retries and idempotent archive deletion.
Required release checks: phpunit, composer-validate, live-server.
See [support-matrix.json](../support-matrix.json) and [Companion development guide](../../docs/companion-development-guide.md).
