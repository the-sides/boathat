//! A fixed metric summary for short tiles; only the process table below it
//! receives the panel's scroll offset.

use crate::panel::Panel;
use multitop_agent::{
    fmt::{fmt_rate, fmt_size},
    palette_view::PaletteView,
    render::Snapshot,
};
use ratatui::{
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::meters::{meter, split_x, split_y, style};

const LABEL_WIDTH: u16 = 10;
const VALUES_WIDTH: u16 = 27;
const SUMMARY_ROWS: u16 = 4;
const MIN_BAR_WIDTH: u16 = 4;

pub fn draw(f: &mut Frame, panel: &Panel, snap: &Snapshot, area: Rect, pal: &PaletteView) -> usize {
    if area.height < SUMMARY_ROWS {
        let summary = format!(
            "CPU {:.0}%  MEM {:.0}%  DSK {:.0}%",
            snap.cpu_pct, snap.mem.pct, snap.disk.pct
        );
        f.render_widget(Paragraph::new(summary).style(style(pal.primary())), area);
        return 0;
    }
    let swap_values = snap.swap.map_or_else(
        || "unavailable".into(),
        |usage| format!("{} / {}", fmt_size(usage.used), fmt_size(usage.total)),
    );
    let metrics = [
        (
            "CPU",
            snap.cpu_pct,
            format!(
                "NET ↑ {} ↓ {}",
                fmt_rate(snap.tx_rate),
                fmt_rate(snap.rx_rate)
            ),
        ),
        (
            "MEM",
            snap.mem.pct,
            format!("{} / {}", fmt_size(snap.mem.used), fmt_size(snap.mem.total)),
        ),
        (
            "DSK",
            snap.disk.pct,
            format!(
                "{} / {}",
                fmt_size(snap.disk.used),
                fmt_size(snap.disk.total)
            ),
        ),
        (
            "SWAP",
            snap.swap.map_or(0.0, |usage| usage.pct),
            swap_values,
        ),
    ];
    let (summary, processes) = split_y(area, SUMMARY_ROWS);
    for ((name, pct, values), y) in metrics.into_iter().zip(summary.y..summary.bottom()) {
        let row = Rect {
            y,
            height: 1,
            ..summary
        };
        let (label, rest) = split_x(row, LABEL_WIDTH);
        let label_text = if name == "SWAP" && snap.swap.is_none() {
            "SWAP   --".into()
        } else {
            format!("{name} {pct:>3.0}%")
        };
        f.render_widget(
            Paragraph::new(label_text).style(style(pal.primary())),
            label,
        );
        let value_width = VALUES_WIDTH.min(rest.width.saturating_sub(MIN_BAR_WIDTH));
        let (bar, value) = split_x(rest, rest.width - value_width);
        // Unavailable swap is a missing measurement, not a measured zero.
        let spans = if name == "SWAP" && snap.swap.is_none() {
            vec![Span::styled("unavailable", style(pal.muted()))]
        } else {
            meter(pct, bar.width, pal).spans
        };
        f.render_widget(Paragraph::new(Line::from(spans)), bar);
        f.render_widget(
            Paragraph::new(values)
                .alignment(Alignment::Right)
                .style(style(pal.text())),
            value,
        );
    }
    super::fleet_stats::draw_processes(f, panel, snap, processes, pal)
}
