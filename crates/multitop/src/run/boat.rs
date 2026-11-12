//! Periodic Boat discovery and identity-preserving panel reconciliation.

use crate::app::App;
use crate::config::Server;
use crate::panel::{Mode, Panel};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const STALE: &str = "Boat discovery unavailable; fleet state may be stale: ";

pub(super) struct Feed {
    rx: mpsc::Receiver<Result<Vec<Server>, String>>,
    task: JoinHandle<()>,
}

impl Feed {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel(1);
        let task = tokio::spawn(async move {
            loop {
                let result = crate::boat::discover().await;
                if tx.send(result).await.is_err() {
                    break;
                }
                tokio::time::sleep(crate::boat::POLL_INTERVAL).await;
            }
        });
        Self { rx, task }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) async fn next(feed: &mut Option<Feed>) -> Result<Vec<Server>, String> {
    match feed {
        Some(feed) => match feed.rx.recv().await {
            Some(update) => update,
            None => std::future::pending().await,
        },
        None => std::future::pending().await,
    }
}

/// Returns true only when the monitor task list must be restarted. A failed
/// discovery keeps the last inventory; a successful one clears the stale note.
pub(super) fn reconcile(app: &mut App, update: Result<Vec<Server>, String>) -> bool {
    let servers = match update {
        Ok(servers) => servers,
        Err(error) => {
            for p in &mut app.panels {
                p.notes.retain(|n| !n.starts_with(STALE));
                p.note(format!("{STALE}{error}"));
            }
            return false;
        }
    };
    let servers = if let Some(state) = &mut app.boat {
        state.select(servers)
    } else {
        servers
    };
    for p in &mut app.panels {
        p.notes.retain(|n| !n.starts_with(STALE));
    }
    if app.panels.iter().map(|p| &p.server).eq(servers.iter()) {
        return false;
    }
    // User-started upgrades retain their task ownership until complete. Boat
    // inventories never configure upgrades, but Settings can add one.
    if app.upgrades_in_flight() {
        return false;
    }
    let selected = app
        .panels
        .get(app.selected_panel)
        .and_then(|p| crate::boat::identity(&p.server.host))
        .map(str::to_owned);
    let focused = app
        .focused_panel
        .and_then(|i| app.panels.get(i))
        .and_then(|p| crate::boat::identity(&p.server.host))
        .map(str::to_owned);
    app.panels_epoch += 1;
    let generation = app.panels.iter().map(|p| p.gen).max().unwrap_or(0) + 1;
    let mut old = std::mem::take(&mut app.panels);
    for server in servers {
        let previous = old.iter().position(|p| {
            crate::boat::identity(&p.server.host) == crate::boat::identity(&server.host)
        });
        let mut panel = previous.map_or_else(|| Panel::new(server.clone()), |i| old.remove(i));
        let changed = panel.server.host != server.host;
        panel.server = server;
        panel.gen = generation;
        panel.mode = panel.mode.for_startup();
        panel.last_upgrade.set_cap(app.upgrade_history_lines);
        if crate::boat::offline(&panel.server.host) {
            panel.last_monitor = None;
            panel.last_frame = None;
            panel.mode = Mode::Monitor;
            panel.show_body(["Not running. No SSH connection or automatic resume.".into()]);
        } else if changed {
            panel.last_monitor = None;
            panel.last_frame = None;
            panel.show_body(["Connecting to Boat…".into()]);
        }
        app.panels.push(panel);
    }
    let position = |id: Option<&str>| {
        app.panels
            .iter()
            .position(|p| crate::boat::identity(&p.server.host) == id)
    };
    app.selected_panel = position(selected.as_deref()).unwrap_or(0);
    app.focused_panel = focused.as_deref().and_then(|id| position(Some(id)));
    true
}

/// A click acknowledges only a stopped tile; running tiles keep ordinary focus.
pub(super) fn dismiss(
    app: &mut App,
    mouse: crossterm::event::MouseEvent,
    size: Option<(u16, u16)>,
) -> bool {
    use crossterm::event::{MouseButton, MouseEventKind};
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
        return false;
    }
    let Some((w, h)) = size else { return false };
    let Some(i) = super::event_loop::panel_at_pos(
        mouse.column,
        mouse.row,
        ratatui::layout::Rect::new(0, 0, w, h),
        &app.filtered_indices(),
    ) else {
        return false;
    };
    let host = app.panels[i].server.host.clone();
    if !crate::boat::offline(&host) {
        return false;
    }
    let Some(id) = crate::boat::identity(&host).filter(|id| *id != "waiting") else {
        return false;
    };
    let Some(state) = &mut app.boat else {
        return false;
    };
    state.visible.remove(id);
    let inventory = state.inventory.clone();
    reconcile(app, Ok(inventory))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet(states: &[(&str, &str)]) -> Vec<Server> {
        let sandboxes: Vec<_> = states
            .iter()
            .map(|(id, state)| serde_json::json!({"id":id,"name":id,"state":state}))
            .collect();
        crate::boat::parse_inventory(
            &serde_json::to_vec(&serde_json::json!({"sandboxes":sandboxes})).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn discovery_keeps_selection_and_scrollback_when_another_boat_is_deleted() {
        let mut app = App::new(fleet(&[("bx_a", "idle"), ("bx_b", "idle")]));
        app.selected_panel = 1;
        app.focused_panel = Some(1);
        app.panels[1].note("keep this history".into());
        app.panels[1].mode = Mode::Graphs;
        let old_epoch = app.panels_epoch;
        assert!(reconcile(&mut app, Ok(fleet(&[("bx_b", "idle")]))));
        assert!(app.panels_epoch > old_epoch);
        assert_eq!(app.selected_panel, 0);
        assert_eq!(app.focused_panel, Some(0));
        assert!(app.panels[0].notes.contains(&"keep this history".into()));
        assert_eq!(app.panels[0].mode, Mode::Graphs);
        assert!(!app.apply(crate::app::Msg::Frame {
            panel: 0,
            epoch: old_epoch,
            lines: vec!["old boat".into()],
        }));
    }

    #[test]
    fn unchanged_inventory_does_not_restart_monitors_and_failures_keep_the_fleet() {
        let initial = fleet(&[("bx_a", "idle")]);
        let mut app = App::new(initial.clone());
        assert!(!reconcile(&mut app, Ok(initial.clone())));
        assert!(!reconcile(&mut app, Err("offline".into())));
        assert_eq!(app.panels.len(), 1);
        assert!(app.panels[0].notes.iter().any(|n| n.starts_with(STALE)));
        assert!(!reconcile(&mut app, Ok(initial)));
        assert_eq!(app.panels[0].notes, [] as [String; 0]);
    }

    #[test]
    fn stopping_and_resuming_invalidates_old_samples_without_losing_identity() {
        let mut app = App::new(fleet(&[("bx_a", "idle")]));
        app.panels[0].last_frame = Some(vec!["stale CPU".into()]);
        assert!(reconcile(&mut app, Ok(fleet(&[("bx_a", "stopped")]))));
        assert!(app.panels[0].last_frame.is_none());
        assert!(crate::boat::offline(&app.panels[0].server.host));
        assert!(reconcile(&mut app, Ok(fleet(&[("bx_a", "idle")]))));
        assert!(!crate::boat::offline(&app.panels[0].server.host));
        assert!(app.panels[0].view.iter().any(|s| s.contains("Connecting")));
    }
    #[test]
    fn clicking_a_stopped_tile_dismisses_it_but_a_running_tile_is_only_selected() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let mut app = App::new(vec![crate::boat::waiting("starting")]);
        app.boat = Some(crate::boat_fleet::State::default());
        assert!(reconcile(
            &mut app,
            Ok(fleet(&[("bx_a", "idle"), ("bx_b", "stopped")]))
        ));
        let (tiles, _) = crate::ui::regions(
            ratatui::layout::Rect::new(0, 0, 160, 40),
            app.visible_panes(),
        );
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: tiles[1].x + 2,
            row: tiles[1].y + 2,
            modifiers: KeyModifiers::NONE,
        };
        assert!(!dismiss(&mut app, click, Some((160, 40))));
        assert!(reconcile(
            &mut app,
            Ok(fleet(&[("bx_a", "stopped"), ("bx_b", "stopped")]))
        ));
        assert_eq!(app.panels.len(), 2);
        assert!(dismiss(&mut app, click, Some((160, 40))));
        assert_eq!(app.panels.len(), 1);
        assert!(crate::ssh::is_local(&app.panels[0].server));
        assert_eq!(app.boat.as_ref().unwrap().visible.len(), 0);
    }
}
