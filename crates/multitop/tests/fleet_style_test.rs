use multitop::{
    app::{App, Msg},
    boat, boat_fleet,
    panel::Mode,
};
use multitop_agent::{
    proc::{Proc, Usage},
    proc_name::ProcName,
    proto::Payload,
    render::Snapshot,
};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

fn fixture() -> App {
    let mut app = App::new(vec![boat::local_server()]);
    app.boat = Some(boat_fleet::State::default());
    for cpu in [20.0, 55.0, 95.0].into_iter().cycle().take(200) {
        app.apply(Msg::Packet {
            panel: 0,
            gen: 0,
            epoch: 0,
            dims: (158, 48),
            payload: Payload::Monitor(Snapshot {
                host: "demo-local".into(),
                cpu_pct: cpu,
                cores: (0..8).map(|id| (id, cpu, None)).collect(),
                mem: Usage::new(16 << 30, 6 << 30),
                disk: Usage::new(128 << 30, 40 << 30),
                swap: Some(Usage::new(8 << 30, 1 << 30)),
                procs: (0..100)
                    .map(|id| Proc {
                        pid: id + 100,
                        name: ProcName::new("demo-worker"),
                        cpu: 5.0,
                        mem: 20 << 20,
                    })
                    .collect(),
                ..Snapshot::default()
            }),
        });
    }
    app
}

fn draw(app: &mut App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| multitop::ui::draw(frame, app))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

#[test]
fn framed_stats_keep_network_on_right_and_scroll_processes() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = fixture();
    let screen = draw(&mut app, 160, 50);
    let output = text(&screen);
    for label in [
        "This Mac [local]",
        "SWAP 1.0GiB/8.0GiB",
        "CPU",
        "MEM",
        "Root",
        "Swap",
        "processes",
        "demo-worker",
    ] {
        assert!(output.contains(label), "missing {label}");
    }
    let net = screen
        .content
        .windows(3)
        .position(|cells| {
            cells
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                == "NET"
        })
        .unwrap();
    assert!(net % 160 > 80);
    app.panels[0].scroll_offset = 10;
    assert!(text(&draw(&mut app, 160, 50)).contains("scroll 10"));
}

#[test]
fn graphs_have_height_colors_and_no_network_at_all_sizes() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = fixture();
    app.panels[0].mode = Mode::Graphs;
    app.rerender_all((158, 48));
    let screen = draw(&mut app, 160, 50);
    let output = text(&screen);
    assert!(output.contains("CPU") && output.contains("MEM"));
    assert!(!output.contains("NET"));
    let colors: std::collections::HashSet<_> = screen
        .content
        .iter()
        .filter(|cell| {
            cell.symbol()
                .chars()
                .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
        })
        .map(|cell| cell.fg)
        .collect();
    assert!(colors.len() >= 3, "gradient had {} colors", colors.len());
    for (width, height) in [(1, 1), (24, 8), (40, 12), (80, 24), (200, 60)] {
        assert!(!text(&draw(&mut app, width, height)).contains("NET"));
    }
}

#[test]
fn connection_error_replaces_live_stats_and_stale_swap_title() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    for mode in [Mode::Monitor, Mode::Graphs] {
        let mut app = fixture();
        app.panels[0].mode = mode;
        app.apply(Msg::Frame {
            panel: 0,
            epoch: 0,
            lines: vec!["Connection lost; retrying".into()],
        });
        let output = text(&draw(&mut app, 160, 50));
        assert!(output.contains("Connection lost; retrying"));
        assert!(output.contains("no live sample"));
        assert!(!output.contains("SWAP 1.0GiB"));
        assert!(!output.contains("demo-worker"));
    }
}

#[test]
fn notices_remain_visible_after_visual_redesign() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = fixture();
    app.panels[0].note("Discovery unavailable; keeping previous fleet".into());
    assert!(text(&draw(&mut app, 160, 50)).contains("Discovery unavailable"));
}

#[test]
fn long_names_keep_swap_visible_and_process_controls_are_removed() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = fixture();
    app.panels[0].server.name = Some("demo-machine-with-a-long-branch-name-".repeat(4));
    if let Some(Payload::Monitor(snapshot)) = &mut app.panels[0].last_monitor {
        snapshot.procs[0].name = ProcName::new("\u{1b}[31mworker\r");
    }
    let output = text(&draw(&mut app, 80, 40));
    assert!(output.contains("SWAP 1.0GiB/8.0GiB"));
    assert!(output.contains("worker"));
    assert!(!output.contains('\u{1b}') && !output.contains('\r'));
}

#[test]
fn short_bottom_graph_tile_always_fits_cpu_and_memory_without_scroll() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = fixture();
    let mut second = app.panels[0].clone();
    second.server.name = Some("Demo boat [idle]".into());
    app.panels.push(second);
    for panel in &mut app.panels {
        panel.mode = Mode::Graphs;
    }
    // Simulate a cached frame prepared before a terminal resize, then wheel
    // events over the shorter bottom tile. Neither may move metric headings.
    app.rerender_all((158, 32));
    for height in [36, 40, 42, 44, 50] {
        app.panels[1].scroll_offset = 15;
        let screen = draw(&mut app, 100, height);
        let (areas, _) = multitop::ui::regions(screen.area, 2);
        for area in areas {
            let output: String = (area.y..area.bottom())
                .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
                .map(|point| screen[point].symbol())
                .collect();
            assert!(
                output.contains("CPU") && output.contains("MEM"),
                "height {height}: {output}"
            );
        }
        assert_eq!(app.panels[1].scroll_offset, 0);
        app.scroll_panel_up(1, 3);
        app.scroll_panel_down(1, 3);
        app.selected_panel = 1;
        app.scroll_to_top();
        assert_eq!(app.panels[1].scroll_offset, 0);
    }
}

#[test]
fn short_stats_tiles_only_scroll_processes_while_metrics_stay_fixed() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = fixture();
    for height in [12, 16, 20, 22] {
        app.panels[0].scroll_offset = 0;
        let before = draw(&mut app, 100, height);
        app.scroll_panel_down(0, 5);
        let after = draw(&mut app, 100, height);
        let top_rows = |screen: &Buffer| -> String {
            (1..5)
                .flat_map(|y| (0..100).map(move |x| (x, y)))
                .map(|point| screen[point].symbol())
                .collect()
        };
        assert_eq!(top_rows(&before), top_rows(&after));
        assert!(text(&after).contains("CPU"));
        assert!(text(&after).contains("MEM"));
        assert!(app.panels[0].scroll_offset > 0);
        app.scroll_to_top();
        assert_eq!(app.panels[0].scroll_offset, 0);
    }
}

#[test]
fn graph_headings_survive_fleet_growth_notices_and_uneven_resizes() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = fixture();
    let prototype = app.panels[0].clone();
    for count in [2, 3, 4, 6] {
        app.panels = vec![prototype.clone(); count];
        for panel in &mut app.panels {
            panel.mode = Mode::Graphs;
            panel
                .notes
                .push("Discovery delayed; keeping previous fleet".into());
            panel.scroll_offset = usize::MAX;
        }
        app.rerender_all((158, 48));
        for height in [18, 36, 40, 42, 60] {
            let screen = draw(&mut app, 160, height);
            let (areas, _) = multitop::ui::regions(screen.area, count);
            for area in areas {
                let output: String = (area.y..area.bottom())
                    .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
                    .map(|point| screen[point].symbol())
                    .collect();
                assert!(
                    output.contains("CPU") && output.contains("MEM"),
                    "{count} tiles at height {height}"
                );
            }
            assert!(app.panels.iter().all(|panel| panel.scroll_offset == 0));
        }
    }
}

#[test]
fn a_wide_ten_machine_grid_shows_full_dashboard_sections_without_font_zoom() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = fixture();
    let prototype = app.panels[0].clone();
    app.panels = vec![prototype; 10];
    for (i, panel) in app.panels.iter_mut().enumerate() {
        panel.server.name = Some(format!("Demo machine {i}"));
    }
    for width in [300, 400, 600] {
        let output = text(&draw(&mut app, width, 80));
        assert_eq!(output.matches("disks / swap").count(), 10, "width {width}");
        assert_eq!(output.matches("Used  ").count(), 10, "width {width}");
        assert_eq!(output.matches("processes").count(), 10, "width {width}");
    }
}

#[test]
fn inactive_boat_shows_dead_lettering_instead_of_cached_graphs() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = fixture();
    app.panels[0].server = boat::parse_inventory(br#"{"sandboxes":[{"id":"bx_demo","name":"Demo stopped","state":"stopping","type":"large"}]}"#).unwrap().remove(0);
    app.panels[0].mode = Mode::Graphs;
    for (width, height) in [(160, 50), (80, 18), (40, 12)] {
        let output = text(&draw(&mut app, width, height));
        assert!(output.matches('#').count() > 20);
        assert!(!output.contains("MEM") && !output.contains("processes"));
        assert!(!output.contains("Not running"));
        assert_eq!(app.panels[0].scroll_offset, 0);
    }
    assert!(text(&draw(&mut app, 24, 8)).contains("DEAD"));
}

#[test]
fn inventory_loading_placeholder_is_not_marked_dead() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = App::new(vec![boat::waiting("Discovering boats")]);
    app.boat = Some(boat_fleet::State::default());
    let output = text(&draw(&mut app, 80, 24));
    assert!(!output.contains("DEAD") && !output.contains('#'));
}

#[test]
fn dead_lettering_is_contained_and_centered_in_a_tall_narrow_tile() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let mut app = fixture();
    app.panels[0].server.host = "boat-offline:bx_demo".into();
    let screen = draw(&mut app, 48, 34);
    let painted: Vec<_> = (1..32)
        .filter(|&y| (1..47).any(|x| screen[(x, y)].symbol() == "#"))
        .collect();
    assert_eq!(
        painted.len(),
        9,
        "the approved contained word is nine rows tall"
    );
    let above = painted[0] - 1;
    let below = 31 - painted.last().unwrap();
    assert!(above.abs_diff(below) <= 1);
}
