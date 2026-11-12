//! Session-local visibility and aggregate CPU for the Boat grid.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use crate::boat::{identity, offline, POLL_INTERVAL};
use crate::config::Server;

#[derive(Default)]
pub struct State {
    pub inventory: Vec<Server>,
    pub visible: BTreeSet<String>,
    pub samples: BTreeMap<String, Instant>,
}

impl State {
    pub fn select(&mut self, mut inventory: Vec<Server>) -> Vec<Server> {
        inventory.retain(|s| identity(&s.host) != Some("waiting"));
        self.inventory = inventory;
        self.visible
            .retain(|id| self.inventory.iter().any(|s| identity(&s.host) == Some(id)));
        self.samples
            .retain(|id, _| id == "localhost" || self.visible.contains(id));
        for server in &self.inventory {
            if !offline(&server.host) {
                if let Some(id) = identity(&server.host) {
                    self.visible.insert(id.into());
                }
            }
        }
        let mut shown: Vec<_> = self
            .inventory
            .iter()
            .filter(|s| identity(&s.host).is_some_and(|id| self.visible.contains(id)))
            .cloned()
            .collect();
        shown.insert(0, crate::boat::local_server());
        shown
    }
}

/// Compute from fresh samples only, weighted by each reporting VM's core count.
/// Missing hosts are named as missing coverage rather than counted as idle.
#[must_use]
pub fn summary(app: &crate::app::App) -> String {
    let Some(state) = &app.boat else {
        return String::new();
    };
    let running = state.inventory.iter().filter(|s| !offline(&s.host)).count();
    let hidden = state
        .inventory
        .iter()
        .filter(|s| {
            offline(&s.host) && identity(&s.host).is_some_and(|id| !state.visible.contains(id))
        })
        .count();
    let mut weighted = 0.0;
    let mut cores = 0_u32;
    let mut reporting = 0;
    for panel in &app.panels {
        let Some(id) = identity(&panel.server.host) else {
            continue;
        };
        if offline(&panel.server.host)
            || state
                .samples
                .get(id)
                .is_none_or(|at| at.elapsed() > POLL_INTERVAL)
        {
            continue;
        }
        if let Some(multitop_agent::proto::Payload::Monitor(snap)) = &panel.last_monitor {
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
    format!("q quit | {running} running | {hidden} stopped hidden | Fleet CPU {cpu} ({reporting}/{running} reporting) | G graphs | z zoom | click stopped tile to dismiss")
}
