//! A fixed metric summary for short tiles; only the process table below it
//! receives the panel's scroll offset.

use crate::{
    boat_fleet::{Display, StatsPanel},
    panel::Panel,
};
use multitop_agent::{
    fmt::{fmt_rate, fmt_size},
    render::Snapshot,
};
use ratatui::{
    layout::{Alignment, Rect},
    text::Line,
    widgets::Paragraph,
    Frame,
};

use super::fleet_stats_style::{StatsStyle, CPU, SWAP, USED};
use super::meters::{split_x, split_y};

const LABEL_WIDTH: u16 = 10;
const VALUES_WIDTH: u16 = 27;
const MIN_BAR_WIDTH: u16 = 4;

pub fn draw(
    f: &mut Frame,
    panel: &Panel,
    snap: &Snapshot,
    area: Rect,
    style: StatsStyle,
    display: &Display,
) -> usize {
    let swap_values = snap.swap.map_or_else(
        || "unavailable".into(),
        |usage| format!("{} / {}", fmt_size(usage.used), fmt_size(usage.total)),
    );
    let metrics = [
        (
            !display.hidden(StatsPanel::Cpu),
            "CPU",
            Some(snap.cpu_pct),
            String::new(),
            CPU,
        ),
        (
            !display.hide_memory,
            "MEM",
            Some(snap.mem.pct),
            format!("{} / {}", fmt_size(snap.mem.used), fmt_size(snap.mem.total)),
            USED,
        ),
        (
            !display.hide_memory,
            "SWAP",
            snap.swap.map(|usage| usage.pct),
            swap_values,
            SWAP,
        ),
        (
            !display.hidden(StatsPanel::Network),
            "NET",
            None,
            format!("↑ {} ↓ {}", fmt_rate(snap.tx_rate), fmt_rate(snap.rx_rate)),
            CPU,
        ),
        (
            !display.hidden(StatsPanel::Disks),
            "DSK",
            Some(snap.disk.pct),
            format!(
                "{} / {}",
                fmt_size(snap.disk.used),
                fmt_size(snap.disk.total)
            ),
            USED,
        ),
    ];
    let visible = metrics.into_iter().filter(|(show, ..)| *show);
    let row_count = u16::try_from(visible.clone().count()).unwrap_or(0);
    let (summary, processes) = split_y(area, row_count);
    for ((_, name, pct, values, colors), y) in visible.zip(summary.y..summary.bottom()) {
        let row = Rect {
            y,
            height: 1,
            ..summary
        };
        let (label, rest) = split_x(row, LABEL_WIDTH);
        let label_text = pct.map_or_else(|| name.to_owned(), |pct| format!("{name} {pct:>3.0}%"));
        f.render_widget(Paragraph::new(label_text).style(style.text()), label);
        let value_width = VALUES_WIDTH.min(rest.width.saturating_sub(MIN_BAR_WIDTH));
        let (bar, value) = split_x(rest, rest.width - value_width);
        let meter = pct.map_or_else(Line::default, |pct| style.meter(pct, bar.width, colors));
        f.render_widget(Paragraph::new(meter), bar);
        f.render_widget(
            Paragraph::new(values)
                .alignment(Alignment::Right)
                .style(style.text()),
            value,
        );
    }
    if display.hidden(StatsPanel::Processes) {
        0
    } else {
        super::fleet_stats::draw_processes(f, panel, snap, processes, style)
    }
}
