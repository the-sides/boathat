//! Boat tiles. Structured samples supply the visual layout; errors and other
//! views still pass through the existing notice/scrollback composer.

use crate::{app::App, panel::Mode};
use multitop_agent::{fmt::fmt_size, proto::Payload, render::Snapshot};
use ratatui::{layout::Rect, widgets::Paragraph, Frame};

use super::{meters, windowing::pane_lines};

const MIN_TILE_WIDTH: u16 = 38;
const MIN_TILE_HEIGHT: u16 = 20;

fn sample(app: &App, index: usize) -> Option<&Snapshot> {
    let panel = &app.panels[index];
    let fleet = app.boat.as_ref()?;
    let id = crate::boat::identity(&panel.server.host).unwrap_or(&panel.server.host);
    if crate::boat::offline(&panel.server.host)
        || fleet
            .samples
            .get(id)
            .is_none_or(|at| at.elapsed() > crate::boat::POLL_INTERVAL)
    {
        return None;
    }
    match panel.last_monitor.as_ref()? {
        Payload::Monitor(snapshot) => Some(snapshot),
        _ => None,
    }
}

/// `None` leaves ordinary SSH mode on its original renderer.
pub fn draw(f: &mut Frame, app: &App, index: usize, area: Rect) -> Option<usize> {
    app.boat.as_ref()?;
    let panel = &app.panels[index];
    let pal = app.current_theme();
    let snapshot = sample(app, index);
    let name = panel.server.name.as_deref().unwrap_or(&panel.server.host);
    let swap = snapshot.map(|snap| {
        snap.swap.map_or_else(
            || "SWAP unavailable".into(),
            |usage| format!("SWAP {}/{}", fmt_size(usage.used), fmt_size(usage.total)),
        )
    });
    let suffix = swap.map_or_else(
        || {
            if panel.last_monitor.is_some() && !crate::boat::offline(&panel.server.host) {
                " | no live sample".into()
            } else {
                String::new()
            }
        },
        |s| format!(" | {s}"),
    );
    let name = crate::layout::fit_banner(
        "",
        name,
        usize::from(area.width).saturating_sub(suffix.chars().count() + 4),
    );
    let title = format!(" {name}{suffix} ");
    let mut border = meters::block(title, &pal);
    if index == app.selected_panel {
        border = border.border_style(meters::style(pal.primary()));
    }
    let inner = border.inner(area);
    f.render_widget(border, area);
    if let Some(snap) = snapshot {
        if inner.width >= MIN_TILE_WIDTH
            && inner.height >= MIN_TILE_HEIGHT
            && panel.notes.is_empty()
        {
            match panel.mode {
                Mode::Monitor => {
                    return Some(super::fleet_stats::draw(f, panel, snap, inner, &pal));
                }
                Mode::Graphs => {
                    draw_graphs(f, panel, inner, &pal, app.graph_zoom);
                    return Some(0);
                }
                _ => {}
            }
        }
    }
    let (lines, offset) = pane_lines(
        app,
        index,
        usize::from(inner.height) + 1,
        usize::from(inner.width),
        panel.scroll_offset,
    );
    // The outer frame owns the title; pane_lines' placeholder row is omitted.
    let body: Vec<_> = lines.into_iter().skip(1).collect();
    f.render_widget(Paragraph::new(crate::ansi::to_text(&body)), inner);
    Some(offset)
}

fn draw_graphs(
    f: &mut Frame,
    panel: &crate::panel::Panel,
    area: Rect,
    pal: &multitop_agent::palette_view::PaletteView,
    zoom: u8,
) {
    let (cpu, mem) = meters::split_y(area, area.height.div_ceil(2));
    for (name, series, rect) in [
        ("CPU", &panel.history.cpu, cpu),
        ("MEM", &panel.history.mem, mem),
    ] {
        let latest = series.latest().unwrap_or_default();
        let border = meters::block(format!(" {name} {latest:.0}% "), pal);
        let plot = border.inner(rect);
        f.render_widget(border, rect);
        let samples = meters::samples(series, plot.width, zoom);
        meters::plot(f, plot, &samples, pal);
    }
}
