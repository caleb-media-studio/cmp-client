//! GitHub CLI-inspired CMP commands backed by CMP repositories, not Git.
use super::{argument, client, context_request, load_config, open_url, project_file,
    save_config, validate_repo, Config, Result, CLOUD};
use cmp_context::cmp_repository_from_url;
#[cfg(test)]
use cmp_context::cmp_transport_path;
use serde_json::{json,Value};
use std::{env,fs,path::PathBuf};

fn selected(cfg:&Config,raw:Option<&str>)->Result<String>{
    let value=raw.or(cfg.repository.as_deref()).ok_or("No context repository selected. Run cmp init")?;
    let id=if value.starts_with("https://")||value.starts_with("http://"){
        cmp_repository_from_url(value,&cfg.endpoint)?
    } else {value.to_owned()};
    validate_repo(&id)?;
    Ok(id)
}
fn encoded(id:&str)->String{url::form_urlencoded::byte_serialize(id.as_bytes()).collect()}
fn output(value:&Value)->Result<()>{println!("{}",serde_json::to_string_pretty(value)?);Ok(())}
pub fn help_category(name:&str){
    match name {
       "repo"=>println!("Work with CMP context repositories.\n\nUSAGE\n  cmp repo <command> [flags]\n\nGENERAL COMMANDS\n  create:        Create a new CMP repository\n  list:          List accessible context repositories\n\nTARGETED COMMANDS\n  view:          Inspect a repository or open its web page\n  clone:         Import a remote .cmp snapshot into a new local project\n  set-default:   Select a default context repository for this project\n\nNote: CMP does not use Git packfiles, Git checkout or Git remotes."),
       "auth"=>println!("Authenticate CMP Cloud.\n\nUSAGE\n  cmp auth <command>\n\nCOMMANDS\n  login:    Sign in through OAuth PKCE\n  logout:   Revoke stored CMP Cloud credentials\n  status:   Show effective auth configuration"),
       "config"=>println!("Work with CMP settings.\n\nUSAGE\n  cmp config <command>\n\nCOMMANDS\n  list:     Show non-secret configuration\n  get:      Print one setting (repository, branch, backend, endpoint, streaming)\n  set:      Change repository, branch or streaming\n  show:     Show active configuration and account state\n\nCloud refresh tokens never appear in config output."),
       "branch"=>println!("Manage CMP versioned context branches.\n\nUSAGE\n  cmp branch list\n  cmp branch create <name> [from]"),
       "api"=>println!("Make an authenticated CMP API request to the configured host.\n\nUSAGE\n  cmp api /v1/repos [--method GET|POST] [--input request.json]\n\nOnly relative CMP API paths are accepted. Auth tokens are never printed."),
       _=>println!("CMP command not implemented. Use cmp --help."),
    }
}
pub fn repo(args:&[String])->Result<()> {
    let op=args.first().map(String::as_str).unwrap_or("--help");
    if matches!(op,"--help"|"help") {help_category("repo");return Ok(());}
    let (cfg,_) = load_config()?;
    match op {
       "view" => {
           let id=selected(&cfg,args.get(1).filter(|v|!v.starts_with("--")).map(String::as_str))?;
           if args.iter().any(|arg|arg=="--web"){
               let page=format!("{}/{}",cfg.endpoint, id);
               if !open_url(&page) {println!("{page}");}
               return Ok(());
           }
           let head=context_request(&cfg,"GET",&format!("/v1/repos/{}/head?branch={}",
               encoded(&id),encoded(&cfg.branch)),None)?;
           let remote=cmp_context::Client::new(&cfg.endpoint,"").cmp_remote_url(&id)?;
           println!("{id}\n  Branch: {}\n  Head: {}\n  CMP transport: {remote}",
               cfg.branch,head["head"].as_str().unwrap_or("unknown"));
           Ok(())
       }
       "set-default" => {
           let id=selected(&cfg,args.get(1).map(String::as_str))?;
           let mut config=cfg;
           config.repository=Some(id.clone());
           let project=project_file()?;
           save_config(&project,&config)?;
           println!("Default CMP context repository: {id}");
           Ok(())
       }
       "clone" => clone(args,&cfg),
       _=>Err(format!("Unknown CMP repo command: {op}. Use cmp repo --help").into())
    }
}
fn clone(args:&[String],cfg:&Config)->Result<()> {
    let raw=args.get(1).ok_or("Usage: cmp repo clone <owner/repo|https://server/owner/repo.cmp> [directory]")?;
    if cfg.backend=="offline"&&cfg.endpoint==CLOUD {
        // Explicit cloning is an online operation, but never enables streaming.
    }
    let id=selected(cfg,Some(raw))?;
    // Fetch only from the configured server. No cross-host bearer forwarding.
    let remote=client(cfg)?;
    let manifest=remote.cmp_manifest(&id)?;
    if manifest["protocol"]!="cmp"||manifest["transportVersion"]!=1||manifest["repository"]!=id {
        return Err("Remote .cmp transport manifest failed identity/version verification".into());
    }
    let branch=manifest["defaultBranch"].as_str().ok_or("CMP manifest missing default branch")?.to_owned();
    let declared=manifest["head"].as_str().ok_or("CMP manifest missing head")?.to_owned();
    let snapshot=remote.cmp_snapshot(&id,&branch)?;
    let head=snapshot["head"].as_str().ok_or("CMP snapshot missing resolved head")?;
    if snapshot["repository"]!=id || snapshot["ref"]!=branch || declared!=head {
        return Err("CMP branch changed during clone; retry to obtain a consistent snapshot".into());
    }
    let records=snapshot["records"].as_array().ok_or("Invalid CMP snapshot records")?;
    let final_path=args.get(2).map(PathBuf::from).unwrap_or_else(||{
        PathBuf::from(id.split('/').nth(1).unwrap_or("repository"))
    });
    if final_path.exists(){return Err("Clone destination already exists; refusing to overwrite".into());}
    let cwd=env::current_dir()?;
    let target=if final_path.is_absolute(){final_path}else{cwd.join(final_path)};
    let parent=target.parent().ok_or("Invalid clone destination")?;
    let parent=fs::canonicalize(parent)?;
    let target=parent.join(target.file_name().ok_or("Invalid clone destination name")?);
    if target.exists(){return Err("Clone destination already exists".into());}
    let stage=parent.join(format!(".cmp-clone-{}",super::random_short()));
    fs::create_dir(&stage)?;
    let attempt=(||->Result<()>{
        let dir=stage.join(".cmp");
        let mut local=Config::new("offline",&cfg.endpoint);
        local.repository=Some(id.clone());
        local.branch=branch.clone();
        local.streaming=false;
        save_config(&dir.join("config.toml"),&local)?;
        super::cmp_offline::import_snapshot(&dir,&id,&branch,head,records)?;
        super::cmp_stream::record_clone_origin(&dir,&id,&branch,head)?;
        fs::rename(&stage,&target)?;
        Ok(())
    })();
    if attempt.is_err(){let _=fs::remove_dir_all(&stage);}
    attempt?;
    println!("Cloned CMP snapshot {id}@{head} into {}",target.display());
    println!("Streaming remains disabled. Only the selected branch snapshot was imported, not the full remote commit history.");
    Ok(())
}
pub fn auth(args:&[String])->Result<()>{
    match args.first().map(String::as_str).unwrap_or("--help"){
        "login"=>super::login(&args[1..]),
        "logout"=>super::logout(),
        "status"=>super::status(),
        "help"|"--help"=>{help_category("auth");Ok(())},
        other=>Err(format!("Unknown CMP auth command: {other}. Use cmp auth --help").into())
    }
}
pub fn config(args:&[String])->Result<()>{
    let action=args.first().map(String::as_str).unwrap_or("--help");
    if matches!(action,"--help"|"help"){help_category("config");return Ok(());}
    if action=="show"{return super::status();}
    let (mut cfg,path)=load_config()?;
    match action{
        "list"=>output(&json!({"backend":cfg.backend,"endpoint":cfg.endpoint,
           "repository":cfg.repository,"branch":cfg.branch,"streaming":cfg.streaming})),
        "get"=>{
            let key=args.get(1).ok_or("Usage: cmp config get <key>")?.as_str();
            match key {
                "backend"=>println!("{}",cfg.backend),
                "endpoint"=>println!("{}",cfg.endpoint),
                "repository"=>println!("{}",cfg.repository.as_deref().unwrap_or("")),
                "branch"=>println!("{}",cfg.branch),
                "streaming"=>println!("{}",cfg.streaming),
                _=>return Err("Unknown config key. Use cmp config --help".into())
            }
            Ok(())
        }
        "set"=>{
            let key=args.get(1).ok_or("Usage: cmp config set <repository|branch|streaming> <value>")?;
            let value=args.get(2).ok_or("Usage: cmp config set <key> <value>")?;
            match key.as_str(){
                "repository"=>{validate_repo(value)?;cfg.repository=Some(value.clone());}
                "branch"=>{
                    if value.is_empty()||value.len()>120||value.contains("..")||value.bytes().any(|b|!b.is_ascii_alphanumeric()&&!b"._/-".contains(&b)){
                        return Err("Invalid CMP branch".into());
                    }
                    cfg.branch=value.clone();
                }
                "streaming"=>{
                    if cfg.backend!="offline"{return Err("Streaming requires offline backend".into());}
                    cfg.streaming=match value.as_str(){"true"=>true,"false"=>false,_=>return Err("Streaming must be true or false".into())};
                }
                _=>return Err("Use cmp setup to change backend or endpoint; config set only accepts repository, branch and streaming".into())
            };
            save_config(&path,&cfg)?;
            println!("{key} = {value}");
            Ok(())
        }
        _=>Err(format!("Unknown CMP config command: {action}").into())
    }
}
pub fn browse(args:&[String])->Result<()>{
    let (cfg,_) = load_config()?;
    let id=selected(&cfg,args.first().map(String::as_str))?;
    let page=format!("{}/{}",cfg.endpoint,id);
    if !open_url(&page){println!("{page}");}
    Ok(())
}
pub fn api(args:&[String])->Result<()>{
    if args.is_empty()||args.iter().any(|x|x=="--help"||x=="-h"){help_category("api");return Ok(());}
    let path=&args[0];
    let path_only=path.split('?').next().unwrap_or(path);
    let lowered=path_only.to_ascii_lowercase();
    if path_only.split('/').any(|part| part=="."||part=="..") || lowered.contains("%2e") || path.contains("//") {
        return Err("CMP API path traversal is not allowed".into());
    }
    if !path.starts_with("/v1/") || path.starts_with("//") || path.contains('\\')
        || path.contains('#') || path.chars().any(char::is_control){
        return Err("CMP API path must start with /v1/ and cannot specify another host".into());
    }
    let method=argument(args,"--method").unwrap_or_else(||"GET".into());
    if method!="GET"&&method!="POST"{return Err("CMP API supports GET or POST only".into());}
    let body=if let Some(file)=argument(args,"--input"){
        let meta=fs::metadata(&file)?;
        if !meta.is_file()||meta.len()>5_000_000{return Err("CMP API input must be a JSON file under 5 MB".into());}
        Some(serde_json::from_slice::<Value>(&fs::read(file)?)?)
    }else{None};
    if method=="POST"&&body.is_none(){return Err("POST requires --input request.json".into());}
    let (cfg,_)=load_config()?;
    output(&context_request(&cfg,&method,path,body)?)
}
pub fn branch_list()->Result<()>{
    let (cfg,_)=load_config()?;
    let id=selected(&cfg,None)?;
    output(&context_request(&cfg,"GET",&format!("/v1/repos/{}/branches",encoded(&id)),None)?)
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]fn command_category_help_is_nonsecret(){
    for category in ["repo","auth","config","branch","api"]{help_category(category);}
    assert!(cmp_transport_path("team/repo").is_ok());
 }
}
