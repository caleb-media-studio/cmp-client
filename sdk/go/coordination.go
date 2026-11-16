package cmp

import (
	"context"
	"encoding/json"
)

// AgentSession identifies one revocable development CMP agent connection.
// Never store the SessionToken in Git or application logs.
type AgentSession struct {
	SessionID          string `json:"sessionId"`
	SessionToken       string `json:"sessionToken"`
	OrganizationID     string `json:"organizationId"`
	RepositoryID       string `json:"repositoryId"`
	CanonicalSourceKey string `json:"canonicalSourceKey"`
}
type AgentPeer struct {
	SessionID string   `json:"sessionId"`
	AgentID   string   `json:"agentId"`
	Intent    string   `json:"intent"`
	Paths     []string `json:"paths"`
}
type WorkClaim struct {
	ClaimID      string `json:"claimId"`
	Scope        string `json:"scope"`
	FencingToken int64  `json:"fencingToken"`
	ExpiresAt    string `json:"expiresAt"`
}
type ArchiveRevision struct {
	Accepted   bool     `json:"accepted"`
	Duplicate  bool     `json:"duplicate"`
	EventID    string   `json:"eventId"`
	RevisionID string   `json:"revisionId"`
	Parents    []string `json:"parents"`
	Branch     string   `json:"branch"`
}

// RPC is the shared typed-envelope transport for all agent/session methods.
func (c *Client) RPC(ctx context.Context, method string, params map[string]any) (json.RawMessage, error) {
	var response struct {
		Result json.RawMessage `json:"result"`
	}
	err := c.request(ctx, "POST", "/v1/rpc", map[string]any{"method": method, "params": params}, &response)
	return response.Result, err
}
func auth(session AgentSession) map[string]any {
	return map[string]any{"sessionId": session.SessionID, "sessionToken": session.SessionToken}
}
func (c *Client) JoinAgent(ctx context.Context, org, repo, agent, provider, providerRepo, intent string, paths []string) (AgentSession, error) {
	var session AgentSession
	data, err := c.RPC(ctx, "agent.join", map[string]any{
		"organizationId": org, "repositoryId": repo, "agentId": agent,
		"sourceRepository": map[string]string{"provider": provider, "repositoryId": providerRepo},
		"intent":           intent, "paths": paths})
	if err == nil {
		err = json.Unmarshal(data, &session)
	}
	return session, err
}
func (c *Client) ListAgents(ctx context.Context, session AgentSession) ([]AgentPeer, error) {
	data, err := c.RPC(ctx, "agent.list", auth(session))
	if err != nil {
		return nil, err
	}
	var result struct {
		Peers []AgentPeer `json:"peers"`
	}
	err = json.Unmarshal(data, &result)
	return result.Peers, err
}
func (c *Client) AgentEvents(ctx context.Context, session AgentSession, after string) (json.RawMessage, error) {
	p := auth(session)
	p["after"] = after
	return c.RPC(ctx, "agent.events", p)
}
func (c *Client) PublishIntent(ctx context.Context, session AgentSession, intent string, paths []string) error {
	p := auth(session)
	p["intent"] = intent
	p["paths"] = paths
	_, err := c.RPC(ctx, "agent.intent.publish", p)
	return err
}
func (c *Client) ProposeContract(ctx context.Context, session AgentSession, objective string,
	participants []string, scopes map[string][]string) (json.RawMessage, error) {
	p := auth(session)
	p["objective"] = objective
	p["participants"] = participants
	p["ownershipScopes"] = scopes
	return c.RPC(ctx, "coordination.propose", p)
}
func (c *Client) AcceptContract(ctx context.Context, session AgentSession, id string, revision int) error {
	p := auth(session)
	p["contractId"] = id
	p["revision"] = revision
	_, err := c.RPC(ctx, "coordination.accept", p)
	return err
}
func (c *Client) AcquireClaim(ctx context.Context, session AgentSession, contractID, scope string) (WorkClaim, error) {
	p := auth(session)
	p["contractId"] = contractID
	p["scope"] = scope
	data, err := c.RPC(ctx, "coordination.claim.acquire", p)
	var claim WorkClaim
	if err == nil {
		err = json.Unmarshal(data, &claim)
	}
	return claim, err
}
func (c *Client) OpenArchive(ctx context.Context, session AgentSession, source string, enabled, exclude bool) (json.RawMessage, error) {
	p := auth(session)
	p["source"] = source
	p["archiveEnabled"] = enabled
	p["exclude"] = exclude
	return c.RPC(ctx, "session.open", p)
}
func (c *Client) AppendArchive(ctx context.Context, session AgentSession, event map[string]any) (ArchiveRevision, error) {
	p := auth(session)
	for k, v := range event {
		p[k] = v
	}
	data, err := c.RPC(ctx, "session.append", p)
	var result ArchiveRevision
	if err == nil {
		err = json.Unmarshal(data, &result)
	}
	return result, err
}
func (c *Client) DeleteArchive(ctx context.Context, session AgentSession) error {
	_, err := c.RPC(ctx, "session.delete", auth(session))
	return err
}
func (c *Client) ExcludeArchive(ctx context.Context, session AgentSession) error {
	_, err := c.RPC(ctx, "session.exclude", auth(session))
	return err
}
