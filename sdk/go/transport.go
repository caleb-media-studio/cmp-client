package cmp

import (
	"context"
	"errors"
	"net/url"
	"strings"
)

// TransportPath identifies a CMP context repository, not a Git repository.
// A CMP repository URL is /owner/repository.cmp.
func TransportPath(repo string) (string, error) {
	parts := strings.Split(repo, "/")
	if len(parts) != 2 {
		return "", errors.New("CMP repository must be owner/name")
	}
	for _, part := range parts {
		if len(part) == 0 || len(part) > 100 || part == "." || part == ".." {
			return "", errors.New("invalid CMP repository")
		}
		for _, b := range []byte(part) {
			if !((b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9') || b == '-' || b == '_' || b == '.') {
				return "", errors.New("invalid CMP repository")
			}
		}
	}
	return "/" + url.PathEscape(parts[0]) + "/" + url.PathEscape(parts[1]) + ".cmp", nil
}

type TransportManifest struct {
	Protocol         string `json:"protocol"`
	TransportVersion int    `json:"transportVersion"`
	Repository       string `json:"repository"`
	DefaultBranch    string `json:"defaultBranch"`
	Head             string `json:"head"`
	Capabilities     struct {
		Refs     bool `json:"refs"`
		Snapshot bool `json:"snapshot"`
		Commit   bool `json:"commit"`
	} `json:"capabilities"`
	Links map[string]string `json:"links"`
}
type TransportSnapshot struct {
	Repository string   `json:"repository"`
	Ref        string   `json:"ref"`
	Head       string   `json:"head"`
	Records    []Record `json:"records"`
}

func (c *Client) CMPManifest(ctx context.Context, repo string) (TransportManifest, error) {
	var result TransportManifest
	path, err := TransportPath(repo)
	if err != nil {
		return result, err
	}
	err = c.request(ctx, "GET", path, nil, &result)
	return result, err
}
func (c *Client) CMPRefs(ctx context.Context, repo string) (map[string]string, error) {
	path, err := TransportPath(repo)
	if err != nil {
		return nil, err
	}
	var result struct {
		Branches map[string]string `json:"branches"`
	}
	err = c.request(ctx, "GET", path+"?view=refs", nil, &result)
	return result.Branches, err
}
func (c *Client) CMPSnapshot(ctx context.Context, repo, ref string) (TransportSnapshot, error) {
	var result TransportSnapshot
	path, err := TransportPath(repo)
	if err != nil {
		return result, err
	}
	if len(ref) == 0 || len(ref) > 128 {
		return result, errors.New("invalid CMP reference")
	}
	err = c.request(ctx, "GET", path+"?view=snapshot&ref="+url.QueryEscape(ref), nil, &result)
	return result, err
}
func (c *Client) CMPPush(ctx context.Context, repo string, payload CommitRequest) (Commit, error) {
	var result Commit
	path, err := TransportPath(repo)
	if err != nil {
		return result, err
	}
	err = c.request(ctx, "POST", path, payload, &result)
	return result, err
}
