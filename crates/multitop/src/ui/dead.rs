//! Centered, contained ASCII lettering for an inactive boat's tile.

use multitop_agent::palette_view::PaletteView;
use ratatui::{
    layout::{Alignment, Rect},
    widgets::Paragraph,
    Frame,
};

const WORD: [&str; 7] = [
    "####  #####  ###  #### ",
    "#   # #     #   # #   #",
    "#   # #     #   # #   #",
    "#   # ####  ##### #   #",
    "#   # #     #   # #   #",
    "#   # #     #   # #   #",
    "####  ##### #   # #### ",
];

// The proportions approved in the sizing comparison, in terminal cells.
// Character cells are taller than wide; preserving the raw 23x7 bitmap's
// cell ratio would make the letters much thinner than the approved example.
const DESIGN_WIDTH: u32 = 94;
const DESIGN_HEIGHT: u32 = 19;

fn contained(area: Rect) -> Rect {
    let (width, height) =
        if u32::from(area.width) * DESIGN_HEIGHT <= u32::from(area.height) * DESIGN_WIDTH {
            (
                area.width,
                u16::try_from(u32::from(area.width) * DESIGN_HEIGHT / DESIGN_WIDTH)
                    .unwrap_or(area.height),
            )
        } else {
            (
                u16::try_from(u32::from(area.height) * DESIGN_WIDTH / DESIGN_HEIGHT)
                    .unwrap_or(area.width),
                area.height,
            )
        };
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

pub fn draw(f: &mut Frame, area: Rect, pal: &PaletteView) {
    let style = super::meters::style(&pal.red);
    let lettering = contained(area);
    let width = usize::from(lettering.width);
    let height = usize::from(lettering.height);
    let source_width = WORD[0].len();
    if width < source_width || height < WORD.len() {
        let row = Rect {
            y: area.y + area.height.saturating_sub(1) / 2,
            height: area.height.min(1),
            ..area
        };
        f.render_widget(
            Paragraph::new("DEAD")
                .alignment(Alignment::Center)
                .style(style),
            row,
        );
        return;
    }
    // Rasterize within the fitted rectangle, leaving the spare space around
    // it empty. Every painted character remains ASCII.
    let lines: Vec<_> = (0..height)
        .map(|y| {
            let source = WORD[y * WORD.len() / height].as_bytes();
            (0..width)
                .map(|x| char::from(source[x * source_width / width]))
                .collect::<String>()
        })
        .collect();
    f.render_widget(Paragraph::new(lines.join("\n")).style(style), lettering);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitted_rectangles_match_the_approved_sizing_examples() {
        for (size, expected) in [
            ((94, 20), Rect::new(0, 0, 94, 19)),
            ((46, 20), Rect::new(0, 5, 46, 9)),
            ((142, 10), Rect::new(46, 0, 49, 10)),
            ((46, 30), Rect::new(0, 10, 46, 9)),
        ] {
            assert_eq!(contained(Rect::new(0, 0, size.0, size.1)), expected);
        }
    }
}
