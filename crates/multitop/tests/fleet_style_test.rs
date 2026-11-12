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
