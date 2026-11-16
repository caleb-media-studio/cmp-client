import { createHash, randomBytes } from "node:crypto";
import { mkdir, readFile, rename, rm, stat, utimes, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

/**
 * Per-host-session advisory lock. A single Node companion hook owns the
 * encrypted session state and outbox at a time. Not a distributed lock.
 */
export async function withCompanionSessionLock<T>(
 directory:string,
 sourceSessionId:string,
 action:()=>Promise<T>,
 options:{waitMs?:number;staleMs?:number}={}
):Promise<T>{
 if(sourceSessionId.length<3||sourceSessionId.length>200)throw new Error("Invalid companion lock identity");
 const waitMs=options.waitMs??7000,staleMs=options.staleMs??120000;
 if(waitMs<0||staleMs<30000)throw new Error("Invalid companion lock timeout");
 await mkdir(directory,{mode:0o700,recursive:true});
 const key=createHash("sha256").update(sourceSessionId).digest("hex");
 const lockDir=join(directory,key+".lock");
 const nonce=randomBytes(16).toString("hex"),started=Date.now();
 while(true){
  try{
   await mkdir(lockDir,{mode:0o700});
   await writeFile(join(lockDir,"owner"),nonce,{mode:0o600});
   break;
  }catch(error){
   if((error as NodeJS.ErrnoException).code!=="EEXIST")throw error;
   try{
    const info=await stat(lockDir);
    if(Date.now()-info.mtimeMs>staleMs){
     const quarantine=lockDir+".expired-"+randomBytes(8).toString("hex");
     try{
      await rename(lockDir,quarantine);
      await rm(quarantine,{recursive:true,force:true});
     }catch(e){
      if(!["ENOENT","EEXIST"].includes((e as NodeJS.ErrnoException).code||""))throw e;
     }
     continue;
    }
   }catch(e){
    if((e as NodeJS.ErrnoException).code!=="ENOENT")throw e;
   }
   if(Date.now()-started>=waitMs)throw new Error("CMP companion session is busy; retry this hook without dropping the event");
   await delay(50);
  }
 }
 const heartbeat=setInterval(()=>{
  const current=new Date();
  void utimes(lockDir,current,current).catch(()=>{});
 },Math.max(1000,Math.floor(staleMs/4)));
 heartbeat.unref();
 try{return await action();}
 finally{
  clearInterval(heartbeat);
  try{
   const owner=await readFile(join(lockDir,"owner"),"utf8");
   if(owner===nonce)await rm(lockDir,{recursive:true,force:true});
  }catch(e){
   if((e as NodeJS.ErrnoException).code!=="ENOENT")throw e;
  }
 }
}
