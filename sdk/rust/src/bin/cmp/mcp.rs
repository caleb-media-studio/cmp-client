//! Local stdio MCP adapter. Remote credentials remain in the chosen CLI vault;
//! agent hosts receive only an executable path, never bearer or refresh tokens.
use super::{context_request, load_config, validate_repo, Config, Result};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn as_text(result: Value) -> Value {
    json!({"content":[{"type":"text","text":result.to_string()}],"isError":false})
}
fn failure(message: &str) -> Value {
    json!({"content":[{"type":"text","text":message}],"isError":true})
}
fn required<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Missing required input: {name}").into())
}
fn repo(args: &Value, config: &Config) -> Result<String> {
    let name = args
        .get("repository")
        .and_then(Value::as_str)
        .or(config.repository.as_deref())
        .ok_or("No repository bound. Run cmp init")?;
    validate_repo(name)?;
    Ok(name.into())
}
fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
fn tools() -> Value {
    let common_repo = json!({"type":"string","description":"owner/repository; defaults to the current project binding"});
    json!([
     {"name":"cmp_repositories","description":"List CMP context repositories accessible to your account.",
      "inputSchema":{"type":"object","properties":{}}},
     {"name":"cmp_context_head","description":"Read the immutable context branch head.",
      "inputSchema":{"type":"object","properties":{"repository":common_repo,
       "branch":{"type":"string","description":"Defaults to the configured CMP project branch"}}}},
     {"name":"cmp_context_snapshot","description":"Read the records at a context branch or immutable revision.",
      "inputSchema":{"type":"object","properties":{"repository":common_repo,
       "ref":{"type":"string","description":"Defaults to the configured CMP project branch"}}}},
     {"name":"cmp_context_diff","description":"Compare two context branches or revisions.",
      "inputSchema":{"type":"object","properties":{"repository":common_repo,
       "from":{"type":"string"},"to":{"type":"string"}},"required":["from","to"]}},
     {"name":"cmp_context_commit","description":"Commit an explicitly reviewed CMP context change using CAS. Requires expectedHead, message, changes. Do not commit unsupported assertions.",
      "inputSchema":{"type":"object","properties":{"repository":common_repo,
       "branch":{"type":"string","description":"Defaults to the configured CMP project branch"},"expectedHead":{"type":"string"},
       "message":{"type":"string"},"changes":{"type":"array","items":{"type":"object"}}},
       "required":["expectedHead","message","changes"]}},
     {"name":"cmp_context_merge","description":"Three-way merge two context branches, rejecting conflicting assertions.",
      "inputSchema":{"type":"object","properties":{"repository":common_repo,
       "source":{"type":"string"},"target":{"type":"string","description":"Defaults to the configured CMP project branch"}},
       "required":["source"]}},
     {"name":"cmp_branch_create","description":"Create a context branch from an existing branch.",
      "inputSchema":{"type":"object","properties":{"repository":common_repo,
       "name":{"type":"string"},"from":{"type":"string","description":"Defaults to the configured CMP project branch"}},"required":["name"]}}
    ])
}
fn tool_call(name: &str, args: &Value, config: &Config) -> Result<Value> {
    if name == "cmp_repositories" {
        return context_request(config, "GET", "/v1/repos", None);
    }
    let id = repo(args, config)?;
    let prefix = format!("/v1/repos/{}", encode(&id));
    let branch = args
        .get("branch")
        .and_then(Value::as_str)
        .unwrap_or(&config.branch);
    match name {
        "cmp_context_head" => Ok(context_request(config,
            "GET",
            &format!("{prefix}/head?branch={}", encode(branch)),
            None,
        )?),
        "cmp_context_snapshot" => {
            let refid = args
                .get("ref")
                .and_then(Value::as_str)
                .unwrap_or(&config.branch);
            Ok(context_request(config,
                "GET",
                &format!("{prefix}/records?ref={}", encode(refid)),
                None,
            )?)
        }
        "cmp_context_diff" => {
            let from = required(args, "from")?;
            let to = required(args, "to")?;
            Ok(context_request(config,
                "GET",
                &format!("{prefix}/diff?from={}&to={}", encode(from), encode(to)),
                None,
            )?)
        }
        "cmp_context_commit" => {
            let expected = required(args, "expectedHead")?;
            let message = required(args, "message")?;
            let changes = args
                .get("changes")
                .and_then(Value::as_array)
                .ok_or("Changes must be an array")?;
            if changes.len() > 500 || message.len() > 500 {
                return Err("Commit exceeds allowed size".into());
            }
            Ok(context_request(config,
                "POST",
                &format!("{prefix}/commits"),
                Some(json!({
                 "branch":branch,"expectedHead":expected,"message":message,"changes":changes,
                 "authorId":"cmp-cli"
                })),
            )?)
        }
        "cmp_context_merge" => {
            let source = required(args, "source")?;
            let target = args
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or(&config.branch);
            Ok(context_request(config,
                "POST",
                &format!("{prefix}/merge"),
                Some(json!({
                 "source":source,"target":target,"authorId":"cmp-cli"
                })),
            )?)
        }
        "cmp_branch_create" => {
            let name = required(args, "name")?;
            let from = args
                .get("from")
                .and_then(Value::as_str)
                .unwrap_or(&config.branch);
            Ok(context_request(config,
                "POST",
                &format!("{prefix}/branches"),
                Some(json!({"name":name,"from":from})),
            )?)
        }
        _ => Err("Unknown CMP MCP tool".into()),
    }
}
pub fn serve() -> Result<()> {
    let (config, _source) = load_config()?;
    let stdin = io::stdin();
    let mut out = io::stdout().lock();
    // MCP stdio is newline-delimited JSON-RPC, not the LSP Content-Length protocol.
    // Never print progress or authentication secrets to stdout.
    for line in stdin.lock().lines() {
        let line = line?;
        if line.len() > 2_000_000 {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = match request.get("id") {
            Some(x) => x.clone(),
            None => continue,
        };
        let method = request["method"].as_str().unwrap_or("");
        let response = match method {
            "initialize" => json!({"protocolVersion":"2025-06-18",
    "capabilities":{"tools":{"listChanged":false}},
    "serverInfo":{"name":"cmp","version":env!("CARGO_PKG_VERSION")},
    "instructions":"CMP context is a versioned knowledge graph, not a Git file tree. Verify source evidence and review contradictions before committing. Never publish secrets. MCP writes are explicit."}),
            "ping" => json!({}),
            "tools/list" => json!({"tools":tools()}),
            "tools/call" => {
                let name = request["params"]["name"].as_str().unwrap_or("");
                let arguments = &request["params"]["arguments"];
                match tool_call(name, arguments, &config) {
                    Ok(data) => as_text(data),
                    Err(e) => failure(&format!("CMP tool failed: {e}")),
                }
            }
            "resources/list" => json!({"resources":[]}),
            _ => {
                let error = json!({"jsonrpc":"2.0","id":id,"error":{
      "code":-32601,"message":"Method not supported"}});
                writeln!(out, "{}", serde_json::to_string(&error)?)?;
                out.flush()?;
                continue;
            }
        };
        writeln!(
            out,
            "{}",
            serde_json::to_string(&json!({
              "jsonrpc":"2.0","id":id,"result":response
            }))?
        )?;
        out.flush()?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mcp_tools_have_unique_names() {
        let entries = tools();
        let values = entries.as_array().unwrap();
        let names: std::collections::HashSet<_> =
            values.iter().map(|v| v["name"].as_str().unwrap()).collect();
        assert_eq!(values.len(), names.len());
        assert!(names.contains("cmp_context_commit"));
        for tool in values {
            if let Some(props) = tool["inputSchema"]["properties"].as_object() {
                for (_, property) in props {
                    // Project branch defaults depend on runtime config, never a
                    // schema-static main that sends agents to the wrong branch.
                    assert_ne!(
                        property.get("default").and_then(Value::as_str),
                        Some("main")
                    );
                }
            }
        }
    }
    #[test]
    fn mcp_missing_required_fails() {
        assert!(required(&json!({}), "expectedHead").is_err());
    }
}
