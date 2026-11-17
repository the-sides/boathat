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
pub struct Sandbox {
    pub id: String,
    name: Option<String>,
    pub state: String,
    #[serde(rename = "type")]
    size: Option<String>,
    #[serde(rename = "archiveAfter")]
    archive_after: Option<String>,
}

impl Sandbox {
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("Unnamed")
    }

    #[must_use]
    pub fn size(&self) -> &str {
        self.size.as_deref().unwrap_or("unknown")
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        matches!(self.state.as_str(), "idle" | "running" | "ready")
    }

    #[must_use]
    pub fn is_dead(&self) -> bool {
        matches!(
            self.state.as_str(),
            "stopped" | "error" | "failed" | "expired" | "archived" | "dead"
        )
    }

    #[must_use]
    pub fn lease_deadline(&self) -> Option<i64> {
        let timestamp = self.archive_after.as_deref()?;
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
            .ok()
            .map(time::OffsetDateTime::unix_timestamp)
    }

    #[must_use]
    pub fn server(&self) -> Server {
        let prefix = if self.is_running() { ACTIVE } else { OFFLINE };
        Server {
            host: format!("{prefix}{}", self.id),
            name: Some(format!(
                "{} [{}] | SIZE {}",
                self.name(),
                self.state,
                self.size()
            )),
            port: crate::config::DEFAULT_PORT,
            ..Server::default()
        }
    }
}

/// Parse the complete CLI response, refusing partial or ambiguous inventories.
///
/// # Errors
/// Returns a displayable reason, without echoing the account response.
pub fn parse_inventory(bytes: &[u8]) -> Result<Vec<Sandbox>, String> {
    let mut inventory: Inventory = serde_json::from_slice(bytes)
        .map_err(|e| format!("Boat returned an invalid inventory: {e}"))?;
    if inventory.page_info.is_some_and(|p| p.has_more) {
        return Err("Boat returned a partial inventory; keeping the previous fleet.".into());
    }
    for sandbox in &mut inventory.sandboxes {
        if !sandbox.id.starts_with("bx_")
            || !sandbox
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return Err("Boat returned an invalid sandbox id.".into());
        }
        // Names are remote text, never shell commands or terminal escapes.
        sandbox.name = clean_label(sandbox.name.take());
        sandbox.size = clean_label(sandbox.size.take());
        sandbox.state.retain(|c| !c.is_control());
    }
    inventory.sandboxes.sort_by(|a, b| a.id.cmp(&b.id));
    if inventory.sandboxes.windows(2).any(|w| w[0].id == w[1].id) {
        return Err("Boat returned duplicate sandbox ids.".into());
    }
    Ok(inventory.sandboxes)
}

fn clean_label(label: Option<String>) -> Option<String> {
    label
        .map(|s| s.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|s| !s.trim().is_empty())
}

/// Read metadata only: listing never creates, resumes or changes a sandbox.
///
/// # Errors
/// Missing CLI/login, failed requests and timeouts leave the last fleet intact.
pub async fn discover() -> Result<Vec<Sandbox>, String> {
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
