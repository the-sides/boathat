//! btop 1.4.8's Matcha Dark Sea colors and terminal glyphs for fleet stats.
//! Palette reference: <https://github.com/aristocratos/btop/blob/v1.4.8/themes/matcha-dark-sea.theme>

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph},
    Frame,
};

pub(super) const INK: Color = Color::Rgb(248, 248, 242);
pub(super) const ACCENT: Color = Color::Rgb(46, 179, 152);
pub(super) const OUTLINE: Color = Color::Rgb(117, 113, 94);
pub(super) const DIVIDER: Color = Color::Rgb(89, 86, 71);
pub(super) const GREEN: Color = Color::Rgb(51, 177, 101);

const SCALE: i16 = 100;
pub(super) const CPU: [Color; 3] = [GREEN, INK, ACCENT];
pub(super) const USED: [Color; 3] = [OUTLINE, ACCENT, GREEN];
pub(super) const SWAP: [Color; 3] = [
    OUTLINE,
    Color::Rgb(169, 196, 116),
    Color::Rgb(226, 245, 188),
];
pub(super) const TEMP: [Color; 3] = [Color::Rgb(121, 118, 183), Color::Rgb(216, 184, 178), GREEN];

#[derive(Clone, Copy)]
pub(super) struct StatsStyle {
    color_enabled: bool,
    text: Color,
}

impl StatsStyle {
    pub(super) fn new(
        pal: &multitop_agent::palette_view::PaletteView,
        text: Option<multitop_agent::surface::Rgb>,
    ) -> Self {
        Self {
            color_enabled: pal.page.text.is_some(),
            text: text.map_or(INK, |rgb| super::ink(Some(rgb))),
        }
    }

    pub(super) const fn text_color(self) -> Color {
        self.text
    }

    pub(super) fn text(self) -> Style {
        self.fg(self.text)
    }

    pub(super) fn fg(self, color: Color) -> Style {
        Style::default().fg(if self.color_enabled {
            color
        } else {
            Color::Reset
        })
    }

    pub(super) fn frame(self, title: impl Into<String>, number: &str) -> Block<'static> {
        let frame = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(self.fg(OUTLINE));
        let title = title.into();
        if title.is_empty() {
            return frame;
        }
        let mut title_spans = vec![Span::styled("─┐", self.fg(OUTLINE))];
        if !number.is_empty() {
            title_spans.push(Span::styled(number.to_owned(), self.fg(ACCENT)));
        }
        title_spans.extend([
            Span::styled(title, self.text().add_modifier(Modifier::BOLD)),
            Span::styled("┌", self.fg(OUTLINE)),
        ]);
        frame.title(Line::from(title_spans))
    }

    pub(super) fn meter(self, pct: f64, width: u16, colors: [Color; 3]) -> Line<'static> {
        let scale = f64::from(SCALE);
        Line::from(
            (1..=width)
                .map(|i| {
                    let level = (scale * f64::from(i) / f64::from(width)).round();
                    let color = if pct.round() >= level {
                        gradient(colors, level / scale)
                    } else {
                        DIVIDER
                    };
                    Span::styled("■", self.fg(color))
                })
                .collect::<Vec<_>>(),
        )
    }

    pub(super) fn plot(self, f: &mut Frame, area: Rect, values: &[f64], colors: [Color; 3]) {
        let height = usize::from(area.height);
        let lines: Vec<_> =
            crate::graphs::braille_rows(values, f64::from(SCALE), usize::from(area.width), height)
                .into_iter()
                .enumerate()
                .map(|(index, row)| {
                    let level = multitop_agent::conv::count(height - index - 1)
                        / multitop_agent::conv::count(height.saturating_sub(1).max(1));
                    Line::styled(row, self.fg(gradient(colors, level)))
                })
                .collect();
        f.render_widget(Paragraph::new(lines), area);
    }
}

pub(super) fn blend(start: Color, end: Color, progress: f64) -> Color {
    let (Color::Rgb(sr, sg, sb), Color::Rgb(er, eg, eb)) = (start, end) else {
        return start;
    };
    let channel = |from, to| {
        let from = f64::from(from);
        let delta = f64::from(to) - from;
        u8::try_from(multitop_agent::conv::whole_usize(
            from + (delta * progress).trunc(),
        ))
        .unwrap_or(u8::MAX)
    };
    Color::Rgb(channel(sr, er), channel(sg, eg), channel(sb, eb))
}

fn gradient([start, middle, end]: [Color; 3], progress: f64) -> Color {
    let halfway = 0.5;
    if progress <= halfway {
        blend(start, middle, progress / halfway)
    } else {
        blend(middle, end, (progress - halfway) / halfway)
    }
}
