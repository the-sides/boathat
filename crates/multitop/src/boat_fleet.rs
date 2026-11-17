//! Session-local inventory, visibility and live samples for the Boat dashboard.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use crate::boat::{identity, offline, Sandbox, POLL_INTERVAL};
use crate::config::Server;
use crate::panel::Panel;
use multitop_agent::{proto::Payload, render::Snapshot};

#[derive(Default)]
pub struct State {
    pub inventory: Vec<Sandbox>,
    pub visible: BTreeSet<String>,
    pub samples: BTreeMap<String, Instant>,
    pub auto_clear: bool,
    pub list_open: bool,
    pub list_scroll: usize,
    pub discovery_notice: Option<String>,
    pub discovered: bool,
    dismissed: BTreeSet<String>,
}

impl State {
    pub fn select(&mut self, inventory: Vec<Sandbox>) -> Vec<Server> {
        self.inventory = inventory;
        self.discovered = true;
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
            .chain(
                self.list_rows()
                    .filter(|boat| self.visible.contains(&boat.id))
                    .map(Sandbox::server),
            )
            .collect()
    }

    pub(crate) fn fresh_sample<'a>(&self, panel: &'a Panel) -> Option<&'a Snapshot> {
        let host = &panel.server.host;
        let id = identity(host).unwrap_or(host);
        if offline(host)
            || self
                .samples
                .get(id)
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
    format!("{view}  c clear  C auto {auto}  G graphs  s stats | {running} up | Fleet CPU {cpu} ({reporting}/{running} reporting){hidden} | / filter  q quit")
}
