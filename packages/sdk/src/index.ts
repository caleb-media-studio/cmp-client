import {cmpTransportPath} from "@cmp-chat/protocol";
import type { CmpFrame, Change, ContextCommit, KnowledgeRecord,
 CmpTransportManifest,CmpTransportRefs,CmpTransportSnapshot } from "@cmp-chat/protocol";
import type {
 AgentJoinRequest, AgentSession, AgentCredentials, AgentPeer,
 WorkContractProposal, WorkContract, ClaimAcquireRequest, WorkClaim
} from "./cloud/coordination-types.js";
import type {
 ArchivePolicy, SessionArchiveEvent, ArchiveRevision, ArchiveStatus
} from "./cloud/archive-types.js";

type CloudFrame = CmpFrame | (Omit<CmpFrame,"type"> & {type:"event";cursor?:string;eventId?:string});

export type CMPConnectionOptions = {
  httpEndpoint: string;
  accessToken?: string;
  /** Called before each HTTP request to support renewable Firebase/OIDC tokens. */
  accessTokenProvider?: () => Promise<string>;
  webSocketEndpoint?: string;
  reconnectDelayMs?: number;
  requestTimeoutMs?: number;
};
export type CMPEvents = (event: CloudFrame) => void;

export class CMPHTTPError extends Error {
 constructor(readonly status:number,message:string){
  super(message);
  this.name="CMPHTTPError";
 }
}
export function isRetryableCMPError(error:unknown):boolean{
 if(error instanceof CMPHTTPError)
  return error.status===408||error.status===425||error.status===429||error.status>=500;
 return error instanceof TypeError ||
  error instanceof Error&&["AbortError","TimeoutError"].includes(error.name);
}

export class UnsupportedCMPCapability extends Error {
 constructor(readonly capability:string){
  super("CMP endpoint does not support "+capability);
  this.name="UnsupportedCMPCapability";
 }
}

/** Browser and Node 24+ Core HTTP client with optional Cloud API wrappers. */
export class CMPClient {
  private ws: WebSocket | undefined;
  private cursors = new Map<string,string>();
  private agentSubscriptions = new Map<string,AgentSession>();
  private counter = 0;
  private closed = false;
  private listeners = new Set<CMPEvents>();
  private subs = new Set<string>();
  private pending = new Map<string,{resolve:(value:unknown)=>void;reject:(reason:Error)=>void}>();
  constructor(private readonly options: CMPConnectionOptions) {}
  private async requireCapability(feature:string):Promise<void>{
    const available=await this.getCapabilities();
    if(available.capabilities[feature]!==true)throw new UnsupportedCMPCapability(feature);
  }

  private async api<T>(path:string,method="GET",body?:object):Promise<T> {
    const token=(this.options.accessTokenProvider?await this.options.accessTokenProvider():
      this.options.accessToken||"").trim();
    if(!token||token.length>12000)throw new Error("CMP access token is required");
    const response=await fetch(new URL(path,this.options.httpEndpoint),{
      method,headers:{authorization:"Bearer "+token,"content-type":"application/json"},
      redirect:"error",
      signal:AbortSignal.timeout(this.options.requestTimeoutMs??10000),
      body:body?JSON.stringify(body):undefined
    });
    // Proxies can return non-JSON 502/503 pages. Preserve the HTTP status
    // instead of turning a retryable failure into an unrelated SyntaxError.
    let data:T & {error?:string};
    try{data=await response.json() as T & {error?:string};}
    catch{
      if(!response.ok)throw new CMPHTTPError(response.status,"CMP HTTP "+response.status);
      throw new Error("CMP returned invalid JSON");
    }
    if(!response.ok)throw new CMPHTTPError(response.status,data.error||"CMP HTTP "+response.status);
    return data;
  }
  /** Generic REST RPC used by all official CMP language clients. */
  async call<T=unknown>(method:string,params:Record<string,unknown>={}):Promise<T>{
    if(/^(agent\.|coordination\.)/.test(method))await this.requireCapability("agentCoordination");
    if(/^session\./.test(method))await this.requireCapability("sessionArchive");
    if(/^sources\./.test(method))await this.requireCapability("connect");
    const response=await this.api<{result:T}>("/v1/rpc","POST",{method,params});
    return response.result;
  }
  joinAgent(request:AgentJoinRequest){return this.call<AgentSession>("agent.join",request);}
  resumeAgent(request:AgentJoinRequest,session:AgentCredentials){
    return this.joinAgent({...request,resumeSessionId:session.sessionId,resumeSessionToken:session.sessionToken});
  }
  listAgents(auth:AgentCredentials){return this.call<{peers:AgentPeer[]}>("agent.list",auth);}
  agentEvents(auth:AgentCredentials,after="0"){return this.call<{
    events:CmpFrame[];nextCursor:string;hasMore:boolean
  }>("agent.events",{...auth,after});}
  heartbeat(auth:AgentCredentials,ttlMs=60000){return this.call("agent.heartbeat",{...auth,ttlMs});}
  publishIntent(auth:AgentCredentials,intent:string,paths:string[]=[]){return this.call("agent.intent.publish",{...auth,intent,paths});}
  leaveAgent(auth:AgentCredentials){return this.call("agent.leave",auth);}
  sendCoordinationMessage(auth:AgentCredentials,message:string,toSessionId?:string){
    return this.call("coordination.message",{...auth,message,toSessionId});
  }
  proposeContract(proposal:WorkContractProposal){return this.call<WorkContract>("coordination.propose",proposal);}
  reviseContract(auth:AgentCredentials,contractId:string,objective:string,ownershipScopes:Record<string,string[]>){
    return this.call<WorkContract>("coordination.revise",{...auth,contractId,objective,ownershipScopes});
  }
  acceptContract(auth:AgentCredentials,contractId:string,revision:number){
    return this.call("coordination.accept",{...auth,contractId,revision});
  }
  rejectContract(auth:AgentCredentials,contractId:string){
    return this.call("coordination.reject",{...auth,contractId});
  }
  getContract(auth:AgentCredentials,contractId:string){
    return this.call("coordination.contract.get",{...auth,contractId});
  }
  acquireClaim(request:ClaimAcquireRequest){return this.call<WorkClaim>("coordination.claim.acquire",request);}
  renewClaim(auth:AgentCredentials,claimId:string,fencingToken:number,durationMs=30000){
    return this.call<WorkClaim>("coordination.claim.renew",{...auth,claimId,fencingToken,durationMs});
  }
  releaseClaim(auth:AgentCredentials,claimId:string,fencingToken:number){
    return this.call("coordination.claim.release",{...auth,claimId,fencingToken});
  }
  proposeChange(auth:AgentCredentials,claimId:string,fencingToken:number,path:string,gitRevision:string){
    return this.call("coordination.change.propose",{...auth,claimId,fencingToken,path,gitRevision});
  }
  archivePolicy(auth:AgentCredentials){return this.call<ArchivePolicy>("session.policy.get",auth);}
  /** Administrative development-server operation. Production requires scoped RBAC. */
  configureArchivePolicy(organizationId:string,repositoryId:string,policy:{
    enabled:boolean;rawEnabled?:boolean;retentionDays?:number;legalHold?:boolean
  }){
    return this.call<ArchivePolicy>("session.policy.set",{organizationId,repositoryId,...policy});
  }
  openArchive(auth:AgentCredentials,source:string,enabled:boolean,exclude=false,coverage:"partial"|"complete"="partial"){
    return this.call<{archiving:boolean;reason?:string;sessionId?:string;head?:string;policyVersion?:number}>(
      "session.open",{...auth,source,archiveEnabled:enabled,exclude,coverage});
  }
  appendArchive(auth:AgentCredentials,event:SessionArchiveEvent){
    return this.call<ArchiveRevision>("session.append",{...auth,...event});
  }
  checkpointArchive(auth:AgentCredentials,event:Omit<SessionArchiveEvent,"kind">){
    return this.call<ArchiveRevision>("session.checkpoint",{...auth,...event});
  }
  archiveStatus(auth:AgentCredentials){return this.call<ArchiveStatus>("session.get",auth);}
  archiveEvents(auth:AgentCredentials,includeRaw=false){return this.call("session.events",{...auth,includeRaw});}
  closeArchive(auth:AgentCredentials){return this.call("session.close",auth);}
  excludeArchive(auth:AgentCredentials){return this.call("session.exclude",auth);}
  deleteArchive(auth:AgentCredentials){return this.call("session.delete",auth);}
  // Versioned CMP JSON repository transport, not Git smart HTTP.
  cmpManifest(repo:string){
    return this.api<CmpTransportManifest>(cmpTransportPath(repo));
  }
  cmpRefs(repo:string){
    return this.api<CmpTransportRefs>(cmpTransportPath(repo)+"?view=refs");
  }
  cmpSnapshot(repo:string,ref="main"){
    if(!ref||ref.length>128)throw new Error("Invalid CMP snapshot reference");
    return this.api<CmpTransportSnapshot>(cmpTransportPath(repo)+
      "?view=snapshot&ref="+encodeURIComponent(ref));
  }
  cmpPush(repo:string,args:{branch:string;expectedHead:string;authorId:string;message:string;changes:Change[]}){
    return this.api<ContextCommit>(cmpTransportPath(repo),"POST",args);
  }
  createRepository(id:string){return this.api<{id:string;head:string}>("/v1/repos","POST",{id});}
  listRepositories(){return this.api<{repositories:string[]}>("/v1/repos");}
  getSnapshot(repo:string,ref="main"){
    return this.api<{records:KnowledgeRecord[]}>("/v1/repos/"+encodeURIComponent(repo)+"/records?ref="+encodeURIComponent(ref));
  }
  getHead(repo:string,branch="main"){
    return this.api<{head:string}>("/v1/repos/"+encodeURIComponent(repo)+"/head?branch="+encodeURIComponent(branch));
  }
  createBranch(repo:string,name:string,from="main"){
    return this.api<{head:string}>("/v1/repos/"+encodeURIComponent(repo)+"/branches","POST",{name,from});
  }
  commit(repo:string,args:{branch:string;expectedHead:string;authorId:string;message:string;changes:Change[]}){
    return this.api<ContextCommit>("/v1/repos/"+encodeURIComponent(repo)+"/commits","POST",args);
  }
  merge(repo:string,source:string,target:string,authorId:string){
    return this.api<ContextCommit>("/v1/repos/"+encodeURIComponent(repo)+"/merge","POST",{source,target,authorId});
  }
  /** Compare two context branches or immutable revisions. */
  diff(repo:string,from:string,to:string){
    return this.api<{from:string;to:string;changes:Change[]}>(
      "/v1/repos/"+encodeURIComponent(repo)+"/diff?from="+encodeURIComponent(from)+
      "&to="+encodeURIComponent(to));
  }
  /** Managed CMP Cloud capability discovery; older self-host servers may return 404. */
  getCapabilities(){return this.api<{
    protocolVersion:number;implementation:string;
    capabilities:Record<string,boolean>;
  }>("/v1/capabilities");}

  async connect():Promise<void>{
    await this.requireCapability("websocket");
    this.closed=false;
    if(this.ws?.readyState===WebSocket.OPEN)return;
    const {ticket}=await this.api<{ticket:string}>("/v1/ws-ticket","POST",{});
    const url=new URL(this.options.webSocketEndpoint||this.options.httpEndpoint.replace(/^http/,"ws")+"/v1/cmp/connect");
    if(!["wss:","ws:"].includes(url.protocol))throw new Error("Invalid socket protocol");
    if(url.protocol==="ws:"&&!["localhost","127.0.0.1","::1"].includes(url.hostname))throw new Error("CMP requires encrypted WSS outside localhost");
    url.searchParams.set("ticket",ticket);
    await new Promise<void>((resolve,reject)=>{
      const ws=new WebSocket(url);
      this.ws=ws;
      ws.onopen=()=>resolve();
      ws.onerror=()=>reject(new Error("CMP WebSocket connection failed"));
      ws.onmessage=e=>this.receive(String(e.data));
      ws.onclose=()=>{
        for(const [,value] of this.pending)value.reject(new Error("CMP socket closed"));
        this.pending.clear();
        if(!this.closed)setTimeout(()=>{
          if(!this.closed)void this.connect().then(async()=>{
            for(const repo of this.subs)await this.send("context.subscribe",{
              repo,after:this.cursors.get("context:"+repo)||"0"
            });
            for(const [key,session] of this.agentSubscriptions){
              await this.send("agent.subscribe",{sessionId:session.sessionId,sessionToken:session.sessionToken,
                after:this.cursors.get(key)||"0"});
            }
          }).catch(()=>{});
        },this.options.reconnectDelayMs??1500);
      };
    });
  }

  private receive(data:string) {
    let frame:CloudFrame;
    try{frame=JSON.parse(data) as CloudFrame;}catch{return;}
    if(frame.v!==1)return;
    if(frame.type==="event"){
      if(frame.cursor){
        const params=frame.params||{};
        const key=typeof params.organizationId==="string"&&typeof params.repositoryId==="string"
          ?"agent:"+params.organizationId+":"+params.repositoryId
          :typeof params.repo==="string"?"context:"+params.repo:null;
        if(key){
          if(!/^\d+$/.test(frame.cursor))return;
          const previous=BigInt(this.cursors.get(key)||"0");
          const current=BigInt(frame.cursor);
          // Native CMP uses at-least-once delivery. Older or duplicate cursor
          // events must not run subscribers more than once per stream.
          if(current<=previous)return;
          this.cursors.set(key,frame.cursor);
        }
      }
      for(const listener of this.listeners)listener(frame);
      return;
    }
    const awaiting=this.pending.get(frame.id);
    if(!awaiting)return;
    this.pending.delete(frame.id);
    if(frame.type==="error")awaiting.reject(new Error(frame.error?.message||"CMP request failed"));
    else awaiting.resolve(frame.result);
  }
  private send(method:string,params:Record<string,unknown>):Promise<unknown>{
    if(this.ws?.readyState!==WebSocket.OPEN)throw new Error("CMP connection is not open");
    const id="r"+(++this.counter);
    return new Promise((resolve,reject)=>{
      this.pending.set(id,{resolve,reject});
      this.ws!.send(JSON.stringify({v:1,id,type:"request",method,params} satisfies CmpFrame));
    });
  }
  onEvent(handler:CMPEvents):()=>void {this.listeners.add(handler);return()=>this.listeners.delete(handler);}
  async subscribe(repo:string):Promise<void>{
    await this.connect();
    await this.send("context.subscribe",{repo,after:this.cursors.get("context:"+repo)||"0"});
    this.subs.add(repo);
  }
  async subscribeAgents(session:AgentSession):Promise<void>{
    await this.connect();
    const key="agent:"+session.organizationId+":"+session.repositoryId;
    await this.send("agent.subscribe",{sessionId:session.sessionId,sessionToken:session.sessionToken,
      after:this.cursors.get(key)||"0"});
    this.agentSubscriptions.set(key,session);
  }
  disconnect(){
    this.closed=true;
    this.ws?.close();
    for(const [,p] of this.pending)p.reject(new Error("CMP disconnected"));
    this.pending.clear();
  }
}
