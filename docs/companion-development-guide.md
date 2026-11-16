# CMP Cloud agent companion development

The CMP Companion is a client-side host integration, not an agent
coordination or archive server. It may be open-source while the Cloud
backend remains proprietary.

Supported development adapters include Claude Code, Codex and OpenCode
host events. Agents communicate with CMP Cloud through optional documented
Cloud API requests and streams. The self-hosted CMP Core Server has no
WebSocket, agent presence, work contracts or session archiving.

Session capture must remain disabled unless the Cloud organization policy
and local host settings both authorize capture. A user exclusion wins.
Never put the Cloud access token or agent session key in model-generated
programs, checked-in project config or logs.

Do not use the old local-server agent coordination test suite as proof
that these capabilities are deployed in CMP Cloud. Require an actual
Cloud E2E test, including opt-in, access control, replay, deletion and
negative cross-tenant testing.
