//! Fleet scrolling belongs to the process table, never the metric tile.

use super::App;
use crate::panel::{Mode, Panel};

impl App {
    fn scrollable_panel(&mut self, index: usize) -> Option<&mut Panel> {
        let fleet = self.boat.is_some();
        let panel = self.panels.get_mut(index)?;
        let processes = matches!(&panel.last_monitor,
            Some(multitop_agent::proto::Payload::Monitor(snap)) if !snap.procs.is_empty());
        if fleet && (panel.mode != Mode::Monitor || !processes) {
            panel.scroll_offset = 0;
            return None;
        }
        Some(panel)
    }

    pub fn scroll_up(&mut self, delta: usize) {
        self.scroll_panel_up(self.selected_panel, delta);
    }

    pub fn scroll_down(&mut self, delta: usize) {
        self.scroll_panel_down(self.selected_panel, delta);
    }

    pub fn scroll_panel_up(&mut self, index: usize, delta: usize) {
        let fleet = self.boat.is_some();
        if let Some(panel) = self.scrollable_panel(index) {
            panel.scroll_offset = if fleet {
                panel.scroll_offset.saturating_sub(delta)
            } else {
                panel.scroll_offset.saturating_add(delta)
            };
        }
    }

    pub fn scroll_panel_down(&mut self, index: usize, delta: usize) {
        let fleet = self.boat.is_some();
        if let Some(panel) = self.scrollable_panel(index) {
            panel.scroll_offset = if fleet {
                panel.scroll_offset.saturating_add(delta)
            } else {
                panel.scroll_offset.saturating_sub(delta)
            };
        }
    }

    pub fn scroll_to_top(&mut self) {
        let fleet = self.boat.is_some();
        if let Some(panel) = self.scrollable_panel(self.selected_panel) {
            panel.scroll_offset = if fleet { 0 } else { Self::SCROLL_TOP };
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        let fleet = self.boat.is_some();
        if let Some(panel) = self.scrollable_panel(self.selected_panel) {
            panel.scroll_offset = if fleet { Self::SCROLL_TOP } else { 0 };
        }
    }
}
