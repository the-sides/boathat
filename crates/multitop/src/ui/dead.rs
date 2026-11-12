//! Scalable ASCII lettering for an inactive boat's entire tile interior.

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

pub fn draw(f: &mut Frame, area: Rect, pal: &PaletteView) {
    let style = super::meters::style(&pal.red);
    let width = usize::from(area.width);
    let height = usize::from(area.height);
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
    // Stretch the bitmap on both axes so a large tile does not surround a
    // tiny label with empty space. Every painted character remains ASCII.
    let lines: Vec<_> = (0..height)
        .map(|y| {
            let source = WORD[y * WORD.len() / height].as_bytes();
            (0..width)
                .map(|x| char::from(source[x * source_width / width]))
                .collect::<String>()
        })
        .collect();
    f.render_widget(Paragraph::new(lines.join("\n")).style(style), area);
}
