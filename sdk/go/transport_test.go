package cmp

import "testing"

func TestCMPTransportPath(t *testing.T) {
	path, err := TransportPath("team/repo-first")
	if err != nil || path != "/team/repo-first.cmp" {
		t.Fatalf("bad path %s %v", path, err)
	}
	for _, value := range []string{"team/../repo", "team/repo/a", "team/.", "/repo", "team/.."} {
		if _, err := TransportPath(value); err == nil {
			t.Fatalf("accepted %s", value)
		}
	}
}
