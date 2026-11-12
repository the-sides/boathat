use multitop::ui::{agent_dims, regions};
use ratatui::layout::{Rect, Size};

fn columns(width: u16, height: u16, count: usize) -> usize {
    let (tiles, _) = regions(Rect::new(0, 0, width, height), count);
    tiles.iter().filter(|tile| tile.y == 0).count()
}

#[test]
fn wide_ten_machine_fleet_uses_four_or_five_columns_for_full_tiles() {
    for (width, expected) in [(400, 4), (600, 5)] {
        let (tiles, _) = regions(Rect::new(0, 0, width, 80), 10);
        assert_eq!(columns(width, 80, 10), expected);
        assert!(tiles
            .iter()
            .all(|tile| tile.width >= 64 && tile.height >= 22));
    }
}

#[test]
fn two_machines_use_the_width_when_stacking_would_force_compact_metrics() {
    assert_eq!(columns(200, 40, 2), 2);
    assert_eq!(columns(100, 44, 2), 1);
}

#[test]
fn resizing_reflows_without_gaps_overlap_or_leaving_agents_at_old_sizes() {
    for (width, height) in [(100, 44), (200, 80), (400, 80), (600, 80), (320, 62)] {
        for count in [1, 2, 3, 4, 7, 10, 16] {
            let area = Rect::new(7, 5, width, height);
            let (tiles, keybar) = regions(area, count);
            let mut covered = std::collections::HashSet::new();
            let dims = agent_dims(Size::new(width, height), count);
            let shown: Vec<_> = (0..count).map(|i| i * 3 + 1).collect();
            for (index, tile) in tiles.into_iter().enumerate() {
                assert!(tile.x >= area.x && tile.right() <= area.right());
                assert!(tile.y >= area.y && tile.bottom() <= keybar.y);
                assert!(dims.0 <= tile.width.saturating_sub(2));
                assert!(dims.1 <= tile.height);
                assert_eq!(
                    multitop::run::panel_at_pos(
                        tile.x + tile.width / 2,
                        tile.y + tile.height / 2,
                        area,
                        &shown
                    ),
                    Some(shown[index])
                );
                for y in tile.y..tile.bottom() {
                    for x in tile.x..tile.right() {
                        assert!(covered.insert((x, y)));
                    }
                }
            }
            assert_eq!(covered.len(), usize::from(width) * usize::from(height - 1));
        }
    }
}
