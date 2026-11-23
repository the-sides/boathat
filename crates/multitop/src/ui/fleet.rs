//! Boat tiles. Structured samples supply the visual layout; errors and other
//! views still pass through the existing notice/scrollback composer.

use crate::{app::App, boat, panel::Mode};
use multitop_agent::fmt::fmt_size;
use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Block, Paragraph, Wrap},
    Frame,
};

use super::{meters, windowing::pane_lines};

/// `None` leaves ordinary SSH mode on its original renderer.
pub fn draw(f: &mut Frame, app: &App, index: usize, area: Rect) -> Option<usize> {
    let panel = &app.panels[index];
    let pal = app.current_theme();
    let state = app.boat.as_ref()?;
    let snapshot = state.fresh_sample(panel);
    let offline = boat::offline(&panel.server.host);
    let graphs = panel.mode == Mode::Graphs && !offline;
    let sandbox = boat::identity(&panel.server.host)
        .filter(|_| graphs)
        .and_then(|id| state.inventory.iter().find(|sandbox| sandbox.id == id));
    let graph_title = sandbox.map(boat::Sandbox::title);
    let name = graph_title
        .as_deref()
        .or(panel.server.name.as_deref())
        .unwrap_or(&panel.server.host);
    let swap = snapshot.filter(|_| !state.display.hide_memory).map(|snap| {
        snap.swap.map_or_else(
            || "SWAP unavailable".into(),
            |usage| format!("SWAP {}/{}", fmt_size(usage.used), fmt_size(usage.total)),
        )
    });
    let suffix = if let Some(swap) = swap.as_deref().filter(|_| !graphs) {
        format!(" | {swap}")
    } else if snapshot.is_none() && panel.last_monitor.is_some() && !offline {
        " | no live sample".into()
    } else {
        String::new()
    };
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
    if offline {
        super::dead::draw(f, inner, &pal);
        return Some(0);
    }
    if graphs && !panel.history.is_empty() {
        let content = metric_area(f, panel, inner, &pal, snapshot.is_none());
        draw_graphs(
            f,
            panel,
            content,
            &pal,
            app.graph_zoom,
            state.display.hide_memory,
            [
                sandbox.map(|sandbox| super::boat_list::size_label(sandbox.size())),
                swap.as_deref(),
            ],
        );
        return Some(0);
    }
    if let Some(snap) = snapshot.filter(|_| panel.mode == Mode::Monitor) {
        let content = metric_area(f, panel, inner, &pal, false);
        return Some(super::fleet_stats::draw(
            f, panel, snap, content, &pal, state,
        ));
    }
    if matches!(panel.mode, Mode::Monitor | Mode::Graphs) {
        let status = panel
            .last_frame
            .as_ref()
            .filter(|lines| lines.len() <= 2)
            .or_else(|| (panel.view.len() <= 2).then_some(&panel.view));
        let mut lines = status.map_or_else(
            || vec!["Waiting for fresh metrics.".into()],
            |lines| lines.iter().skip(1).cloned().collect(),
        );
        lines.extend(panel.notes.iter().cloned());
        f.render_widget(
            Paragraph::new(crate::ansi::to_text(&lines)).wrap(Wrap { trim: false }),
            inner,
        );
        return Some(0);
    }
    let (lines, _) = pane_lines(
        app,
        index,
        usize::from(inner.height) + 1,
        usize::from(inner.width),
        0,
    );
    // The outer frame owns the title; pane_lines' placeholder row is omitted.
    let body: Vec<_> = lines.into_iter().skip(1).collect();
    f.render_widget(Paragraph::new(crate::ansi::to_text(&body)), inner);
    Some(0)
}

/// Notices take fixed space from the plots instead of turning the entire tile
/// back into a scrolling cached text frame.
fn metric_area(
    f: &mut Frame,
    panel: &crate::panel::Panel,
    area: Rect,
    pal: &multitop_agent::palette_view::PaletteView,
    stale: bool,
) -> Rect {
    let mut notes = panel.notes.clone();
    if stale {
        // Connection/status frames carry at most a body line plus row zero.
        // Longer last_frame values are old metric frames, not diagnostics.
        let errors = panel.last_frame.as_ref().filter(|lines| lines.len() <= 2);
        if let Some(errors) = errors {
            notes.extend(errors.iter().skip(1).cloned());
        } else {
            notes.push("Waiting for fresh metrics; showing previous history.".into());
        }
    }
    let lines: Vec<_> = notes
        .iter()
        .flat_map(|note| {
            crate::layout::wrap_words(
                &multitop_agent::color::strip_ansi(note),
                usize::from(area.width),
            )
        })
        .collect();
    let budget = (area.height / 3).max(1).min(area.height.saturating_sub(2));
    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX).min(budget);
    let (content, notice_area) = meters::split_y(area, area.height.saturating_sub(height));
    f.render_widget(
        Paragraph::new(lines.join("\n")).style(meters::style(pal.meter_mid())),
        notice_area,
    );
    content
}

fn draw_graphs(
    f: &mut Frame,
    panel: &crate::panel::Panel,
    area: Rect,
    pal: &multitop_agent::palette_view::PaletteView,
    zoom: u8,
    hide_memory: bool,
    [size, swap]: [Option<&str>; 2],
) {
    if area.height < 2 {
        let mut summary = format!("CPU {:.0}%", panel.history.cpu.latest().unwrap_or_default());
        if !hide_memory {
            use std::fmt::Write;
            let _ = write!(
                summary,
                "  MEM {:.0}%",
                panel.history.mem.latest().unwrap_or_default()
            );
        }
        f.render_widget(
            Paragraph::new(summary).style(meters::style(pal.primary())),
            area,
        );
        return;
    }
    let cpu_height = if hide_memory {
        area.height
    } else {
        area.height.div_ceil(2)
    };
    let (cpu, mem) = meters::split_y(area, cpu_height);
    for (name, series, rect, detail) in [
        ("CPU", &panel.history.cpu, cpu, size),
        ("MEM", &panel.history.mem, mem, swap),
    ] {
        if rect.height == 0 {
            continue;
        }
        let latest = series.latest().unwrap_or_default();
        let title = format!(" {name} {latest:.0}% ");
        let framed = rect.height >= 3;
        let detail = detail.map(|detail| Line::from(format!(" {detail} ")).right_aligned());
        let title = Line::from(title);
        let detail = detail.filter(|detail| {
            let available = usize::from(rect.width).saturating_sub(usize::from(framed) * 2);
            title.width() + detail.width() < available
        });
        let mut block = if framed {
            meters::block(title, pal)
        } else {
            Block::new()
                .title_top(title)
                .title_style(meters::style(pal.primary()))
        };
        if let Some(detail) = detail {
            block = block.title_top(detail);
        }
        let plot = block.inner(rect);
        f.render_widget(block, rect);
        let samples = meters::samples(series, plot.width, zoom);
        meters::plot(f, plot, &samples, pal);
    }
}
