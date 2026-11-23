//! Fixed summaries for the local machine and SSH hosts above the inventory.

use multitop_agent::palette_view::PaletteView;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::{app::App, boat_fleet::State, graphs::dots_for, history::Series, panel::Panel};

use super::super::{ink, meters};
use super::{compact_size, cpu, memory, MISSING};

const HOST_ROWS: u16 = 3;
const TABLE_ROWS: u16 = 3; // Section title, column labels and one inventory row.
const FULL_MEMORY_WIDTH: u16 = 64;
const MIN_PLOT_WIDTH: usize = 8;
const MAX_PLOT_WIDTH: usize = 14;
const GAP: &str = "  ";
const CPU_LABEL: &str = "CPU ";
const MEM_LABEL: &str = "MEM ";
const BAR_LEVELS: [char; 9] = ['·', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const FULL_PERCENT: f64 = 100.0;

pub(super) fn draw<'a>(
    f: &mut Frame,
    body: Rect,
    app: &App,
    state: &State,
    theme: &PaletteView,
    ssh_hosts: &'a [&'a Panel],
) -> (Rect, &'a [&'a Panel]) {
    let expanded = body.height >= HOST_ROWS + TABLE_ROWS;
    let local_rows = if expanded { HOST_ROWS } else { HOST_ROWS - 1 };
    let ssh_capacity = body.height.saturating_sub(local_rows + TABLE_ROWS) / HOST_ROWS;
    let (sections, ssh_rows) = ssh_hosts.split_at(ssh_hosts.len().min(usize::from(ssh_capacity)));
    let local = app
        .panels
        .iter()
        .find(|panel| crate::ssh::is_local(&panel.server));
    let (local_area, mut rest) = meters::split_y(body, local_rows);
    draw_host(f, local_area, "This Mac · Local", local, state, theme);
    for panel in sections {
        let (area, remaining) = meters::split_y(rest, HOST_ROWS);
        let server = &panel.server;
        let name = server.name.as_deref().unwrap_or(&server.host);
        draw_host(f, area, &format!("{name} · SSH"), Some(panel), state, theme);
        rest = remaining;
    }
    let (heading, table) = meters::split_y(rest, u16::from(expanded));
    let title = if ssh_rows.is_empty() {
        "Boat sandboxes"
    } else {
        "SSH hosts and Boat sandboxes"
    };
    f.render_widget(Paragraph::new(title).style(accent(theme)), heading);
    (table, ssh_rows)
}

fn accent(theme: &PaletteView) -> Style {
    Style::default()
        .fg(ink(theme.page.accent))
        .add_modifier(Modifier::BOLD)
}

fn draw_host(
    f: &mut Frame,
    area: Rect,
    title: &str,
    panel: Option<&Panel>,
    state: &State,
    theme: &PaletteView,
) {
    let (section, divider) = meters::split_y(area, area.height - u16::from(area.height > 1));
    let (heading, metrics) = meters::split_y(section, 1);
    f.render_widget(
        Paragraph::new(title).style(
            Style::default()
                .bg(ink(theme.bar.bg))
                .fg(ink(theme.bar.text))
                .add_modifier(Modifier::BOLD),
        ),
        heading,
    );
    f.render_widget(
        Block::new()
            .borders(Borders::TOP)
            .border_type(BorderType::Thick)
            .border_style(accent(theme)),
        divider,
    );
    if metrics.is_empty() {
        return;
    }
    let live = panel.and_then(|panel| state.fresh_sample(panel).map(|sample| (panel, sample)));
    let sample = live.map(|(_, sample)| sample);
    let cpu = cpu(sample);
    let status = if state.display.hide_memory {
        format!("CPU {cpu}")
    } else {
        let swap = sample.and_then(|snap| snap.swap.as_ref()).map_or_else(
            || MISSING.into(),
            |usage| format!("{}/{}", compact_size(usage.used), compact_size(usage.total)),
        );
        let mem = memory(sample, metrics.width >= FULL_MEMORY_WIDTH);
        format!("CPU {cpu}   Memory {mem}   Swap {swap}")
    };
    let status = Span::raw(status);
    let status_width = status.width();
    f.render_widget(
        Paragraph::new(Line::from(status)).style(Style::default().fg(ink(theme.page.text))),
        metrics,
    );
    if let Some((panel, sample)) = live {
        let graphs = [
            (
                CPU_LABEL,
                sample.cpu_pct.is_finite().then_some(&panel.history.cpu),
                theme.primary(),
            ),
            (
                MEM_LABEL,
                (sample.mem.total > 0 && sample.mem.pct.is_finite()).then_some(&panel.history.mem),
                theme.secondary(),
            ),
        ];
        draw_sparklines(
            f,
            metrics,
            status_width,
            &graphs[..2 - usize::from(state.display.hide_memory)],
            theme,
        );
    }
}

fn draw_sparklines(
    f: &mut Frame,
    area: Rect,
    status_width: usize,
    graphs: &[(&str, Option<&Series>, &str)],
    theme: &PaletteView,
) {
    let overhead = graphs
        .iter()
        .map(|(label, ..)| label.len() + GAP.len())
        .sum::<usize>();
    let available = usize::from(area.width).saturating_sub(status_width + overhead);
    let width = (available / graphs.len()).min(MAX_PLOT_WIDTH);
    if width < MIN_PLOT_WIDTH {
        return;
    }
    let mut spans = Vec::new();
    for &(label, series, color) in graphs {
        let bars: String = series
            .map_or_else(Vec::new, |series| series.tail(width))
            .into_iter()
            .map(|value| BAR_LEVELS[dots_for(value, FULL_PERCENT, BAR_LEVELS.len() - 1)])
            .collect();
        let plot = if bars.is_empty() { MISSING } else { &bars };
        spans.push(Span::styled(
            format!("{GAP}{label}"),
            Style::default().fg(ink(theme.page.muted)),
        ));
        spans.push(Span::styled(
            format!("{plot:>width$}"),
            meters::style(color),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans).right_aligned()), area);
}
