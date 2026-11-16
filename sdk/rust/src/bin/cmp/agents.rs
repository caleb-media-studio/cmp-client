//! Host-specific, reversible CMP MCP installation. Never write credentials into
//! Claude Code, Codex, or OpenCode files.
use super::Result;
use serde_json::{json, Value};
use std::{env, fs, path::PathBuf, process::Command};
fn available(command: &str) -> bool {
    Command::new(command).arg("--version").output().is_ok()
}
fn project() -> Result<PathBuf> {
    Ok(env::current_dir()?)
}
pub fn list() {
    for (host, exe) in [
        ("claude", "claude"),
        ("codex", "codex"),
        ("opencode", "opencode"),
    ] {
        println!(
            "{}: {}",
            host,
            if available(exe) {
                "available"
            } else {
                "not installed"
            }
        );
    }
}
pub fn install(host: &str) -> Result<()> {
    let current = project()?;
    match host {
        "claude" => {
            if !available("claude") {
                return Err("Claude Code CLI not installed; cannot install project MCP".into());
            }
            // Claude Code writes .mcp.json for project-scoped MCP servers and retains
            // host's schema/format instead of handwritten, potentially invalid JSON.
            let output = Command::new("claude")
                .args([
                    "mcp", "add", "--scope", "project", "cmp", "--", "cmp", "mcp", "serve",
                ])
                .current_dir(&current)
                .output()?;
            if !output.status.success() {
                return Err(format!(
                    "Claude Code refused MCP installation: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )
                .into());
            }
            println!(
                "Installed CMP MCP for Claude Code project. Restart agent and run cmp doctor."
            );
        }
        "codex" => {
            let file = current.join(".codex/config.toml");
            fs::create_dir_all(file.parent().unwrap())?;
            let existing = if file.exists() {
                fs::read_to_string(&file)?
            } else {
                String::new()
            };
            if file.is_symlink() {
                return Err("Refusing to modify symlinked Codex config".into());
            }
            let parsed: toml::Value = if existing.trim().is_empty() {
                toml::from_str("title = 'Codex'")?
            } else {
                toml::from_str(&existing)?
            };
            if parsed
                .get("mcp_servers")
                .and_then(|x| x.get("cmp"))
                .is_some()
            {
                return Err(
                    "CMP MCP is already installed for Codex. Remove it before reinstalling".into(),
                );
            }
            let cwd = toml::Value::String(current.display().to_string()).to_string();
            let next=format!("{}\n# CMP CLI managed MCP entry\n[mcp_servers.cmp]\ncommand = 'cmp'\nargs = ['mcp','serve']\ncwd = {}\n",
    existing.trim_end(),cwd);
            super::write_secure_file(&file, &next, true)?;
            println!(
                "Installed CMP MCP in {}. Restart Codex and check /mcp.",
                file.display()
            );
        }
        "opencode" => {
            let file = current.join("opencode.json");
            if file.is_symlink() {
                return Err("Refusing to modify symlinked OpenCode config".into());
            }
            if current.join("opencode.jsonc").exists() {
                return Err(
                    "OpenCode JSONC config detected. Please merge the CMP MCP entry manually"
                        .into(),
                );
            }
            let mut value: Value = if file.exists() {
                serde_json::from_slice(&fs::read(&file)?)?
            } else {
                json!({"$schema":"https://opencode.ai/config.json"})
            };
            if !value.is_object() {
                return Err("OpenCode configuration must be a JSON object".into());
            }
            if value.get("mcp").is_none() {
                value["mcp"] = json!({})
            }
            if !value["mcp"].is_object() {
                return Err("OpenCode MCP configuration must be an object".into());
            }
            if value["mcp"].get("cmp").is_some() {
                return Err("CMP MCP already configured for OpenCode".into());
            }
            value["mcp"]["cmp"] =
                json!({"type":"local","command":["cmp","mcp","serve"],"enabled":true});
            // The existing config has been parsed and merged; update it
            // atomically rather than treating every pre-existing file as fatal.
            super::write_secure_file(&file, &serde_json::to_string_pretty(&value)?, file.exists())?;
            println!("Installed CMP MCP in {}. Restart OpenCode.", file.display());
        }
        _ => return Err("Supported agents: claude, codex, opencode".into()),
    }
    Ok(())
}
pub fn remove(host: &str) -> Result<()> {
    let current = project()?;
    match host {
        "claude" => {
            if !available("claude") {
                return Err("Claude Code CLI is needed to remove its CMP MCP entry".into());
            }
            let result = Command::new("claude")
                .args(["mcp", "remove", "cmp", "--scope", "project"])
                .current_dir(&current)
                .status()?;
            if !result.success() {
                return Err("Claude Code could not remove CMP MCP".into());
            }
        }
        "codex" => {
            let file = current.join(".codex/config.toml");
            let text = fs::read_to_string(&file)?;
            let marker = "# CMP CLI managed MCP entry\n[mcp_servers.cmp]";
            let at = text
                .find(marker)
                .ok_or("CMP CLI managed MCP entry not found; preserve manual configs")?;
            let parsed: toml::Value = toml::from_str(&text)?;
            let entry = parsed
                .get("mcp_servers")
                .and_then(|v| v.get("cmp"))
                .ok_or("CMP MCP section missing")?;
            if entry.get("command").and_then(|v| v.as_str()) != Some("cmp")
                || entry
                    .get("args")
                    .and_then(|v| v.as_array())
                    .is_none_or(|args| {
                        args.len() != 2
                            || args[0].as_str() != Some("mcp")
                            || args[1].as_str() != Some("serve")
                    })
            {
                return Err("CMP MCP entry was edited manually; refusing removal".into());
            }
            // A nested table such as [mcp_servers.cmp.env] is user-owned
            // customization. Our marker-based slice must not leave an
            // incomplete CMP entry behind or delete the user's environment.
            if entry.as_table().is_none_or(|table| {
                table
                    .keys()
                    .any(|key| !matches!(key.as_str(), "command" | "args" | "cwd"))
            }) {
                return Err(
                    "CMP MCP contains custom nested tables or fields; refusing destructive removal"
                        .into(),
                );
            }
            let remaining = &text[at + marker.len()..];
            let end = remaining
                .find("\n[")
                .map(|offset| at + marker.len() + offset)
                .unwrap_or(text.len());
            let next = format!("{}{}", text[..at].trim_end(), &text[end..]);
            super::write_secure_file(&file, &next, true)?;
        }
        "opencode" => {
            let file = current.join("opencode.json");
            let mut value: Value = serde_json::from_slice(&fs::read(&file)?)?;
            if value["mcp"].get("cmp").is_none() {
                return Err("CMP MCP not found".into());
            }
            let expected = json!({"type":"local","command":["cmp","mcp","serve"],"enabled":true});
            if value["mcp"]["cmp"] != expected {
                return Err("CMP entry modified; refusing destructive removal".into());
            }
            value["mcp"].as_object_mut().unwrap().remove("cmp");
            super::write_secure_file(&file, &serde_json::to_string_pretty(&value)?, true)?;
        }
        _ => return Err("Supported agents: claude, codex, opencode".into()),
    }
    println!("Removed CMP MCP for {host}");
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn agent_names_are_known() {
        assert!(matches!("codex", "claude" | "codex" | "opencode"));
    }
}
