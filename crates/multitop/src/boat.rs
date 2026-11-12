//! Boat inventory and transport. Credentials and SSH destinations stay owned by
//! the authenticated Boat CLI; the dashboard stores only sandbox identities.

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use tokio::process::Command;

use crate::config::Server;

pub const POLL_INTERVAL: Duration = Duration::from_secs(15);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(12);
const ACTIVE: &str = "boat:";
const OFFLINE: &str = "boat-offline:";

/// The CLI override is useful for another installation or a fixture CLI.
#[must_use]
pub fn executable() -> PathBuf {
    if let Some(path) = std::env::var_os("BOAT_CLI") {
        return path.into();
    }
    if let Some(home) = std::env::var_os("HOME") {
        let installed = PathBuf::from(home).join(".ascii/bin/boat");
        if installed.is_file() {
            return installed;
        }
    }
    "boat".into()
}

/// A Boat host has a stable sandbox id, independent of its current address.
#[must_use]
pub fn identity(host: &str) -> Option<&str> {
    host.strip_prefix(ACTIVE)
        .or_else(|| host.strip_prefix(OFFLINE))
}

#[must_use]
pub fn offline(host: &str) -> bool {
    host.starts_with(OFFLINE)
}

/// None means ordinary SSH. Inactive Boat targets always fail locally: a view
/// switch must never accidentally resume a stopped machine.
#[must_use]
pub fn transport(server: &Server) -> Option<Command> {
    if offline(&server.host) {
        let mut cmd = Command::new("false");
        cmd.kill_on_drop(true);
        return Some(cmd);
    }
    let id = server.host.strip_prefix(ACTIVE)?;
    let mut cmd = Command::new(executable());
    cmd.args(["--no-update", "ssh", id]);
    Some(cmd)
}

#[derive(Deserialize)]
struct Inventory {
    sandboxes: Vec<Sandbox>,
    #[serde(rename = "pageInfo")]
    page_info: Option<PageInfo>,
}

#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasMore")]
    has_more: bool,
}

#[derive(Deserialize)]
struct Sandbox {
    id: String,
    name: Option<String>,
    state: String,
    #[serde(rename = "type")]
    size: Option<String>,
}

/// Parse the complete CLI response, refusing partial or ambiguous inventories.
///
/// # Errors
/// Returns a displayable reason, without echoing the account response.
pub fn parse_inventory(bytes: &[u8]) -> Result<Vec<Server>, String> {
    let inventory: Inventory = serde_json::from_slice(bytes)
        .map_err(|e| format!("Boat returned an invalid inventory: {e}"))?;
    if inventory.page_info.is_some_and(|p| p.has_more) {
        return Err("Boat returned a partial inventory; keeping the previous fleet.".into());
    }
    let mut servers = Vec::new();
    for sandbox in inventory.sandboxes {
        if !sandbox.id.starts_with("bx_")
            || !sandbox
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return Err("Boat returned an invalid sandbox id.".into());
        }
        let prefix = if matches!(sandbox.state.as_str(), "idle" | "running" | "ready") {
            ACTIVE
        } else {
            OFFLINE
        };
        let name = sandbox
            .name
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| sandbox.id.clone());
        // Names are remote text, never shell commands or terminal escapes.
        let name: String = name.chars().filter(|c| !c.is_control()).collect();
        let state: String = sandbox.state.chars().filter(|c| !c.is_control()).collect();
        let size = sandbox
            .size
            .map(|size| size.chars().filter(|c| !c.is_control()).collect::<String>())
            .filter(|size| !size.trim().is_empty())
            .unwrap_or_else(|| "unknown".into());
        servers.push(Server {
            host: format!("{prefix}{}", sandbox.id),
            name: Some(format!("{name} [{state}] | SIZE {size}")),
            port: crate::config::DEFAULT_PORT,
            user: String::new(),
            upgrade_cmd: None,
            custom_command: None,
            mcp: None,
        });
    }
    servers.sort_by(|a, b| identity(&a.host).cmp(&identity(&b.host)));
    if servers
        .windows(2)
        .any(|w| identity(&w[0].host) == identity(&w[1].host))
    {
        return Err("Boat returned duplicate sandbox ids.".into());
    }
    if servers.is_empty() {
        servers.push(waiting(
            "No boats found. Discovery refreshes every 15 seconds.",
        ));
    }
    Ok(servers)
}

/// A placeholder keeps an empty fleet navigable while discovery runs.
#[must_use]
pub fn waiting(message: &str) -> Server {
    Server {
        host: format!("{OFFLINE}waiting"),
        name: Some(message.into()),
        port: crate::config::DEFAULT_PORT,
        user: String::new(),
        upgrade_cmd: None,
        custom_command: None,
        mcp: None,
    }
}

/// Read metadata only: listing never creates, resumes or changes a sandbox.
///
/// # Errors
/// Missing CLI/login, failed requests and timeouts leave the last fleet intact.
pub async fn discover() -> Result<Vec<Server>, String> {
    let mut cmd = Command::new(executable());
    cmd.args(["--no-update", "list", "--all", "--json"])
        .kill_on_drop(true);
    let output = tokio::time::timeout(DISCOVERY_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "Boat discovery timed out; retrying in 15 seconds.".to_string())?
        .map_err(|e| format!("Cannot run Boat CLI: {e}. Set BOAT_CLI or install boat."))?;
    if !output.status.success() {
        return Err("Boat discovery failed. Check boat status / boat login.".into());
    }
    parse_inventory(&output.stdout)
}

/// This machine is sampled directly by the embedded native collector, not SSH.
#[must_use]
pub fn local_server() -> Server {
    Server {
        host: "localhost".into(),
        name: Some("This Mac [local]".into()),
        port: 0,
        ..Server::default()
    }
}
