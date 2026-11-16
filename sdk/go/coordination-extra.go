package cmp

import (
	"context"
	"encoding/json"
	"errors"
)

// ScopedRPC binds this CMP operation to the agent session credential managed
// by the caller. The caller-supplied parameters cannot replace that identity.
func (c *Client) ScopedRPC(ctx context.Context, session AgentSession,
	method string, params map[string]any) (json.RawMessage, error) {
	scoped := auth(session)
	for key, value := range params {
		if key == "sessionId" || key == "sessionToken" {
			return nil, errors.New("CMP session credentials must not be overridden")
		}
		scoped[key] = value
	}
	return c.RPC(ctx, method, scoped)
}
func (c *Client) HeartbeatAgent(ctx context.Context, s AgentSession, ttlMs int) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "agent.heartbeat", map[string]any{"ttlMs": ttlMs})
}
func (c *Client) LeaveAgent(ctx context.Context, s AgentSession) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "agent.leave", nil)
}
func (c *Client) SendCoordinationMessage(ctx context.Context, s AgentSession,
	message, toSessionID string) (json.RawMessage, error) {
	params := map[string]any{"message": message}
	if toSessionID != "" {
		params["toSessionId"] = toSessionID
	}
	return c.ScopedRPC(ctx, s, "coordination.message", params)
}
func (c *Client) ReviseContract(ctx context.Context, s AgentSession, id, objective string,
	scopes map[string][]string) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "coordination.revise", map[string]any{
		"contractId": id, "objective": objective, "ownershipScopes": scopes})
}
func (c *Client) RejectContract(ctx context.Context, s AgentSession, id string) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "coordination.reject", map[string]any{"contractId": id})
}
func (c *Client) GetContract(ctx context.Context, s AgentSession, id string) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "coordination.contract.get", map[string]any{"contractId": id})
}
func (c *Client) RenewClaim(ctx context.Context, s AgentSession, id string, fence int64, durationMs int) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "coordination.claim.renew", map[string]any{
		"claimId": id, "fencingToken": fence, "durationMs": durationMs})
}
func (c *Client) ReleaseClaim(ctx context.Context, s AgentSession, id string, fence int64) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "coordination.claim.release", map[string]any{
		"claimId": id, "fencingToken": fence})
}
func (c *Client) ProposeChange(ctx context.Context, s AgentSession, id string, fence int64,
	path, gitRevision string) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "coordination.change.propose", map[string]any{
		"claimId": id, "fencingToken": fence, "path": path, "gitRevision": gitRevision})
}
func (c *Client) ArchivePolicy(ctx context.Context, s AgentSession) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "session.policy.get", nil)
}
func (c *Client) ArchiveStatus(ctx context.Context, s AgentSession) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "session.get", nil)
}
func (c *Client) ArchiveEvents(ctx context.Context, s AgentSession, includeRaw bool) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "session.events", map[string]any{"includeRaw": includeRaw})
}
func (c *Client) CheckpointArchive(ctx context.Context, s AgentSession, event map[string]any) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "session.checkpoint", event)
}
func (c *Client) CloseArchive(ctx context.Context, s AgentSession) (json.RawMessage, error) {
	return c.ScopedRPC(ctx, s, "session.close", nil)
}
func (c *Client) ResumeAgent(ctx context.Context, session AgentSession,
	org, repo, agent, provider, providerRepo string) (AgentSession, error) {
	data, err := c.RPC(ctx, "agent.join", map[string]any{
		"organizationId": org, "repositoryId": repo, "agentId": agent,
		"sourceRepository": map[string]any{"provider": provider, "repositoryId": providerRepo},
		"resumeSessionId":  session.SessionID, "resumeSessionToken": session.SessionToken,
	})
	var resumed AgentSession
	if err == nil {
		err = json.Unmarshal(data, &resumed)
	}
	return resumed, err
}

// Development-server policy administrator only. Production identity must add RBAC.
func (c *Client) ConfigureArchivePolicy(ctx context.Context, org, repo string, enabled, raw bool, retentionDays int) (json.RawMessage, error) {
	return c.RPC(ctx, "session.policy.set", map[string]any{
		"organizationId": org, "repositoryId": repo, "enabled": enabled, "rawEnabled": raw,
		"retentionDays": retentionDays,
	})
}
