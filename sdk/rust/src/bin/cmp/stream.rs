//! Durable opt-in forwarding for local CMP changes.
//! Offline operation never calls these functions unless streaming is enabled.
use super::{client, private_dir, write_secure_file, Config, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};
#[derive(Clone, Serialize, Deserialize)]
struct Event {
    method: String,
    path: String,
    body: Value,
}
#[derive(Default, Serialize, Deserialize)]
struct Outbox {
    queued: Vec<Event>,
    /// The last confirmed remote head for each repository/branch.
    acknowledgments: BTreeMap<String, String>,
    /// Remotes created by this device or confirmed empty at first sync.
    initialized: BTreeSet<String>,
}
fn root() -> Result<PathBuf> {
    let (_, config) = super::load_config()?;
    let root = config
        .parent()
        .ok_or("Invalid CMP streaming configuration")?
        .join("state");
    private_dir(&root)?;
    Ok(root)
}
fn state_file(root: &Path) -> PathBuf {
    root.join("stream-outbox.json")
}
fn load(root: &Path) -> Result<Outbox> {
    let path = state_file(root);
    if path.exists() {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    } else {
        Ok(Outbox::default())
    }
}
fn save(root: &Path, outbox: &Outbox) -> Result<()> {
    write_secure_file(&state_file(root), &serde_json::to_string(outbox)?, true)
}
fn locked<T>(callback: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    let root = root()?;
    let lock = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(".stream.lock"))?;
    lock.lock_exclusive()?;
    let result = callback(&root);
    let _ = lock.unlock();
    result
}
fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
fn event_repo(event: &Event) -> Result<(String, String)> {
    if event.path == "/v1/repos" && event.method == "POST" {
        let id = event.body["id"]
            .as_str()
            .ok_or("Streaming repository id missing")?;
        super::validate_repo(id)?;
        return Ok((id.into(), String::new()));
    }
    let url = url::Url::parse(&format!("https://local.invalid{}", event.path))?;
    let parts = url
        .path_segments()
        .ok_or("Invalid streaming path")?
        .collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != "v1" || parts[1] != "repos" {
        return Err("Invalid queued CMP operation".into());
    }
    let repo = percent_encoding::percent_decode_str(parts[2])
        .decode_utf8()?
        .into_owned();
    super::validate_repo(&repo)?;
    Ok((repo, parts[3].into()))
}
fn flush(cfg: &Config, state: &mut Outbox, root: &Path) -> Result<usize> {
    let remote = client(cfg)?;
    let mut count = 0;
    while let Some(event) = state.queued.first().cloned() {
        let (repo, action) = event_repo(&event)?;
        let prefix = format!("/v1/repos/{}", enc(&repo));
        let exists = remote
            .list_repositories()?
            .get("repositories")
            .and_then(Value::as_array)
            .is_some_and(|names| names.iter().any(|name| name.as_str() == Some(&repo)));
        if !exists {
            remote.request(
                "POST",
                "/v1/repos",
                Some(json!({"id":repo,"visibility":"private"})),
            )?;
            state.initialized.insert(repo.clone());
            save(root, state)?;
        }
        let result = match action.as_str() {
            "" => json!({"created":true}),
            "branches" => {
                let branches = remote.request("GET", &format!("{prefix}/branches"), None)?;
                let name = event.body["name"].as_str().ok_or("Missing branch name")?;
                if branches["branches"].get(name).is_some() {
                    json!({"existing":true})
                } else {
                    remote.request("POST", &event.path, Some(event.body.clone()))?
                }
            }
            "commits" => {
                let branch = event.body["branch"].as_str().unwrap_or("main");
                let key = format!("{repo}:{branch}");
                let remote_head = remote
                    .request(
                        "GET",
                        &format!("{prefix}/head?branch={}", enc(branch)),
                        None,
                    )?
                    .get("head")
                    .and_then(Value::as_str)
                    .ok_or("CMP streaming target returned no branch head")?
                    .to_owned();
                if let Some(last) = state.acknowledgments.get(&key) {
                    if *last != remote_head {
                        return Err(format!("CMP streaming conflict: remote {repo}/{branch} changed; queued local edits were not uploaded").into());
                    }
                } else {
                    // Even repositories created by this device may have acquired
                    // remote records before the first local commit is uploaded.
                    // Never skip the first-sync emptiness check.
                    let records = remote.request(
                        "GET",
                        &format!("{prefix}/records?ref={}", enc(branch)),
                        None,
                    )?;
                    if records["records"]
                        .as_array()
                        .is_none_or(|values| !values.is_empty())
                    {
                        return Err(format!("CMP streaming refused to overwrite existing remote repository {repo}/{branch}").into());
                    }
                    state.initialized.insert(repo.clone());
                }
                let mut body = event.body.clone();
                body["expectedHead"] = json!(remote_head);
                let response = remote.request("POST", &event.path, Some(body))?;
                let new_head = response["id"]
                    .as_str()
                    .ok_or("Remote CMP commit returned no id")?;
                state.acknowledgments.insert(key, new_head.into());
                response
            }
            "merge" => {
                let response = remote.request("POST", &event.path, Some(event.body.clone()))?;
                if let (Some(branch), Some(rev)) =
                    (event.body["target"].as_str(), response["id"].as_str())
                {
                    state
                        .acknowledgments
                        .insert(format!("{repo}:{branch}"), rev.into());
                }
                response
            }
            _ => return Err("Unsupported queued CMP streaming operation".into()),
        };
        let _ = result;
        state.queued.remove(0);
        save(root, state)?;
        count += 1;
    }
    Ok(count)
}
pub fn enqueue_and_try(cfg: &Config, method: &str, path: &str, payload: &Value) -> Result<()> {
    if cfg.backend != "offline" || !cfg.streaming || method != "POST" {
        return Ok(());
    }
    let event = Event {
        method: method.into(),
        path: path.into(),
        body: payload.clone(),
    };
    locked(|root| {
        let mut state = load(root)?;
        state.queued.push(event);
        save(root, &state)?;
        if let Err(error) = flush(cfg, &mut state, root) {
            eprintln!(
                "CMP: saved locally; streaming pending ({error}). Retry with cmp stream push"
            );
        }
        Ok(())
    })
}
pub fn push(cfg: &Config) -> Result<usize> {
    if cfg.backend != "offline" || !cfg.streaming {
        return Err(
            "Streaming is disabled. Use cmp setup or cmp init --streaming to opt in".into(),
        );
    }
    locked(|root| {
        let mut state = load(root)?;
        flush(cfg, &mut state, root)
    })
}
pub fn pending() -> Result<usize> {
    locked(|root| Ok(load(root)?.queued.len()))
}

/// Save the remote revision from a snapshot clone so the first opt-in stream
/// commit compares against the actual remote head rather than overwriting it.
pub fn record_clone_origin(config_dir:&Path,repo:&str,branch:&str,head:&str)->Result<()> {
    super::validate_repo(repo)?;
    if branch.is_empty() || head.len()!=64 || !head.bytes().all(|b|b.is_ascii_hexdigit()) {
        return Err("Invalid CMP remote tracking metadata".into());
    }
    let root=config_dir.join("state");
    private_dir(&root)?;
    let mut state=load(&root)?;
    state.initialized.insert(repo.into());
    state.acknowledgments.insert(format!("{repo}:{branch}"),head.into());
    save(&root,&state)
}
