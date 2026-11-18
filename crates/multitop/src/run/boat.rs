//! Periodic Boat discovery and identity-preserving panel reconciliation.

use std::path::Path;

use crate::app::App;
use crate::boat::{Sandbox, Usage, USAGE_INTERVAL};
use crate::config::Server;
use crate::panel::{Mode, Panel};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};

const STALE: &str = "Boat discovery unavailable; fleet state may be stale: ";
const USAGE_CONCURRENCY: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum View {
    List,
    #[default]
    Graphs,
    Stats,
}

impl View {
    pub fn current(app: &App) -> Option<Self> {
        if app.boat.as_ref()?.list_open {
            return Some(Self::List);
        }
        match app.panels.first()?.mode {
            Mode::Graphs => Some(Self::Graphs),
            Mode::Monitor => Some(Self::Stats),
            _ => None,
        }
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path).as_deref().map(str::trim) {
            Ok("list") => Self::List,
            Ok("stats") => Self::Stats,
            _ => Self::Graphs,
        }
    }

    pub fn save(self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let value = match self {
            Self::List => "list\n",
            Self::Graphs => "graphs\n",
            Self::Stats => "stats\n",
        };
        crate::state::write_atomic(path, value)
    }
}

pub(super) enum Update {
    Inventory(Result<Vec<Sandbox>, String>),
    Usage(String, Option<Usage>),
}

pub(super) struct Feed {
    rx: mpsc::Receiver<Update>,
    usage_targets: watch::Sender<Vec<String>>,
    task: JoinHandle<()>,
}

impl Feed {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel(1);
        let (usage_targets, targets) = watch::channel(Vec::new());
        let task = tokio::spawn(async move {
            tokio::join!(poll_inventory(&tx), poll_usage(&tx, targets));
        });
        Self {
            rx,
            usage_targets,
            task,
        }
    }

    pub fn apply(&self, app: &mut App, update: Update) -> bool {
        match update {
            Update::Inventory(inventory) => {
                reconcile(app, inventory);
                self.sync_usage_targets(app);
                true
            }
            Update::Usage(id, usage) => apply_usage(app, id, usage),
        }
    }

    pub fn sync_usage_targets(&self, app: &App) {
        let targets = app
            .boat
            .as_ref()
            .filter(|state| state.list_open && state.discovery_notice.is_none())
            .map_or_else(Vec::new, |state| {
                state
                    .inventory
                    .iter()
                    .filter(|boat| boat.is_running())
                    .map(|boat| boat.id.clone())
                    .collect()
            });
        self.usage_targets.send_if_modified(|previous| {
            if *previous == targets {
                return false;
            }
            *previous = targets;
            true
        });
    }
}

async fn poll_inventory(tx: &mpsc::Sender<Update>) {
    loop {
        let result = crate::boat::discover().await;
        if tx.send(Update::Inventory(result)).await.is_err() {
            return;
        }
        tokio::time::sleep(crate::boat::POLL_INTERVAL).await;
    }
}

async fn poll_usage(tx: &mpsc::Sender<Update>, mut targets: watch::Receiver<Vec<String>>) {
    loop {
        let ids = targets.borrow_and_update().clone();
        tokio::select! {
            () = usage_round(tx, &ids), if !ids.is_empty() => {}
            changed = targets.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
    }
}

async fn usage_round(tx: &mpsc::Sender<Update>, ids: &[String]) {
    let mut pending = JoinSet::new();
    for group in ids.chunks(USAGE_CONCURRENCY) {
        for id in group.iter().cloned() {
            pending.spawn(async move {
                let usage = crate::boat::usage(&id).await;
                Update::Usage(id, usage)
            });
        }
        while let Some(result) = pending.join_next().await {
            let Ok(update) = result else { continue };
            if tx.send(update).await.is_err() {
                return;
            }
        }
    }
    tokio::time::sleep(USAGE_INTERVAL).await;
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) async fn next(feed: &mut Option<Feed>) -> Update {
    match feed {
        Some(feed) => match feed.rx.recv().await {
            Some(update) => update,
            None => std::future::pending().await,
        },
        None => std::future::pending().await,
    }
}

fn apply_usage(app: &mut App, id: String, usage: Option<Usage>) -> bool {
    let Some(state) = &mut app.boat else {
        return false;
    };
    if state.discovery_notice.is_some()
        || !state
            .inventory
            .iter()
            .any(|boat| boat.id == id && boat.is_running())
    {
        return false;
    }
    if let Some(usage) = usage {
        state.usage.insert(id, usage);
    } else {
        state.usage.remove(&id);
    }
    state.list_open
}

/// Returns true only when the monitor task list must be restarted. A failed
/// discovery keeps the last inventory; a successful one clears the stale note.
pub(super) fn reconcile(app: &mut App, update: Result<Vec<Sandbox>, String>) -> bool {
    let Some(state) = &mut app.boat else {
        return false;
    };
    let servers = match update {
        Ok(inventory) => {
            state.discovery_notice = None;
            state.select(inventory)
        }
        Err(error) => {
            let message = format!("{STALE}{error}");
            state.discovery_notice = Some(message.clone());
            for p in &mut app.panels {
                p.notes.retain(|n| !n.starts_with(STALE));
                p.note(message.clone());
            }
            return false;
        }
    };
    for p in &mut app.panels {
        p.notes.retain(|n| !n.starts_with(STALE));
    }
    rebuild_panels(app, servers)
}

fn rebuild_panels(app: &mut App, servers: Vec<Server>) -> bool {
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
    // View switches apply to the fleet. The permanent local tile keeps that
    // choice even when the selected boat stops and shows its offline status.
    let fleet_mode = app
        .panels
        .iter()
        .find(|p| crate::ssh::is_local(&p.server))
        .or_else(|| {
            app.panels
                .iter()
                .find(|p| !crate::boat::offline(&p.server.host))
        })
        .map_or(Mode::Monitor, |p| p.mode.for_startup());
    let mut old = std::mem::take(&mut app.panels);
    for server in servers {
        let previous = old.iter().position(|p| {
            crate::boat::identity(&p.server.host) == crate::boat::identity(&server.host)
        });
        let mut panel = previous.map_or_else(|| Panel::new(server.clone()), |i| old.remove(i));
        let changed = panel.server.host != server.host;
        panel.server = server;
        panel.gen = generation;
        panel.mode = if previous.is_none() || changed {
            fleet_mode
        } else {
            panel.mode.for_startup()
        };
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

/// Boat-specific keys. Run after text entry and confirmation have had a turn.
pub(super) fn keys(key: KeyEvent, app: &mut App) -> bool {
    let Some(state) = &mut app.boat else {
        return false;
    };
    if matches!(key.code, KeyCode::Char('c' | 'C'))
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        if key.code == KeyCode::Char('C') {
            state.auto_clear = !state.auto_clear;
        }
        if key.code == KeyCode::Char('c') || state.auto_clear {
            state.clear_dead();
            let servers = state.servers();
            rebuild_panels(app, servers);
        }
        return true;
    }
    if matches!(key.code, KeyCode::Char('l' | 'L')) {
        state.list_open = !state.list_open;
        state.list_scroll = 0;
        app.focused_panel = None;
        return true;
    }
    if !state.list_open {
        return false;
    }
    match key.code {
        KeyCode::Char('g' | 'G' | 's' | 'S') => {
            state.list_open = false;
            return false;
        }
        KeyCode::Esc if app.filter_query.trim().is_empty() => state.list_open = false,
        KeyCode::Up | KeyCode::Char('k' | 'K') => {
            state.list_scroll = state.list_scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j' | 'J') => {
            state.list_scroll = state.list_scroll.saturating_add(1);
        }
        KeyCode::PageUp => state.list_scroll = state.list_scroll.saturating_sub(15),
        KeyCode::PageDown => state.list_scroll = state.list_scroll.saturating_add(15),
        KeyCode::Home => state.list_scroll = 0,
        KeyCode::End => state.list_scroll = usize::MAX,
        KeyCode::Esc | KeyCode::Char('/' | 'q' | 'Q' | 't' | 'T') => return false,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return false,
        _ => {}
    }
    true
}

/// The list scrolls independently. A click in the grid can dismiss a dead tile.
pub(super) fn handle_mouse(app: &mut App, mouse: MouseEvent, size: Option<(u16, u16)>) -> bool {
    use crossterm::event::MouseButton;
    let Some(state) = &mut app.boat else {
        return false;
    };
    if state.list_open {
        match mouse.kind {
            MouseEventKind::ScrollUp => state.list_scroll = state.list_scroll.saturating_sub(3),
            MouseEventKind::ScrollDown => state.list_scroll = state.list_scroll.saturating_add(3),
            _ => {}
        }
        return true;
    }
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
    let host = &app.panels[i].server.host;
    if !crate::boat::offline(host) {
        return false;
    }
    let Some(id) = crate::boat::identity(host) else {
        return false;
    };
    let Some(state) = &mut app.boat else {
        return false;
    };
    state.dismiss(id);
    let servers = state.servers();
    rebuild_panels(app, servers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet(states: &[(&str, &str)]) -> Vec<Sandbox> {
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
    fn boat_view_round_trips_to_the_next_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences/boat-view");
        assert_eq!(View::load(&path), View::Graphs);
        for view in [View::List, View::Stats, View::Graphs] {
            view.save(&path).unwrap();
            let app = super::super::boot::boot_boat_app(
                &[crate::boat::local_server()],
                View::load(&path),
            );
            assert_eq!(View::current(&app), Some(view));
        }
    }

    #[test]
    fn discovery_keeps_selection_and_scrollback_when_another_boat_is_deleted() {
        let mut app =
            super::super::boot::boot_boat_app(&[crate::boat::local_server()], View::Graphs);
        assert!(reconcile(
            &mut app,
            Ok(fleet(&[("bx_a", "idle"), ("bx_b", "idle")]))
        ));
        app.selected_panel = 2;
        app.focused_panel = Some(2);
        app.panels[2].note("keep this history".into());
        let old_epoch = app.panels_epoch;
        assert!(reconcile(&mut app, Ok(fleet(&[("bx_b", "idle")]))));
        assert!(app.panels_epoch > old_epoch);
        assert_eq!(app.selected_panel, 1);
        assert_eq!(app.focused_panel, Some(1));
        assert!(app.panels[1].notes.contains(&"keep this history".into()));
        assert_eq!(app.panels[1].mode, Mode::Graphs);
        assert!(!app.apply(crate::app::Msg::Frame {
            panel: 0,
            epoch: old_epoch,
            lines: vec!["old boat".into()],
        }));
    }

    #[test]
    fn new_boats_inherit_graphs_or_stats_without_losing_existing_history() {
        for mode in [Mode::Monitor, Mode::Graphs] {
            let mut app = App::new(vec![crate::boat::local_server()]);
            app.boat = Some(crate::boat_fleet::State::default());
            assert!(reconcile(&mut app, Ok(fleet(&[("bx_a", "idle")]))));
            if mode == Mode::Graphs {
                app.toggle_graphs((120, 40));
            }
            app.graph_zoom = 3;
            app.panels[1].history.cpu.push(55.0);
            let history = app.panels[1].history.clone();
            assert!(reconcile(
                &mut app,
                Ok(fleet(&[("bx_a", "idle"), ("bx_b", "idle")]))
            ));
            assert!(app.panels.iter().all(|p| p.mode == mode));
            assert_eq!(app.graph_zoom, 3);
            assert_eq!(app.panels[1].history, history);
            let index = app
                .panels
                .iter()
                .position(|p| p.server.host == "boat:bx_b")
                .unwrap();
            assert!(app.apply(crate::app::Msg::Packet {
                panel: index,
                epoch: app.panels_epoch,
                gen: app.panels[index].gen,
                dims: (100, 30),
                payload: multitop_agent::proto::Payload::Monitor(
                    multitop_agent::render::Snapshot {
                        cpu_pct: 25.0,
                        ..multitop_agent::render::Snapshot::default()
                    }
                ),
            }));
            assert_eq!(app.panels[index].mode, mode);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(300, 80)).unwrap();
            terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();
            let (tiles, _) = crate::ui::regions(ratatui::layout::Rect::new(0, 0, 300, 80), 3);
            let tile = tiles[index];
            let buffer = terminal.backend().buffer();
            let text: String = (tile.y..tile.bottom())
                .flat_map(|y| (tile.x..tile.right()).map(move |x| (x, y)))
                .map(|point| buffer[point].symbol())
                .collect();
            assert!(text.contains("CPU") && text.contains("MEM"));
            assert_eq!(text.contains("processes"), mode == Mode::Monitor);
        }
    }

    #[test]
    fn resumed_boat_inherits_graph_view_even_if_offline_tile_is_selected() {
        let mut app = App::new(vec![crate::boat::local_server()]);
        app.boat = Some(crate::boat_fleet::State::default());
        assert!(reconcile(&mut app, Ok(fleet(&[("bx_a", "idle")]))));
        app.toggle_graphs((120, 40));
        app.selected_panel = 1;
        assert!(reconcile(&mut app, Ok(fleet(&[("bx_a", "stopped")]))));
        assert_eq!(app.panels[1].mode, Mode::Monitor);
        assert!(reconcile(
            &mut app,
            Ok(fleet(&[("bx_a", "idle"), ("bx_b", "idle")]))
        ));
        assert!(app.panels.iter().all(|p| p.mode == Mode::Graphs));
    }

    #[test]
    fn unchanged_inventory_does_not_restart_monitors_and_failures_keep_the_fleet() {
        let initial = fleet(&[("bx_a", "idle")]);
        let mut app =
            super::super::boot::boot_boat_app(&[crate::boat::local_server()], View::Graphs);
        assert!(reconcile(&mut app, Ok(initial)));
        assert!(!reconcile(&mut app, Ok(fleet(&[("bx_a", "idle")]))));
        assert!(!reconcile(&mut app, Err("offline".into())));
        assert_eq!(app.panels.len(), 2);
        assert!(app.panels[0].notes.iter().any(|n| n.starts_with(STALE)));
        assert!(!reconcile(&mut app, Ok(fleet(&[("bx_a", "idle")]))));
        assert_eq!(app.panels[0].notes, [] as [String; 0]);
    }

    #[test]
    fn stopping_and_resuming_invalidates_old_samples_without_losing_identity() {
        let mut app =
            super::super::boot::boot_boat_app(&[crate::boat::local_server()], View::Graphs);
        assert!(reconcile(&mut app, Ok(fleet(&[("bx_a", "idle")]))));
        app.panels[1].last_frame = Some(vec!["stale CPU".into()]);
        assert!(reconcile(&mut app, Ok(fleet(&[("bx_a", "stopped")]))));
        assert!(app.panels[1].last_frame.is_none());
        assert!(crate::boat::offline(&app.panels[1].server.host));
        assert!(reconcile(&mut app, Ok(fleet(&[("bx_a", "idle")]))));
        assert!(!crate::boat::offline(&app.panels[1].server.host));
        assert!(app.panels[1].view.iter().any(|s| s.contains("Connecting")));
    }
    #[test]
    fn clicking_a_stopped_tile_dismisses_it_but_a_running_tile_is_only_selected() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let mut app = App::new(vec![crate::boat::local_server()]);
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
        assert!(!handle_mouse(&mut app, click, Some((160, 40))));
        assert!(reconcile(
            &mut app,
            Ok(fleet(&[("bx_a", "stopped"), ("bx_b", "stopped")]))
        ));
        assert_eq!(app.panels.len(), 2);
        assert!(handle_mouse(&mut app, click, Some((160, 40))));
        assert_eq!(app.panels.len(), 1);
        assert!(crate::ssh::is_local(&app.panels[0].server));
        assert_eq!(app.boat.as_ref().unwrap().visible.len(), 0);
    }

    #[test]
    fn clear_keys_handle_current_and_future_stops_and_allow_resumes() {
        let mut app =
            super::super::boot::boot_boat_app(&[crate::boat::local_server()], View::Graphs);
        let (tx, _) = mpsc::channel(1);
        let (_, dims) = tokio::sync::watch::channel((120, 40));
        let dims = std::sync::Arc::new(dims);
        let mut tasks = super::super::Tasks::new(1);
        let mut press = |app: &mut App, key| {
            super::super::handle_key(
                KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE),
                app,
                (120, 40),
                &dims,
                &tx,
                &mut tasks,
            );
        };
        let rows = |app: &App| {
            app.boat
                .as_ref()
                .unwrap()
                .list_rows()
                .map(|b| b.id.clone())
                .collect::<Vec<_>>()
        };
        let initial = [("bx_a", "stopped"), ("bx_b", "idle"), ("bx_c", "pending")];
        reconcile(&mut app, Ok(fleet(&initial)));
        press(&mut app, 'l');
        assert!(app.boat.as_ref().unwrap().list_open);
        assert_eq!(rows(&app), ["bx_b", "bx_c", "bx_a"]);

        press(&mut app, 'c');
        assert!(!app.boat.as_ref().unwrap().auto_clear);
        reconcile(&mut app, Ok(fleet(&initial)));
        assert_eq!(rows(&app), ["bx_b", "bx_c"]);

        let newer = [
            ("bx_a", "stopped"),
            ("bx_b", "stopped"),
            ("bx_c", "pending"),
        ];
        reconcile(&mut app, Ok(fleet(&newer)));
        assert_eq!(rows(&app), ["bx_c", "bx_b"]);
        press(&mut app, 'C');
        assert!(app.boat.as_ref().unwrap().auto_clear);
        assert_eq!(rows(&app), ["bx_c"]);
        assert_eq!(app.panels.len(), 1);
        reconcile(
            &mut app,
            Ok(fleet(&[("bx_a", "idle"), ("bx_d", "stopped")])),
        );
        assert_eq!(rows(&app), ["bx_a"]);
        press(&mut app, 'C');
        reconcile(
            &mut app,
            Ok(fleet(&[("bx_a", "stopped"), ("bx_d", "stopped")])),
        );
        assert_eq!(rows(&app), ["bx_a"]);
        press(&mut app, 'G');
        assert!(!app.boat.as_ref().unwrap().list_open);
        assert_eq!(app.panels[0].mode, Mode::Graphs);
    }
}
