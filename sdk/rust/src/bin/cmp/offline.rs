//! Embedded, offline CMP repositories. Nothing here opens a network connection.
//! Repositories and immutable snapshots remain under the selected .cmp directory.
use super::{private_dir, validate_repo, write_secure_file, Config, Result};
use fs2::FileExt;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    path::PathBuf,
};

#[derive(Default, Serialize, Deserialize)]
struct Repository {
    id: String,
    branches: BTreeMap<String, String>,
    snapshots: BTreeMap<String, BTreeMap<String, Value>>,
    parents: BTreeMap<String, Vec<String>>,
}
impl Repository {
    fn new(id: &str) -> Self {
        let genesis = hex_digest(format!("cmp-genesis:{id}").as_bytes());
        Self {
            id: id.into(),
            branches: BTreeMap::from([("main".into(), genesis.clone())]),
            snapshots: BTreeMap::from([(genesis.clone(), BTreeMap::new())]),
            parents: BTreeMap::from([(genesis, Vec::new())]),
        }
    }
    fn resolve(&self, reference: &str) -> Result<String> {
        let id = self
            .branches
            .get(reference)
            .map(String::as_str)
            .unwrap_or(reference);
        if !self.snapshots.contains_key(id) {
            return Err(format!("Unknown local CMP revision or branch: {reference}").into());
        }
        Ok(id.into())
    }
    fn snapshot(&self, reference: &str) -> Result<BTreeMap<String, Value>> {
        Ok(self
            .snapshots
            .get(&self.resolve(reference)?)
            .ok_or("Snapshot missing")?
            .clone())
    }
    fn head(&self, branch: &str) -> Result<String> {
        self.branches
            .get(branch)
            .cloned()
            .ok_or_else(|| format!("Unknown local CMP branch: {branch}").into())
    }
    fn is_ancestor(&self, ancestor: &str, child: &str) -> bool {
        let mut todo = vec![child.to_owned()];
        let mut visited = BTreeSet::new();
        while let Some(hash) = todo.pop() {
            if hash == ancestor {
                return true;
            }
            if visited.insert(hash.clone()) {
                if let Some(parents) = self.parents.get(&hash) {
                    todo.extend(parents.iter().cloned());
                }
            }
        }
        false
    }
}
fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn local_root() -> Result<PathBuf> {
    let (_, source) = super::load_config()?;
    let config_dir = source
        .parent()
        .ok_or("Invalid CMP configuration directory")?;
    // Prevent git-backed development projects from accidentally publishing local
    // context and queued streaming payloads. This does not create a .git folder.
    let ignore = config_dir.join(".gitignore");
    if !ignore.exists() {
        write_secure_file(&ignore, "state/\n", false)?;
    }
    let base = config_dir.join("state");
    private_dir(&base)?;
    private_dir(&base.join("repos"))?;
    Ok(base)
}
fn repo_path(root: &std::path::Path, repo: &str) -> Result<PathBuf> {
    validate_repo(repo)?;
    Ok(root
        .join("repos")
        .join(format!("{}.json", hex_digest(repo.as_bytes()))))
}
fn read_repo(root: &std::path::Path, id: &str) -> Result<Repository> {
    let path = repo_path(root, id)?;
    let repo: Repository = serde_json::from_slice(&fs::read(path)?)?;
    if repo.id != id {
        return Err("CMP local repository identity mismatch".into());
    }
    Ok(repo)
}
fn write_repo(root: &std::path::Path, repo: &Repository) -> Result<()> {
    write_secure_file(
        &repo_path(root, &repo.id)?,
        &serde_json::to_string(repo)?,
        true,
    )
}
fn valid_branch(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && !value.contains("..")
        && value.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_alphanumeric()
                || (i > 0 && (b == b'_' || b == b'-' || b == b'.' || b == b'/'))
        })
}
fn validate_record(record: &Value) -> Result<String> {
    let id = record["id"].as_str().ok_or("Record id required")?;
    let content = record["content"]
        .as_str()
        .ok_or("Record content must be text")?;
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
        || content.len() > 100_000
        || !["fact", "decision", "episode", "instruction", "evidence"]
            .contains(&record["kind"].as_str().unwrap_or(""))
        || !["proposed", "approved", "superseded", "disputed"]
            .contains(&record["status"].as_str().unwrap_or(""))
        || !record["sourceRefs"].is_array()
        || !record["sourceRefs"]
            .as_array()
            .unwrap()
            .iter()
            .all(Value::is_string)
        || !record["authorId"].is_string()
        || !record["updatedAt"].is_string()
    {
        return Err("Invalid CMP knowledge record".into());
    }
    Ok(id.into())
}
fn query_path(path: &str) -> Result<(Vec<String>, url::Url)> {
    let url = url::Url::parse(&format!("http://localhost{path}"))?;
    if url.origin().ascii_serialization() != "http://localhost" {
        return Err("Invalid local CMP request".into());
    }
    let parts = url
        .path_segments()
        .ok_or("Invalid CMP request path")?
        .map(|part| {
            percent_encoding::percent_decode_str(part)
                .decode_utf8()
                .map(|v| v.into_owned())
        })
        .collect::<std::result::Result<Vec<String>, _>>()?;
    Ok((parts, url))
}
fn get_query(url: &url::Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find_map(|(name, value)| (name == key).then(|| value.into_owned()))
}
fn with_lock<F>(action: F) -> Result<Value>
where
    F: FnOnce(&std::path::Path) -> Result<Value>,
{
    let root = local_root()?;
    let lock = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(".store.lock"))?;
    lock.lock_exclusive()?;
    let result = action(&root);
    let _ = lock.unlock();
    result
}
/// REST-compatible embedded store used by the CLI and its stdio MCP adapter.
/// No remote Client is ever constructed here.
pub fn request(_cfg: &Config, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
    with_lock(|root| {
        let (parts, url) = query_path(path)?;
        if parts.as_slice() == ["v1", "repos"] {
            if method == "GET" {
                let mut repositories = Vec::new();
                for file in fs::read_dir(root.join("repos"))? {
                    let file = file?;
                    if file.file_type()?.is_file()
                        && file.path().extension().is_some_and(|ext| ext == "json")
                    {
                        let repo: Repository = serde_json::from_slice(&fs::read(file.path())?)?;
                        repositories.push(repo.id);
                    }
                }
                repositories.sort();
                return Ok(json!({"repositories":repositories}));
            }
            if method == "POST" {
                let id = body
                    .as_ref()
                    .and_then(|v| v["id"].as_str())
                    .ok_or("Repository id required")?;
                let file = repo_path(root, id)?;
                if file.exists() {
                    return Err("CMP local repository already exists".into());
                }
                let repo = Repository::new(id);
                write_repo(root, &repo)?;
                return Ok(json!({"id":id,"head":repo.head("main")?}));
            }
        }
        if parts.len() < 3 || parts[0] != "v1" || parts[1] != "repos" {
            return Err("Unsupported local CMP API request".into());
        }
        let id = &parts[2];
        let mut repo = read_repo(root, id)?;
        let command = parts.get(3).map(String::as_str).unwrap_or("");
        let body = body.unwrap_or(Value::Null);
        match (method, command) {
            ("GET", "head") => Ok(
                json!({"head":repo.head(&get_query(&url,"branch").unwrap_or_else(||"main".into()))?}),
            ),
            ("GET", "records") => Ok(
                json!({"records":repo.snapshot(&get_query(&url,"ref").unwrap_or_else(||"main".into()))?.into_values().collect::<Vec<_>>()}),
            ),
            ("GET", "branches") => Ok(json!({"branches":repo.branches})),
            ("GET", "") => Ok(json!({"id":id,"head":repo.head("main")?,"branches":repo.branches})),
            ("GET", "diff") => {
                let from = get_query(&url, "from").unwrap_or_else(|| "main".into());
                let to = get_query(&url, "to").unwrap_or_else(|| "main".into());
                let a = repo.snapshot(&from)?;
                let b = repo.snapshot(&to)?;
                let mut changes = Vec::new();
                for id in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
                    if a.get(id) != b.get(id) {
                        changes.push(match b.get(id) {
                            Some(record) => json!({"op":"put","record":record}),
                            None => json!({"op":"delete","id":id}),
                        });
                    }
                }
                Ok(json!({"from":from,"to":to,"changes":changes}))
            }
            ("POST", "branches") => {
                let name = body["name"].as_str().ok_or("Branch name required")?;
                let from = body["from"].as_str().unwrap_or("main");
                if !valid_branch(name) || repo.branches.contains_key(name) {
                    return Err("Invalid or existing local CMP branch".into());
                }
                let revision = repo.resolve(from)?;
                repo.branches.insert(name.into(), revision.clone());
                write_repo(root, &repo)?;
                Ok(json!({"head":revision}))
            }
            ("POST", "commits") => {
                let branch = body["branch"].as_str().unwrap_or("main");
                let expected = body["expectedHead"]
                    .as_str()
                    .ok_or("expectedHead required")?;
                if expected != repo.head(branch)? {
                    return Err("CMP local CAS conflict: branch head changed".into());
                }
                let message = body["message"].as_str().ok_or("Commit message required")?;
                let author = body["authorId"].as_str().ok_or("Commit author required")?;
                let changes = body["changes"]
                    .as_array()
                    .ok_or("Commit changes must be an array")?;
                if changes.len() > 500 || message.len() > 2048 {
                    return Err("CMP commit exceeds size limits".into());
                }
                let mut snapshot = repo.snapshot(branch)?;
                for change in changes {
                    match change["op"].as_str() {
                        Some("put") => {
                            let record = change.get("record").ok_or("Missing record")?;
                            let id = validate_record(record)?;
                            snapshot.insert(id, record.clone());
                        }
                        Some("delete") => {
                            let id = change["id"].as_str().ok_or("Delete record id required")?;
                            snapshot.remove(id);
                        }
                        _ => return Err("Invalid CMP change; expected put or delete".into()),
                    }
                }
                let mut nonce = [0u8; 16];
                OsRng.fill_bytes(&mut nonce);
                let revision = hex_digest(&serde_json::to_vec(&json!({
                    "parent":expected,"branch":branch,"message":message,"authorId":author,
                    "changes":changes,"nonce":nonce,"id":id,
                }))?);
                repo.snapshots.insert(revision.clone(), snapshot);
                repo.parents.insert(revision.clone(), vec![expected.into()]);
                repo.branches.insert(branch.into(), revision.clone());
                write_repo(root, &repo)?;
                Ok(
                    json!({"id":revision,"parents":[expected],"branch":branch,"authorId":author,"message":message,"changes":changes}),
                )
            }
            ("POST", "merge") => {
                let source = body["source"].as_str().ok_or("Merge source required")?;
                let target = body["target"].as_str().ok_or("Merge target required")?;
                let src = repo.head(source)?;
                let dst = repo.head(target)?;
                if !repo.is_ancestor(&dst, &src) {
                    return Err("Offline merge requires fast-forward ancestry; resolve divergent branches before merge".into());
                }
                repo.branches.insert(target.into(), src.clone());
                write_repo(root, &repo)?;
                Ok(json!({"id":src,"branch":target,"fastForward":true}))
            }
            _ => Err("Unsupported local CMP API request".into()),
        }
    })
}
