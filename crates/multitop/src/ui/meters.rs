//! Shared btop-style frames, shaded meters and height-tinted history plots.

use multitop_agent::{palette_view::PaletteView, surface::Rgb};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph},
    Frame,
};

const PERCENT: f64 = 100.0;

pub fn samples(series: &crate::history::Series, width: u16, zoom: u8) -> Vec<f64> {
    let stride = usize::from(zoom.max(1));
    series
        .tail(usize::from(width) * 2 * stride)
        .chunks(stride)
        .map(|chunk| chunk.iter().copied().fold(0.0, f64::max))
        .collect()
}

pub fn style(ansi: &str) -> Style {
    Style::default().fg(super::ink(Rgb::from_ansi(ansi)))
}

pub fn block(title: impl Into<Line<'static>>, pal: &PaletteView) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(super::ink(pal.page.border)))
        .title(title)
        .title_style(style(pal.primary()).add_modifier(Modifier::BOLD))
}

pub fn meter(pct: f64, width: u16, pal: &PaletteView) -> Line<'static> {
    let cells = usize::from(width);
    let filled = multitop_agent::conv::whole_usize(
        (pct.clamp(0.0, PERCENT) / PERCENT * f64::from(width)).round(),
    );
    Line::from(
        (0..cells)
            .map(|i| {
                if i < filled {
                    Span::styled("▮", style(crate::graphs::height_ink(pal, i, cells)))
                } else {
                    Span::styled("░", style(pal.muted()))
                }
            })
            .collect::<Vec<_>>(),
    )
}

pub fn plot(f: &mut Frame, area: Rect, samples: &[f64], pal: &PaletteView) {
    if samples.is_empty() {
        f.render_widget(
            Paragraph::new("Waiting for samples").style(style(pal.muted())),
            area,
        );
        return;
    }
    let rows = usize::from(area.height);
    let lines: Vec<_> =
        crate::graphs::braille_rows(samples, PERCENT, usize::from(area.width), rows)
            .into_iter()
            .enumerate()
            .map(|(i, row)| {
                Line::styled(
                    row,
                    style(crate::graphs::height_ink(pal, rows - 1 - i, rows)),
                )
            })
            .collect();
    f.render_widget(Paragraph::new(lines), area);
}

pub fn split_x(area: Rect, left_width: u16) -> (Rect, Rect) {
    let width = left_width.min(area.width);
    (
        Rect { width, ..area },
        Rect {
            x: area.x + width,
            width: area.width - width,
            ..area
        },
    )
}

pub fn split_y(area: Rect, top_height: u16) -> (Rect, Rect) {
    let height = top_height.min(area.height);
    (
        Rect { height, ..area },
        Rect {
            y: area.y + height,
            height: area.height - height,
            ..area
        },
    )
}
