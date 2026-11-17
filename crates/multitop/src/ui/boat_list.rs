use std::collections::BTreeMap;

use multitop_agent::{fmt::fmt_size, palette_view::PaletteView, render::Snapshot};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::{app::App, boat::Sandbox, boat_fleet::State, clock::now_epoch, layout::fit_row};

use super::{ink, keybar::keybar_content};

const HEADERS: [&str; 6] = ["State", "Time left", "CPU", "Memory", "Size", "Name"];
const FULL_WIDTHS: [u16; 6] = [8, 10, 5, 18, 7, 18];
const COMPACT_WIDTHS: [u16; 6] = [8, 9, 5, 6, 7, 8];
const GAP: u16 = 2;
const NAME: usize = 5;
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
    let scroll = draw_table(f, body, app, state, &theme);
    if let Some(state) = &mut app.boat {
        state.list_scroll = scroll;
    }
}

fn draw_table(f: &mut Frame, body: Rect, app: &App, state: &State, theme: &PaletteView) -> usize {
    let panels: BTreeMap<_, _> = app
        .panels
        .iter()
        .filter_map(|panel| crate::boat::identity(&panel.server.host).map(|id| (id, panel)))
        .collect();
    let query = app.filter_query.trim().to_lowercase();
    let now = now_epoch();
    let body_width = usize::from(body.width);
    let gap = usize::from(GAP);
    let full = body_width >= FULL_WIDTHS.map(usize::from).into_iter().sum::<usize>() + gap * NAME;
    let widths = if full { FULL_WIDTHS } else { COMPACT_WIDTHS };
    let columns = fit_row(&widths.map(usize::from), gap, body_width, &[4, 1, 0, 3, 2]);
    let name_width = body_width.saturating_sub(
        columns
            .iter()
            .filter(|&&i| i != NAME)
            .map(|&i| usize::from(widths[i] + GAP))
            .sum(),
    );
    let mut rows = Vec::new();
    for boat in state.list_rows().filter(|boat| matches(boat, &query)) {
        let sample = panels
            .get(boat.id.as_str())
            .and_then(|panel| state.fresh_sample(panel));
        let names = name_lines(boat.name(), name_width);
        let Some((name, continuation)) = names.split_first() else {
            continue;
        };
        let values = [
            boat.state.clone(),
            time_left(boat, now),
            cpu(sample),
            memory(sample, full),
            boat.size().into(),
            (*name).into(),
        ];
        let color = if boat.is_dead() {
            theme.page.muted
        } else {
            theme.page.text
        };
        let style = Style::default().fg(ink(color));
        rows.push(Row::new(columns.iter().map(|&i| values[i].clone())).style(style));
        rows.extend(continuation.iter().map(|name| {
            Row::new(columns.iter().map(|&i| if i == NAME { *name } else { "" })).style(style)
        }));
    }
    let scroll = state.list_scroll.min(
        rows.len()
            .saturating_sub(usize::from(body.height.saturating_sub(1))),
    );
    if rows.is_empty() {
        let message = if !state.discovered && state.discovery_notice.is_none() {
            "Discovering Boat sandboxes…"
        } else if !query.is_empty() {
            "No boats match the filter."
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
                    format!("{}/{}", fmt_size(snap.mem.used), fmt_size(snap.mem.total))
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
