//! Compact machine dashboard, assembled from measurements already on the wire.

use crate::panel::Panel;
use multitop_agent::{
    fmt::{fmt_rate, fmt_size},
    palette_view::PaletteView,
    proc::Usage,
    render::Snapshot,
};
use ratatui::{
    layout::{Alignment, Constraint, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use super::meters::{self, block, meter, plot, split_x, split_y, style};

const CPU_MIN_HEIGHT: u16 = 6;
const CPU_MAX_HEIGHT: u16 = 12;
const RESOURCE_HEIGHT: u16 = 8;
const CPU_DETAIL_MIN_WIDTH: u16 = 62;
const TWO_CORE_COLUMNS_WIDTH: u16 = 36;
const CORE_LABEL_WIDTH: u16 = 12;
const CPU_PERCENT_WIDTH: u16 = 6;
const WIDE_TABLE: u16 = 110;
const PID_WIDTH: u16 = 7;
const MEM_WIDTH: u16 = 10;

pub fn draw(f: &mut Frame, panel: &Panel, snap: &Snapshot, area: Rect, pal: &PaletteView) -> usize {
    let cpu_height = (area.height / 3).clamp(CPU_MIN_HEIGHT, CPU_MAX_HEIGHT);
    let (cpu, rest) = split_y(area, cpu_height);
    draw_cpu(f, panel, snap, cpu, pal);
    // Leave a useful process table on short panes instead of clipping it away.
    let resources_height = RESOURCE_HEIGHT.min(rest.height / 2);
    let (resources, processes) = split_y(rest, resources_height);
    let (mem, disk) = split_x(resources, resources.width / 2);
    draw_memory(f, panel, snap.mem, mem, pal);
    draw_disk(f, snap, disk, pal);
    draw_processes(f, panel, snap, processes, pal)
}

fn draw_cpu(f: &mut Frame, panel: &Panel, snap: &Snapshot, area: Rect, pal: &PaletteView) {
    let clock = snap
        .cpu_mhz
        .map_or_else(String::new, |mhz| format!(" | {:.2} GHz", mhz / 1000.0));
    let border = block(format!(" CPU {:.0}%{clock} ", snap.cpu_pct), pal);
    let inner = border.inner(area);
    f.render_widget(border, area);
    if inner.width < CPU_DETAIL_MIN_WIDTH {
        plot(
            f,
            inner,
            &meters::samples(&panel.history.cpu, inner.width, 1),
            pal,
        );
        return;
    }
    let (history, detail) = split_x(inner, inner.width / 2);
    plot(
        f,
        history,
        &meters::samples(&panel.history.cpu, history.width, 1),
        pal,
    );
    let (total, cores) = split_y(detail, 1);
    let (bar, value) = split_x(total, total.width.saturating_sub(CPU_PERCENT_WIDTH));
    f.render_widget(Paragraph::new(meter(snap.cpu_pct, bar.width, pal)), bar);
    f.render_widget(
        Paragraph::new(format!("{:>4.0}%", snap.cpu_pct)).style(style(pal.text())),
        value,
    );
    let columns = if cores.width >= TWO_CORE_COLUMNS_WIDTH {
        2
    } else {
        1
    };
    let rows = snap.cores.len().div_ceil(columns);
    for (i, &(id, pct, _)) in snap.cores.iter().enumerate() {
        let row = i % rows.max(1);
        if row >= usize::from(cores.height) {
            continue;
        }
        let width = cores.width / u16::try_from(columns).unwrap_or(1);
        let cell = Rect {
            x: cores.x + u16::try_from(i / rows.max(1)).unwrap_or(0) * width,
            y: cores.y + u16::try_from(row).unwrap_or(0),
            width,
            height: 1,
        };
        let label = format!(" C{id:<2} ");
        let mut spans = vec![Span::styled(label, style(pal.primary()))];
        spans.extend(meter(pct, width.saturating_sub(CORE_LABEL_WIDTH), pal).spans);
        spans.push(Span::styled(format!(" {pct:>3.0}%"), style(pal.text())));
        f.render_widget(Paragraph::new(Line::from(spans)), cell);
    }
}

fn draw_memory(f: &mut Frame, panel: &Panel, usage: Usage, area: Rect, pal: &PaletteView) {
    let border = block(format!(" MEM {:.0}% ", usage.pct), pal);
    let inner = border.inner(area);
    f.render_widget(border, area);
    let (labels, graph) = split_y(inner, 2);
    let lines = vec![
        Line::from(vec![
            Span::styled("Used  ", style(pal.primary())),
            Span::styled(
                format!("{} / {}", fmt_size(usage.used), fmt_size(usage.total)),
                style(pal.text()),
            ),
        ]),
        Line::styled(
            format!("Free  {}", fmt_size(usage.total.saturating_sub(usage.used))),
            style(pal.muted()),
        ),
    ];
    f.render_widget(Paragraph::new(lines), labels);
    plot(
        f,
        graph,
        &meters::samples(&panel.history.mem, graph.width, 1),
        pal,
    );
}

fn draw_disk(f: &mut Frame, snap: &Snapshot, area: Rect, pal: &PaletteView) {
    let border = block(" disks / swap ", pal);
    let inner = border.inner(area);
    f.render_widget(border, area);
    let (disk, rest) = split_y(inner, 2);
    draw_usage(f, "Root", snap.disk, disk, pal);
    let (swap_area, net_area) = split_y(rest, 2);
    if let Some(usage) = snap.swap {
        draw_usage(f, "Swap", usage, swap_area, pal);
    } else {
        f.render_widget(
            Paragraph::new("Swap unavailable").style(style(pal.muted())),
            swap_area,
        );
    }
    let net = Paragraph::new(format!(
        "NET ↑ {} ↓ {}",
        fmt_rate(snap.tx_rate),
        fmt_rate(snap.rx_rate)
    ))
    .alignment(Alignment::Right)
    .style(style(pal.primary()));
    f.render_widget(net, net_area);
}

fn draw_usage(f: &mut Frame, name: &str, usage: Usage, area: Rect, pal: &PaletteView) {
    let (label, bar) = split_y(area, 1);
    f.render_widget(
        Paragraph::new(format!(
            "{name}  {} / {}",
            fmt_size(usage.used),
            fmt_size(usage.total)
        ))
        .style(style(pal.text())),
        label,
    );
    let (meter_area, value) = split_x(bar, bar.width.saturating_sub(CPU_PERCENT_WIDTH));
    f.render_widget(
        Paragraph::new(meter(usage.pct, meter_area.width, pal)),
        meter_area,
    );
    f.render_widget(
        Paragraph::new(format!("{:>4.0}%", usage.pct)).style(style(pal.text())),
        value,
    );
}

fn draw_processes(
    f: &mut Frame,
    panel: &Panel,
    snap: &Snapshot,
    area: Rect,
    pal: &PaletteView,
) -> usize {
    let rows = usize::from(area.height.saturating_sub(3));
    let columns = if area.width >= WIDE_TABLE { 2 } else { 1 };
    let offset = panel
        .scroll_offset
        .min(snap.procs.len().saturating_sub(rows * columns));
    let scroll = if offset == 0 {
        String::new()
    } else {
        format!(" | scroll {offset}")
    };
    let border = block(format!(" processes{scroll} "), pal);
    let inner = border.inner(area);
    f.render_widget(border, area);
    let halves: [Rect; 2] =
        split_x(inner, inner.width / u16::try_from(columns).unwrap_or(1)).into();
    for (column, rect) in halves.into_iter().take(columns).enumerate() {
        let header = Row::new(["PID", "NAME", "CPU%", "MEM"])
            .style(style(pal.primary()).add_modifier(Modifier::BOLD));
        let procs = snap
            .procs
            .iter()
            .skip(offset + column * rows)
            .take(rows)
            .map(|proc| {
                Row::new(vec![
                    Cell::from(proc.pid.to_string()).style(style(pal.muted())),
                    // Process names are remote text too. Never hand terminal
                    // controls to the widget/backend as printable content.
                    Cell::from(
                        proc.name
                            .as_str()
                            .chars()
                            .filter(|c| !c.is_control())
                            .collect::<String>(),
                    ),
                    Cell::from(format!("{:.1}", proc.cpu)).style(style(pal.cpu_bar(proc.cpu))),
                    Cell::from(fmt_size(proc.mem)).style(style(pal.secondary())),
                ])
            });
        let table = Table::new(
            procs,
            [
                Constraint::Length(PID_WIDTH),
                Constraint::Min(1),
                Constraint::Length(CPU_PERCENT_WIDTH),
                Constraint::Length(MEM_WIDTH),
            ],
        )
        .header(header)
        .column_spacing(1)
        .style(style(pal.text()));
        f.render_widget(table, rect);
    }
    offset
}
