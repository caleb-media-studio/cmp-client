// Package cmp provides the development CMP HTTP and WebSocket client.
package cmp

import (
 "bytes"
 "context"
 "encoding/json"
 "errors"
 "fmt"
 "io"
 "net/http"
 "net/url"
 "strings"
 "time"

 "github.com/gorilla/websocket"
)

type Client struct {
 Endpoint string
 Token string
 HTTP *http.Client
}

func NewClient(endpoint,token string)*Client {
 return &Client{Endpoint:strings.TrimSuffix(endpoint,"/"),Token:token,HTTP:&http.Client{Timeout:15*time.Second}}
}
type Record struct {
 ID string `json:"id"`
 Kind string `json:"kind"`
 Content string `json:"content"`
 SourceRefs []string `json:"sourceRefs"`
 Status string `json:"status"`
 AuthorID string `json:"authorId"`
 UpdatedAt string `json:"updatedAt"`
}
type Change struct {
 Op string `json:"op"`
 Record *Record `json:"record,omitempty"`
 ID string `json:"id,omitempty"`
}
type CommitRequest struct {
 Branch string `json:"branch"`
 ExpectedHead string `json:"expectedHead"`
 AuthorID string `json:"authorId"`
 Message string `json:"message"`
 Changes []Change `json:"changes"`
}
type Commit struct {
 ID string `json:"id"`
 Parents []string `json:"parents"`
 Branch string `json:"branch"`
 Message string `json:"message"`
}
type Frame struct {
 V int `json:"v"`
 ID string `json:"id"`
 Type string `json:"type"`
 Method string `json:"method,omitempty"`
 Params json.RawMessage `json:"params,omitempty"`
 Result json.RawMessage `json:"result,omitempty"`
 Cursor string `json:"cursor,omitempty"`
}

func (c *Client) request(ctx context.Context,method,path string,body any,out any)error {
 var payload io.Reader
 if body!=nil {
   data,err:=json.Marshal(body);if err!=nil{return err}
   payload=bytes.NewReader(data)
 }
 req,err:=http.NewRequestWithContext(ctx,method,c.Endpoint+path,payload)
 if err!=nil{return err}
 req.Header.Set("Authorization","Bearer "+c.Token)
 req.Header.Set("Content-Type","application/json")
 resp,err:=c.HTTP.Do(req);if err!=nil{return err}
 defer resp.Body.Close()
 if resp.StatusCode>=300 {
   raw,_:=io.ReadAll(io.LimitReader(resp.Body,4096))
   return fmt.Errorf("cmp http %d: %s",resp.StatusCode,string(raw))
 }
 return json.NewDecoder(io.LimitReader(resp.Body,1024*1024)).Decode(out)
}
func (c *Client) ListRepositories(ctx context.Context)([]string,error){
 var result struct {Repositories []string `json:"repositories"`}
 err:=c.request(ctx,"GET","/v1/repos",nil,&result);return result.Repositories,err
}
func (c *Client) CreateRepository(ctx context.Context,id string)(string,error){
 var result struct{Head string `json:"head"`}
 err:=c.request(ctx,"POST","/v1/repos",map[string]string{"id":id},&result);return result.Head,err
}
func (c *Client) Head(ctx context.Context,repo,branch string)(string,error){
 var result struct{Head string `json:"head"`}
 err:=c.request(ctx,"GET","/v1/repos/"+url.PathEscape(repo)+"/head?branch="+url.QueryEscape(branch),nil,&result)
 return result.Head,err
}
func (c *Client) Snapshot(ctx context.Context,repo,ref string)([]Record,error){
 var result struct{Records []Record `json:"records"`}
 err:=c.request(ctx,"GET","/v1/repos/"+url.PathEscape(repo)+"/records?ref="+url.QueryEscape(ref),nil,&result)
 return result.Records,err
}
func (c *Client) Commit(ctx context.Context,repo string,change CommitRequest)(Commit,error){
 var result Commit
 err:=c.request(ctx,"POST","/v1/repos/"+url.PathEscape(repo)+"/commits",change,&result)
 return result,err
}
func (c *Client) CreateBranch(ctx context.Context,repo,name,from string)(string,error){
 var result struct{Head string `json:"head"`}
 err:=c.request(ctx,"POST","/v1/repos/"+url.PathEscape(repo)+"/branches",map[string]string{"name":name,"from":from},&result)
 return result.Head,err
}
func (c *Client) Merge(ctx context.Context,repo,source,target,author string)(Commit,error){
 var result Commit
 err:=c.request(ctx,"POST","/v1/repos/"+url.PathEscape(repo)+"/merge",map[string]string{"source":source,"target":target,"authorId":author},&result)
 return result,err
}

// Subscribe opens a native CMP WebSocket stream using a one-time ticket.
// The caller must reconnect and resume with the latest cursor after disruptions.
func (c *Client) Subscribe(ctx context.Context,repo,after string)(*websocket.Conn,error){
 var ticket struct{Ticket string `json:"ticket"`}
 if err:=c.request(ctx,"POST","/v1/ws-ticket",map[string]any{},&ticket);err!=nil{return nil,err}
 parsed,err:=url.Parse(c.Endpoint);if err!=nil{return nil,err}
 switch parsed.Scheme{case "https":parsed.Scheme="wss";case "http":parsed.Scheme="ws";default:return nil,errors.New("invalid CMP scheme")}
 if parsed.Scheme=="ws"&&parsed.Hostname()!="localhost"&&parsed.Hostname()!="127.0.0.1"{return nil,errors.New("CMP requires WSS outside loopback")}
 parsed.Path="/v1/cmp/connect"
 q:=url.Values{};q.Set("ticket",ticket.Ticket);parsed.RawQuery=q.Encode()
 ws,_,err:=websocket.DefaultDialer.DialContext(ctx,parsed.String(),nil)
 if err!=nil{return nil,err}
 params,_:=json.Marshal(map[string]string{"repo":repo,"after":after})
 if err:=ws.WriteJSON(Frame{V:1,ID:"subscribe-1",Type:"request",Method:"context.subscribe",Params:params});err!=nil{ws.Close();return nil,err}
 return ws,nil
}
