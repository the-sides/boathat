//! Full fleet stats. The btop layout uses a full-width CPU box above a
//! 45/55 memory/process split, independently inside each host tile.

use crate::{boat_fleet::StatsPanel, history::Series, panel::Panel};
use multitop_agent::{
    fmt::{fmt_rate, fmt_size},
    palette_view::PaletteView,
    proc::Usage,
    render::{Snapshot, TempUnit},
};
use ratatui::{
    layout::{Alignment, Constraint, Rect},
    style::{Color, Modifier},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use super::fleet_stats_style::{blend, StatsStyle, ACCENT, CPU, DIVIDER, GREEN, SWAP, TEMP, USED};
use super::layout::FULL_METRIC_WIDTH;
use super::meters::{samples, split_x, split_y};

const MIN_DASHBOARD_HEIGHT: u16 = 12;
const CPU_MIN_HEIGHT: u16 = 8;
const CPU_PERCENT_HEIGHT: u32 = 32;
const MEMORY_PERCENT_WIDTH: u32 = 45;
const PERCENT: u32 = 100;
const CORE_MIN_WIDTH: u16 = 15;
const CORE_WIDE_WIDTH: u16 = 33;
const CORE_MAX_COLUMNS: u16 = 4;
const METER_MIN_WIDTH: u16 = 29;
const MHZ_PER_GHZ: f64 = 1000.0;
const PID_WIDTH: u16 = 8;
const MEM_WIDTH: u16 = 9;
const CPU_PERCENT_WIDTH: u16 = 6;

pub fn draw(
    f: &mut Frame,
    panel: &Panel,
    snap: &Snapshot,
    area: Rect,
    pal: &PaletteView,
    state: &crate::boat_fleet::State,
) -> usize {
    let display = &state.display;
    let style = StatsStyle::new(pal, display.stats_text);
    let sidebar = !display.hide_memory
        || !display.hidden(StatsPanel::Network)
        || !display.hidden(StatsPanel::Disks);
    if !sidebar && display.hidden(StatsPanel::Cpu) && display.hidden(StatsPanel::Processes) {
        f.render_widget(
            Paragraph::new("All panels hidden · 1 CPU  2 MEM  3 NET  4 PROC  d DISKS")
                .style(style.text()),
            area,
        );
        return 0;
    }
    if area.height < MIN_DASHBOARD_HEIGHT || area.width < FULL_METRIC_WIDTH {
        return super::fleet_compact::draw(f, panel, snap, area, style, display);
    }
    let lower = if display.hidden(StatsPanel::Cpu) {
        area
    } else {
        let height = if !sidebar && display.hidden(StatsPanel::Processes) {
            area.height
        } else {
            let preferred = (u32::from(area.height) * CPU_PERCENT_HEIGHT).div_ceil(PERCENT);
            u16::try_from(preferred)
                .unwrap_or(area.height)
                .max(CPU_MIN_HEIGHT.min(area.height / 2))
        };
        let (cpu, lower) = split_y(area, height);
        draw_cpu(f, panel, snap, cpu, style);
        lower
    };
    let side_width = match (sidebar, display.hidden(StatsPanel::Processes)) {
        (false, _) => 0,
        (true, true) => lower.width,
        (true, false) => {
            u16::try_from((u32::from(lower.width) * MEMORY_PERCENT_WIDTH + PERCENT / 2) / PERCENT)
                .unwrap_or(0)
        }
    };
    let (side, processes) = split_x(lower, side_width);
    draw_sidebar(f, panel, snap, side, style, display);
    if display.hidden(StatsPanel::Processes) {
        0
    } else {
        draw_processes(f, panel, snap, processes, style)
    }
}

fn draw_sidebar(
    f: &mut Frame,
    panel: &Panel,
    snap: &Snapshot,
    area: Rect,
    style: StatsStyle,
    display: &crate::boat_fleet::Display,
) {
    let show_memory = !display.hide_memory;
    let show_network = !display.hidden(StatsPanel::Network);
    let show_disks = !display.hidden(StatsPanel::Disks);
    let disks_height = if !show_disks {
        0
    } else if !show_memory && !show_network {
        area.height
    } else if area.height >= 12 {
        3
    } else {
        1
    };
    let (upper, disks) = split_y(area, area.height - disks_height);
    let memory_height = match (show_memory, show_network) {
        (false, _) => 0,
        (true, false) => upper.height,
        (true, true) => upper.height - (upper.height / 3).max(1).min(upper.height),
    };
    let (memory, network) = split_y(upper, memory_height);
    if show_memory {
        draw_memory(f, panel, snap, memory, style);
    }
    if show_network {
        draw_network(f, panel, snap, network, style);
    }
    if show_disks {
        let value = format!(
            "Root {} / {} · {:.0}%",
            fmt_size(snap.disk.used),
            fmt_size(snap.disk.total),
            snap.disk.pct
        );
        let body = summary_box(f, disks, "disks", "", &value, style);
        let (_, meter) = split_y(body, 1);
        if meter.height > 0 {
            f.render_widget(
                Paragraph::new(style.meter(snap.disk.pct, meter.width, USED)),
                meter,
            );
        }
    }
}

fn summary_box(
    f: &mut Frame,
    area: Rect,
    name: &str,
    number: &str,
    value: &str,
    style: StatsStyle,
) -> Rect {
    if area.height < 3 {
        f.render_widget(
            Paragraph::new(format!("{number}{name}  {value}")).style(style.text()),
            area,
        );
        return Rect::default();
    }
    let border = style.frame(name, number);
    let inner = border.inner(area);
    f.render_widget(border, area);
    let (summary, _) = split_y(inner, 1);
    f.render_widget(Paragraph::new(value).style(style.text()), summary);
    inner
}

fn draw_network(f: &mut Frame, panel: &Panel, snap: &Snapshot, area: Rect, style: StatsStyle) {
    let rates = format!("↑ {} ↓ {}", fmt_rate(snap.tx_rate), fmt_rate(snap.rx_rate));
    let body = summary_box(f, area, "net", "³", &rates, style);
    let (_, graph) = split_y(body, 1);
    let (upload, download) = split_y(graph, graph.height.div_ceil(2));
    for (area, series, colors) in [
        (upload, &panel.history.tx, USED),
        (download, &panel.history.rx, CPU),
    ] {
        let mut values = samples(series, area.width, 1);
        let peak = values.iter().copied().fold(1.0, f64::max);
        for value in &mut values {
            *value = *value / peak * f64::from(PERCENT);
        }
        style.plot(f, area, &values, colors);
    }
}

fn draw_cpu(f: &mut Frame, panel: &Panel, snap: &Snapshot, area: Rect, style: StatsStyle) {
    let border = style.frame("cpu", "¹");
    let inner = border.inner(area);
    f.render_widget(border, area);
    let core_count = u16::try_from(snap.cores.len()).unwrap_or(u16::MAX);
    let available_rows = inner.height.saturating_sub(3).max(1);
    let columns = core_count
        .div_ceil(available_rows)
        .clamp(1, CORE_MAX_COLUMNS)
        .min((inner.width / 2 / CORE_MIN_WIDTH).max(1));
    let rows = core_count.div_ceil(columns).min(available_rows);
    let detail_width = (columns * CORE_WIDE_WIDTH).clamp(METER_MIN_WIDTH, inner.width / 2);
    let (graph, detail) = split_x(inner, inner.width - detail_width);
    style.plot(f, graph, &samples(&panel.history.cpu, graph.width, 1), CPU);
    let detail_height = rows + 3;
    let detail_area = Rect {
        y: detail.y + (detail.height - detail_height).div_ceil(2),
        height: detail_height,
        ..detail
    };
    let clock = snap
        .cpu_mhz
        .map_or_else(String::new, |mhz| format!("{:.2} GHz", mhz / MHZ_PER_GHZ));
    let border = style.frame(clock, "");
    let body = border.inner(detail_area);
    f.render_widget(border, detail_area);
    let (total, cores) = split_y(body, 1);
    let (label, total) = split_x(total, 4);
    let (bar, value) = split_x(total, total.width.saturating_sub(CPU_PERCENT_WIDTH));
    f.render_widget(
        Paragraph::new("CPU ").style(style.text().add_modifier(Modifier::BOLD)),
        label,
    );
    f.render_widget(
        Paragraph::new(style.meter(snap.cpu_pct, bar.width, CPU)),
        bar,
    );
    f.render_widget(
        Paragraph::new(format!("{:>5.0}%", snap.cpu_pct)).style(style.fg(ACCENT)),
        value,
    );

    let unit = match snap.temp_unit {
        TempUnit::C => "C",
        TempUnit::F => "F",
    };
    let width = cores.width / columns;
    for (column, chunk) in snap
        .cores
        .chunks(usize::from(rows.max(1)))
        .take(usize::from(columns))
        .enumerate()
    {
        let x = cores.x + u16::try_from(column).unwrap_or(0) * width;
        for (y, &(id, load, temp)) in (cores.y..cores.bottom()).zip(chunk) {
            let cell = Rect::new(x, y, width, 1);
            let temp = temp
                .filter(|_| width >= CORE_MIN_WIDTH)
                .map_or_else(String::new, |value| format!(" {value:>3.0}°{unit}"));
            let values = format!("{load:>4.0}%{temp}");
            let value_width = u16::try_from(values.chars().count()).unwrap_or(width);
            let (label, values_area) = split_x(cell, width.saturating_sub(value_width + 1));
            f.render_widget(Paragraph::new(format!("C{id}")).style(style.text()), label);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!("{load:>4.0}%"), style.fg(ACCENT)),
                    Span::styled(temp, style.fg(TEMP[1])),
                ])),
                values_area,
            );
        }
    }
}

fn draw_memory(f: &mut Frame, panel: &Panel, snap: &Snapshot, area: Rect, style: StatsStyle) {
    let border = style.frame("mem", "²");
    let inner = border.inner(area);
    f.render_widget(border, area);
    let (total, charts) = split_y(inner, 1);
    reading(f, total, "Total:", &fmt_size(snap.mem.total), style, false);
    let (used, swap_area) = split_y(charts, charts.height.div_ceil(2));
    memory_chart(
        f,
        used,
        ("Used:", Some(snap.mem)),
        &panel.history.mem,
        USED,
        style,
    );
    memory_chart(
        f,
        swap_area,
        ("Swap:", snap.swap),
        &panel.history.swap,
        SWAP,
        style,
    );
}

fn reading(f: &mut Frame, area: Rect, label: &str, value: &str, style: StatsStyle, ruled: bool) {
    if ruled {
        f.render_widget(
            Paragraph::new("─".repeat(usize::from(area.width))).style(style.fg(DIVIDER)),
            area,
        );
    }
    let (label_area, value_area) = split_x(area, u16::try_from(label.len() + 1).unwrap_or(0));
    f.render_widget(
        Paragraph::new(format!(" {label}")).style(style.text()),
        label_area,
    );
    f.render_widget(
        Paragraph::new(format!("{value} "))
            .alignment(Alignment::Right)
            .style(style.text()),
        value_area,
    );
}

fn memory_chart(
    f: &mut Frame,
    area: Rect,
    (label, usage): (&str, Option<Usage>),
    series: &Series,
    colors: [Color; 3],
    style: StatsStyle,
) {
    let (heading, plot) = split_y(area, 1);
    let value = match usage {
        Some(usage) if label == "Swap:" => {
            format!("{} / {}", fmt_size(usage.used), fmt_size(usage.total))
        }
        Some(usage) => fmt_size(usage.used),
        None => "unavailable".into(),
    };
    reading(f, heading, label, &value, style, true);
    if let Some(usage) = usage {
        style.plot(f, plot, &samples(series, plot.width, 1), colors);
        let (percent, _) = split_y(plot, 1);
        f.render_widget(
            Paragraph::new(format!(" {:>3.0}%", usage.pct)).style(style.text()),
            Rect {
                width: percent.width.min(CPU_PERCENT_WIDTH),
                ..percent
            },
        );
    }
}

pub(super) fn draw_processes(
    f: &mut Frame,
    panel: &Panel,
    snap: &Snapshot,
    area: Rect,
    style: StatsStyle,
) -> usize {
    if area.height < 3 {
        return 0;
    }
    let border = style.frame("proc", "⁴");
    let inner = border.inner(area);
    f.render_widget(border, area);
    let rows = usize::from(inner.height.saturating_sub(1));
    let offset = panel
        .scroll_offset
        .min(snap.procs.len().saturating_sub(rows));
    let header = Row::new(["    Pid:", "Program:", "     MemB", "  Cpu%"])
        .style(style.text().add_modifier(Modifier::BOLD));
    let entries = snap
        .procs
        .iter()
        .skip(offset)
        .take(rows)
        .enumerate()
        .map(|(i, proc)| {
            let fade = multitop_agent::conv::count(i) / multitop_agent::conv::count(rows.max(1));
            let text = style.fg(blend(style.text_color(), DIVIDER, fade));
            let value = style.fg(blend(GREEN, DIVIDER, fade));
            let name: String = proc
                .name
                .as_str()
                .chars()
                .filter(|c| !c.is_control())
                .collect();
            Row::new([
                Cell::from(format!("{:>8}", proc.pid)).style(text),
                Cell::from(name).style(value),
                Cell::from(format!("{:>9}", fmt_size(proc.mem))).style(value),
                Cell::from(format!("{:>6.1}", proc.cpu)).style(value),
            ])
        });
    let table = Table::new(
        entries,
        [
            Constraint::Length(PID_WIDTH),
            Constraint::Min(1),
            Constraint::Length(MEM_WIDTH),
            Constraint::Length(CPU_PERCENT_WIDTH),
        ],
    )
    .header(header);
    f.render_widget(table, inner);
    offset
}
