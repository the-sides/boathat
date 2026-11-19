//! Session-local inventory, visibility and live samples for the Boat dashboard.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use crate::boat::{identity, offline, Sandbox, Usage, POLL_INTERVAL};
use crate::config::Server;
use crate::panel::Panel;
use multitop_agent::{proto::Payload, render::Snapshot};

#[derive(Default)]
pub struct State {
    pub(crate) ssh_servers: Vec<Server>,
    pub inventory: Vec<Sandbox>,
    pub visible: BTreeSet<String>,
    pub samples: BTreeMap<String, Instant>,
    pub(crate) usage: BTreeMap<String, Usage>,
    pub auto_clear: bool,
    pub(crate) display: Display,
    pub list_open: bool,
    pub list_scroll: usize,
    pub discovery_notice: Option<String>,
    pub discovered: bool,
    dismissed: BTreeSet<String>,
}

#[derive(Clone, Copy)]
pub(crate) enum StatsPanel {
    Cpu,
    Network,
    Processes,
    Disks,
}

#[derive(Default)]
pub(crate) struct Display {
    pub(crate) hide_memory: bool,
    hidden_panels: u8,
    pub(crate) stats_text: Option<multitop_agent::surface::Rgb>,
}

impl Display {
    pub(crate) const fn hidden(&self, panel: StatsPanel) -> bool {
        self.hidden_panels & (1 << panel as u8) != 0
    }

    pub(crate) fn toggle(&mut self, panel: StatsPanel) {
        self.hidden_panels ^= 1 << panel as u8;
    }
}

/// Stable across inventory refreshes and Boat state changes.
pub(crate) fn panel_key(server: &Server) -> Cow<'_, str> {
    if let Some(id) = identity(&server.host) {
        Cow::Borrowed(id)
    } else if crate::ssh::is_local(server) {
        Cow::Borrowed("localhost")
    } else {
        Cow::Owned(format!("ssh:{}:{}", server.target(), server.port))
    }
}

impl State {
    pub(crate) fn new(ssh_servers: Vec<Server>) -> Self {
        Self {
            ssh_servers,
            ..Self::default()
        }
    }

    pub fn select(&mut self, inventory: Vec<Sandbox>) -> Vec<Server> {
        self.inventory = inventory;
        self.discovered = true;
        self.usage.retain(|id, _| {
            self.inventory
                .iter()
                .any(|boat| &boat.id == id && boat.is_running())
        });
        self.visible
            .retain(|id| self.inventory.iter().any(|boat| &boat.id == id));
        self.dismissed.retain(|id| {
            self.inventory
                .iter()
                .any(|boat| &boat.id == id && !boat.is_running())
        });
        self.samples.retain(|id, _| {
            id == "localhost"
                || self
                    .ssh_servers
                    .iter()
                    .any(|server| panel_key(server) == id.as_str())
                || self
                    .inventory
                    .iter()
                    .any(|boat| &boat.id == id && boat.is_running())
        });
        for boat in &self.inventory {
            if boat.is_running() {
                self.visible.insert(boat.id.clone());
            }
        }
        if self.auto_clear {
            self.clear_dead();
        }
        self.servers()
    }

    pub fn dismiss(&mut self, id: &str) {
        self.visible.remove(id);
        self.dismissed.insert(id.to_owned());
    }

    pub fn clear_dead(&mut self) {
        for boat in self.inventory.iter().filter(|boat| boat.is_dead()) {
            self.visible.remove(&boat.id);
            self.dismissed.insert(boat.id.clone());
        }
    }

    pub fn list_rows(&self) -> impl Iterator<Item = &Sandbox> {
        let shown = || {
            self.inventory
                .iter()
                .filter(|boat| !self.dismissed.contains(&boat.id))
        };
        shown()
            .filter(|boat| !boat.is_dead())
            .chain(shown().filter(|boat| boat.is_dead()))
    }

    pub fn servers(&self) -> Vec<Server> {
        std::iter::once(crate::boat::local_server())
            .chain(self.ssh_servers.iter().cloned())
            .chain(
                self.list_rows()
                    .filter(|boat| self.visible.contains(&boat.id))
                    .map(Sandbox::server),
            )
            .collect()
    }

    pub(crate) fn fresh_sample<'a>(&self, panel: &'a Panel) -> Option<&'a Snapshot> {
        if offline(&panel.server.host)
            || self
                .samples
                .get(panel_key(&panel.server).as_ref())
                .is_none_or(|at| at.elapsed() > POLL_INTERVAL)
        {
            return None;
        }
        match panel.last_monitor.as_ref()? {
            Payload::Monitor(snapshot) => Some(snapshot),
            _ => None,
        }
    }
}

/// Compute from fresh samples only, weighted by each reporting VM's core count.
/// Missing hosts are named as missing coverage rather than counted as idle.
#[must_use]
pub fn summary(app: &crate::app::App) -> String {
    let Some(state) = &app.boat else {
        return String::new();
    };
    let running = state
        .inventory
        .iter()
        .filter(|boat| boat.is_running())
        .count();
    let hidden = state
        .inventory
        .iter()
        .filter(|boat| boat.is_dead() && !state.visible.contains(&boat.id))
        .count();
    let mut weighted = 0.0;
    let mut cores = 0_u32;
    let mut reporting = 0;
    for panel in &app.panels {
        if identity(&panel.server.host).is_none() {
            continue;
        }
        if let Some(snap) = state.fresh_sample(panel) {
            if !snap.cpu_pct.is_finite() {
                continue;
            }
            let count = u32::try_from(snap.cores.len()).unwrap_or(u32::MAX).max(1);
            weighted = snap.cpu_pct.mul_add(f64::from(count), weighted);
            cores = cores.saturating_add(count);
            reporting += 1;
        }
    }
    let cpu = if cores == 0 {
        "--".into()
    } else {
        format!("{:.1}%", weighted / f64::from(cores))
    };
    let auto = if state.auto_clear { "on" } else { "off" };
    let view = if state.list_open { "l grid" } else { "l list" };
    let hidden = if state.list_open || hidden == 0 {
        String::new()
    } else {
        format!(" | {hidden} stopped hidden")
    };
    let memory = if state.display.hide_memory {
        "m show mem"
    } else {
        "m hide mem"
    };
    format!("{view}  {memory}  t theme  c clear  C auto {auto}  G graphs  s stats | {running} up | Fleet CPU {cpu} ({reporting}/{running} reporting){hidden} | / filter  q quit")
}
