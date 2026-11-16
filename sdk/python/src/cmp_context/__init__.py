"""CMP development client. HTTP operations use only the Python standard library."""
import json
from urllib.request import Request, urlopen
from urllib.error import HTTPError
from urllib.parse import quote

class CMPError(Exception):
    pass

from urllib.request import build_opener, HTTPRedirectHandler

class _RejectTransportRedirects(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise CMPError("CMP transport redirects are not permitted")

def cmp_transport_path(repository: str) -> str:
    parts = repository.split("/")
    if len(parts) != 2 or any(not part or len(part) > 100 or part in (".", "..")
        or not all(ch.isascii() and (ch.isalnum() or ch in "._-") for ch in part)
        for part in parts):
        raise CMPError("CMP repository must have owner/name format")
    return f"/{quote(parts[0], safe='')}/{quote(parts[1], safe='')}.cmp"

class CMPClient:
    def __init__(self, endpoint: str, token: str):
        self.endpoint = endpoint.rstrip("/")
        self.token = token

    def _request(self, path: str, method: str = "GET", body=None):
        payload = None if body is None else json.dumps(body).encode("utf-8")
        req = Request(
            self.endpoint + path,
            method=method,
            data=payload,
            headers={"Authorization": f"Bearer {self.token}", "Content-Type": "application/json"},
        )
        try:
            with urlopen(req, timeout=15) as res:
                return json.load(res)
        except HTTPError as error:
            raise CMPError(f"CMP HTTP {error.code}: {error.read().decode()}") from error

    def _cmp_transport(self, path: str, method: str = "GET", body=None):
        payload = None if body is None else json.dumps(body).encode("utf-8")
        req = Request(self.endpoint + path, method=method, data=payload,
                      headers={"Authorization": f"Bearer {self.token}",
                               "Content-Type": "application/json"})
        try:
            with build_opener(_RejectTransportRedirects()).open(req, timeout=15) as response:
                return json.load(response)
        except HTTPError as error:
            raise CMPError(f"CMP transport HTTP {error.code}") from error

    def cmp_manifest(self, repository: str):
        return self._cmp_transport(cmp_transport_path(repository))

    def cmp_refs(self, repository: str):
        return self._cmp_transport(cmp_transport_path(repository) + "?view=refs")

    def cmp_snapshot(self, repository: str, ref: str = "main"):
        if not ref or len(ref) > 128:
            raise CMPError("Invalid CMP snapshot reference")
        return self._cmp_transport(cmp_transport_path(repository) +
                                   f"?view=snapshot&ref={quote(ref, safe='')}")

    def cmp_push(self, repository: str, payload: dict):
        return self._cmp_transport(cmp_transport_path(repository), "POST", payload)

    def list_repositories(self):
        return self._request("/v1/repos")

    def create_repository(self, name: str):
        return self._request("/v1/repos", "POST", {"id": name})

    def head(self, repo: str, branch: str = "main"):
        return self._request(f"/v1/repos/{quote(repo, safe='')}/head?branch={quote(branch, safe='')}")

    def snapshot(self, repo: str, ref: str = "main"):
        return self._request(f"/v1/repos/{quote(repo, safe='')}/records?ref={quote(ref, safe='')}")

    def branch(self, repo: str, name: str, from_ref: str = "main"):
        return self._request(f"/v1/repos/{quote(repo, safe='')}/branches", "POST", {"name": name, "from": from_ref})

    def commit(self, repo: str, branch: str, expected_head: str, author_id: str, message: str, changes: list):
        return self._request(f"/v1/repos/{quote(repo, safe='')}/commits", "POST", {
            "branch": branch, "expectedHead": expected_head, "authorId": author_id,
            "message": message, "changes": changes
        })

    def merge(self, repo: str, source: str, target: str, author_id: str):
        return self._request(f"/v1/repos/{quote(repo, safe='')}/merge", "POST", {
            "source": source, "target": target, "authorId": author_id
        })


    def rpc(self, method: str, params: dict | None = None):
        """Call a versioned CMP method through authenticated HTTP RPC."""
        return self._request("/v1/rpc", "POST",
                             {"method": method, "params": params or {}})["result"]

    def join_agent(self, organization_id: str, repository_id: str, agent_id: str,
                   provider: str, provider_repo_id: str, intent: str = "", paths=None):
        return self.rpc("agent.join", {
            "organizationId": organization_id, "repositoryId": repository_id,
            "agentId": agent_id, "sourceRepository": {
                "provider": provider, "repositoryId": provider_repo_id},
            "intent": intent, "paths": paths or []})

    @staticmethod
    def _credentials(session: dict):
        return {"sessionId": session["sessionId"], "sessionToken": session["sessionToken"]}

    def list_agents(self, session: dict):
        return self.rpc("agent.list", self._credentials(session))

    def agent_events(self, session: dict, after: str = "0"):
        return self.rpc("agent.events", {**self._credentials(session), "after": after})

    def publish_intent(self, session: dict, intent: str, paths=None):
        return self.rpc("agent.intent.publish", {
            **self._credentials(session), "intent": intent, "paths": paths or []})

    def propose_contract(self, session: dict, objective: str, participants: list, scopes: dict):
        return self.rpc("coordination.propose", {
            **self._credentials(session), "objective": objective, "participants": participants,
            "ownershipScopes": scopes})

    def accept_contract(self, session: dict, contract_id: str, revision: int):
        return self.rpc("coordination.accept", {
            **self._credentials(session), "contractId": contract_id, "revision": revision})

    def acquire_claim(self, session: dict, contract_id: str, scope: str):
        return self.rpc("coordination.claim.acquire", {
            **self._credentials(session), "contractId": contract_id, "scope": scope})

    def open_archive(self, session: dict, source: str, enabled: bool, exclude: bool = False):
        return self.rpc("session.open", {
            **self._credentials(session), "source": source,
            "archiveEnabled": enabled, "exclude": exclude})

    def append_archive(self, session: dict, event: dict):
        return self.rpc("session.append", {**self._credentials(session), **event})

    def exclude_archive(self, session: dict):
        return self.rpc("session.exclude", self._credentials(session))

    def delete_archive(self, session: dict):
        return self.rpc("session.delete", self._credentials(session))

    def heartbeat_agent(self, session: dict, ttl_ms: int = 60000):
        return self.rpc("agent.heartbeat", {**self._credentials(session), "ttlMs": ttl_ms})

    def leave_agent(self, session: dict):
        return self.rpc("agent.leave", self._credentials(session))

    def send_message(self, session: dict, message: str, to_session_id: str | None = None):
        return self.rpc("coordination.message", {**self._credentials(session),
                         "message": message, "toSessionId": to_session_id})

    def revise_contract(self, session: dict, contract_id: str, objective: str, scopes: dict):
        return self.rpc("coordination.revise", {**self._credentials(session),
                        "contractId": contract_id, "objective": objective, "ownershipScopes": scopes})

    def reject_contract(self, session: dict, contract_id: str):
        return self.rpc("coordination.reject", {**self._credentials(session), "contractId": contract_id})

    def get_contract(self, session: dict, contract_id: str):
        return self.rpc("coordination.contract.get", {**self._credentials(session), "contractId": contract_id})

    def renew_claim(self, session: dict, claim_id: str, fencing_token: int, duration_ms: int = 30000):
        return self.rpc("coordination.claim.renew", {**self._credentials(session),
                        "claimId": claim_id, "fencingToken": fencing_token, "durationMs": duration_ms})

    def release_claim(self, session: dict, claim_id: str, fencing_token: int):
        return self.rpc("coordination.claim.release", {**self._credentials(session),
                        "claimId": claim_id, "fencingToken": fencing_token})

    def propose_change(self, session: dict, claim_id: str, fencing_token: int,
                       path: str, git_revision: str):
        return self.rpc("coordination.change.propose", {**self._credentials(session),
                        "claimId": claim_id, "fencingToken": fencing_token,
                        "path": path, "gitRevision": git_revision})

    def archive_policy(self, session: dict):
        return self.rpc("session.policy.get", self._credentials(session))

    def checkpoint_archive(self, session: dict, event: dict):
        return self.rpc("session.checkpoint", {**self._credentials(session), **event})

    def archive_status(self, session: dict):
        return self.rpc("session.get", self._credentials(session))

    def archive_events(self, session: dict, include_raw: bool = False):
        return self.rpc("session.events", {**self._credentials(session), "includeRaw": include_raw})

    def close_archive(self, session: dict):
        return self.rpc("session.close", self._credentials(session))

    def resume_agent(self, prior_session: dict, organization_id: str, repository_id: str,
                     agent_id: str, provider: str, provider_repo_id: str):
        return self.rpc("agent.join", {
            "organizationId": organization_id, "repositoryId": repository_id, "agentId": agent_id,
            "sourceRepository": {"provider": provider, "repositoryId": provider_repo_id},
            "resumeSessionId": prior_session["sessionId"],
            "resumeSessionToken": prior_session["sessionToken"]})

    async def events(self, repo: str, after: str = "0"):
        """Requires installation of cmp-context[stream]."""
        try:
            from websockets.asyncio.client import connect
        except ImportError as error:
            raise CMPError("Install cmp-context[stream] to enable native WSS") from error
        import urllib.parse
        ticket = self._request("/v1/ws-ticket", "POST", {})["ticket"]
        base = self.endpoint.replace("https://", "wss://").replace("http://", "ws://")
        url = base + "/v1/cmp/connect?ticket=" + urllib.parse.quote(ticket)
        async with connect(url, max_size=262144) as ws:
            await ws.send(json.dumps({
                "v": 1, "id": "subscribe-1", "type": "request",
                "method": "context.subscribe", "params": {"repo": repo, "after": after}
            }))
            async for raw in ws:
                frame = json.loads(raw)
                if frame.get("type") == "event":
                    yield frame
