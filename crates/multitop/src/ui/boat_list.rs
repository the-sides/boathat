use std::collections::BTreeMap;

use multitop_agent::{fmt::fmt_size, palette_view::PaletteView, render::Snapshot};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::{
    app::App, boat::Sandbox, boat_fleet::State, clock::now_epoch, layout::fit_row, panel::Panel,
};

use super::{ink, keybar::keybar_content};

mod hosts;

const HEADERS: [&str; 8] = [
    "State",
    "Time left",
    "CPU",
    "Memory",
    "Size",
    "Run time",
    "Compute $",
    "Name",
];
const FULL_WIDTHS: [u16; 8] = [8, 10, 5, 13, 4, 9, 9, 8];
const COMPACT_WIDTHS: [u16; 8] = [8, 9, 5, 6, 4, 9, 9, 8];
const GAP: u16 = 2;
const NAME: usize = 7;
const MEMORY: usize = 3;
const MISSING: &str = "—";

pub(super) fn draw(f: &mut Frame, app: &mut App) {
    let Some(state) = &app.boat else { return };
    let theme = app.current_theme();
    let keybar = keybar_content(app, &theme, f.area().width, crate::app::Mode::Graphs);
    let notice = state.discovery_notice.as_deref();
    let [body, status, footer] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(if notice.is_some() { 2 } else { 0 }),
        Constraint::Length(1),
    ])
    .areas(f.area());
    if let Some(notice) = notice {
        f.render_widget(
            Paragraph::new(notice)
                .style(Style::default().fg(ink(theme.page.warn)))
                .wrap(Wrap { trim: true }),
            status,
        );
    }
    f.render_widget(
        Paragraph::new(keybar).style(Style::default().bg(ink(theme.bar.bg))),
        footer,
    );
    let query = app.filter_query.trim().to_lowercase();
    let ssh_hosts: Vec<_> = app
        .panels
        .iter()
        .filter(|panel| {
            !crate::ssh::is_local(&panel.server)
                && crate::boat::identity(&panel.server.host).is_none()
                && panel.matches_filter(&query)
        })
        .collect();
    let (table, ssh_rows) = hosts::draw(f, body, app, state, &theme, &ssh_hosts);
    let scroll = draw_table(f, table, app, state, &theme, ssh_rows, &query);
    if let Some(state) = &mut app.boat {
        state.list_scroll = scroll;
    }
}

fn draw_table(
    f: &mut Frame,
    body: Rect,
    app: &App,
    state: &State,
    theme: &PaletteView,
    ssh_hosts: &[&Panel],
    query: &str,
) -> usize {
    let panels: BTreeMap<_, _> = app
        .panels
        .iter()
        .filter_map(|panel| crate::boat::identity(&panel.server.host).map(|id| (id, panel)))
        .collect();
    let now = now_epoch();
    let ListColumns {
        columns,
        widths,
        name_width,
        full,
    } = list_columns(usize::from(body.width), state.display.hide_memory);
    let mut rows = Vec::new();
    let mut add_rows = |name: &str, values: [String; NAME], style: Style| {
        for (line, name) in name_lines(name, name_width).into_iter().enumerate() {
            rows.push(
                Row::new(columns.iter().map(|&i| {
                    if i == NAME {
                        name.to_owned()
                    } else if line == 0 {
                        values[i].clone()
                    } else {
                        String::new()
                    }
                }))
                .style(style),
            );
        }
    };
    let normal = Style::default().fg(ink(theme.page.text));
    for panel in ssh_hosts {
        let server = &panel.server;
        let sample = state.fresh_sample(panel);
        add_rows(
            server.name.as_deref().unwrap_or(&server.host),
            [
                "SSH".into(),
                MISSING.into(),
                cpu(sample),
                memory(sample, full),
                MISSING.into(),
                MISSING.into(),
                MISSING.into(),
            ],
            normal,
        );
    }
    for boat in state.list_rows().filter(|boat| matches(boat, query)) {
        let sample = panels
            .get(boat.id.as_str())
            .and_then(|panel| state.fresh_sample(panel));
        let usage = state
            .usage
            .get(&boat.id)
            .filter(|_| state.discovery_notice.is_none());
        let values = [
            boat.state.clone(),
            time_left(boat, now),
            cpu(sample),
            memory(sample, full),
            size_label(boat.size()).into(),
            usage.map_or_else(|| MISSING.into(), |usage| run_time(usage.seconds)),
            usage.map_or_else(|| MISSING.into(), |usage| compute_cost(usage.dollars)),
        ];
        let style = if boat.is_dead() {
            Style::default().fg(ink(theme.page.muted))
        } else {
            normal
        };
        add_rows(boat.name(), values, style);
    }
    let scroll = state.list_scroll.min(
        rows.len()
            .saturating_sub(usize::from(body.height.saturating_sub(1))),
    );
    if rows.is_empty() {
        let message = if !state.discovered && state.discovery_notice.is_none() {
            "Discovering Boat sandboxes…"
        } else if !query.is_empty() {
            "No sandboxes match the filter."
        } else {
            "No boats to show."
        };
        f.render_widget(
            Paragraph::new(message).style(Style::default().fg(ink(theme.page.muted))),
            body,
        );
    } else {
        let widths = columns.iter().map(|&i| {
            if i == NAME {
                Constraint::Fill(1)
            } else {
                Constraint::Length(widths[i])
            }
        });
        let header = Row::new(columns.iter().map(|&i| HEADERS[i])).style(
            Style::default()
                .fg(ink(theme.page.accent))
                .add_modifier(Modifier::BOLD),
        );
        f.render_widget(
            Table::new(rows.into_iter().skip(scroll), widths)
                .header(header)
                .column_spacing(GAP),
            body,
        );
    }
    scroll
}

struct ListColumns {
    columns: Vec<usize>,
    widths: [u16; HEADERS.len()],
    name_width: usize,
    full: bool,
}

fn list_columns(body_width: usize, hide_memory: bool) -> ListColumns {
    let gap = usize::from(GAP);
    let full_size = FULL_WIDTHS.map(usize::from).into_iter().sum::<usize>() + gap * NAME;
    let hidden_width = if hide_memory {
        usize::from(FULL_WIDTHS[MEMORY]) + gap
    } else {
        0
    };
    let full = body_width >= full_size - hidden_width;
    let widths = if full { FULL_WIDTHS } else { COMPACT_WIDTHS };
    let mut fitting_widths = widths.map(usize::from);
    let mut fitting_budget = body_width;
    if hide_memory {
        // fit_row counts a separator even for a zero-width column.
        // Credit it back before fitting, then omit the empty column.
        fitting_widths[MEMORY] = 0;
        fitting_budget += gap;
    }
    let columns: Vec<_> = fit_row(
        &fitting_widths,
        gap,
        fitting_budget,
        if hide_memory {
            &[4, 1, 2, 0, 6, 5]
        } else {
            &[4, 1, MEMORY, 2, 0, 6, 5]
        },
    )
    .into_iter()
    .filter(|&index| !hide_memory || index != MEMORY)
    .collect();
    let name_width = body_width.saturating_sub(
        columns
            .iter()
            .filter(|&&i| i != NAME)
            .map(|&i| usize::from(widths[i] + GAP))
            .sum(),
    );
    ListColumns {
        columns,
        widths,
        name_width,
        full,
    }
}

fn matches(boat: &Sandbox, query: &str) -> bool {
    query.is_empty()
        || [boat.name(), &boat.id, &boat.state, boat.size()]
            .iter()
            .any(|s| s.to_lowercase().contains(query))
}

fn time_left(boat: &Sandbox, now: i64) -> String {
    let Some(end) = boat.lease_deadline().filter(|_| !boat.is_dead()) else {
        return MISSING.into();
    };
    let remaining = end.saturating_sub(now).max(0);
    match (remaining / 3600, remaining % 3600 / 60) {
        (0, 0) if remaining > 0 => "<1m".into(),
        (0, mins) => format!("{mins}m"),
        (hours, mins) => format!("{hours}h {mins:02}m"),
    }
}

fn run_time(seconds: u64) -> String {
    match (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60) {
        (0, 0, 0) if seconds > 0 => "<1m".into(),
        (0, 0, minutes) => format!("{minutes}m"),
        (0, hours, minutes) => format!("{hours}h {minutes:02}m"),
        (days, hours, _) => format!("{days}d {hours:02}h"),
    }
}

fn compute_cost(dollars: f64) -> String {
    if dollars > 0.0 && dollars < 0.01 {
        "<$0.01".into()
    } else {
        format!("${dollars:.2}")
    }
}

pub(super) fn size_label(size: &str) -> &'static str {
    match size {
        "small" => "S",
        "default" => "D",
        "large" => "L",
        "xlarge" => "XL",
        _ => MISSING,
    }
}

fn compact_size(bytes: u64) -> String {
    let mut size = fmt_size(bytes);
    if size.ends_with("iB") {
        size.truncate(size.len() - 2);
    }
    size
}

fn cpu(sample: Option<&Snapshot>) -> String {
    sample
        .filter(|snap| snap.cpu_pct.is_finite())
        .map_or_else(|| MISSING.into(), |snap| format!("{:.0}%", snap.cpu_pct))
}

fn memory(sample: Option<&Snapshot>, full: bool) -> String {
    sample
        .filter(|snap| snap.mem.total > 0 && snap.mem.pct.is_finite())
        .map_or_else(
            || MISSING.into(),
            |snap| {
                if full {
                    format!(
                        "{}/{}",
                        compact_size(snap.mem.used),
                        compact_size(snap.mem.total)
                    )
                } else {
                    format!("{:.0}%", snap.mem.pct)
                }
            },
        )
}

/// Wrap names at word boundaries, including branch names longer than a column.
fn name_lines(mut name: &str, width: usize) -> Vec<&str> {
    let mut lines = Vec::new();
    while !name.is_empty() && width > 0 {
        if Span::raw(name).width() <= width {
            lines.push(name);
            break;
        }
        let end = name
            .char_indices()
            .skip(1)
            .take_while(|&(i, _)| Span::raw(&name[..i]).width() <= width)
            .last()
            .map_or_else(|| name.chars().next().map_or(0, char::len_utf8), |(i, _)| i);
        let split = if name[end..].starts_with(char::is_whitespace) {
            end
        } else {
            name[..end]
                .rfind(char::is_whitespace)
                .filter(|&i| i > 0)
                .unwrap_or(end)
        };
        lines.push(name[..split].trim_end());
        name = name[split..].trim_start();
    }
    lines
}
