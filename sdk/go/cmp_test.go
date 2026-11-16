package cmp

import (
 "context"
 "net/http"
 "net/http/httptest"
 "strings"
 "testing"
)

func TestClientAuthAndList(t *testing.T){
 server:=httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter,r *http.Request){
  if r.Header.Get("Authorization")!="Bearer test-token"{t.Error("authorization missing")}
  if r.URL.Path!="/v1/repos"{t.Error("wrong path")}
  w.Header().Set("Content-Type","application/json")
  w.Write([]byte(`{"repositories":["grydbase"]}`))
 }))
 defer server.Close()
 c:=NewClient(server.URL,"test-token")
 repos,err:=c.ListRepositories(context.Background())
 if err!=nil||len(repos)!=1||repos[0]!="grydbase"{t.Fatalf("result %v error %v",repos,err)}
}
func TestErrorResponse(t *testing.T){
 server:=httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter,r *http.Request){http.Error(w,"conflict",409)}))
 defer server.Close()
 _,err:=NewClient(server.URL,"test").Head(context.Background(),"repo","main")
 if err==nil||!strings.Contains(err.Error(),"409"){t.Fatalf("expected 409, got %v",err)}
}
