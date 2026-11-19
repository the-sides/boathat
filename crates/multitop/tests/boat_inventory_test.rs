use multitop::boat::{identity, offline, parse_inventory, transport};

#[test]
fn local_machine_stays_visible_when_no_boats_are_running() {
    let mut state = multitop::boat_fleet::State::default();
    let shown = state.select(Vec::new());
    assert_eq!(shown.len(), 1);
    assert!(multitop::ssh::is_local(&shown[0]));
    assert_eq!(state.select(Vec::new())[0], shown[0]);
}

#[test]
fn title_shows_swap_used_and_allocated_instead_of_cpu_percentage() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    multitop::password_store::clear_mock_store();
    let mut app = multitop::app::App::new(vec![multitop::boat::local_server()]);
    app.boat = Some(multitop::boat_fleet::State::default());
    app.apply(multitop::app::Msg::Packet {
        panel: 0,
        gen: 0,
        epoch: 0,
        dims: (158, 40),
        payload: multitop_agent::proto::Payload::Monitor(multitop_agent::render::Snapshot {
            host: "local".into(),
            cpu_pct: 70.0,
            cores: vec![(0, 70.0, None)],
            swap: Some(multitop_agent::proc::Usage::new(8 << 30, 1 << 30)),
            ..multitop_agent::render::Snapshot::default()
        }),
    });
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|frame| multitop::ui::draw(frame, &mut app))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(text.contains("SWAP 1.0GiB/8.0GiB"), "{text}");
    assert!(!text.contains("| CPU"), "{text}");
    let screen = terminal.backend().buffer();
    let row = |y| {
        (0..160)
            .map(|x| screen[(x, y)].symbol())
            .collect::<String>()
    };
    assert!(row(1).contains("┐¹cpu┌"));
    // The 37-row tile interior gives CPU 12 rows, then mem and proc
    // share the same top row. The memory width rounds to 71 columns.
    let lower = row(13);
    assert!(lower.contains("┐²mem┌"));
    assert!(lower.contains("┐⁴proc┌"));
    assert_eq!(screen[(72, 13)].symbol(), "╭");
    assert!(text.contains("CPU ■"));
    assert!(text.contains("Swap:") && text.contains("1.0GiB / 8.0GiB"));
    assert!(!text.contains("Free") && !text.contains("disks / swap"));
    for height in [18, 12] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, height)).unwrap();
        terminal
            .draw(|frame| multitop::ui::draw(frame, &mut app))
            .unwrap();
        let resized: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(resized.contains("┐⁴proc┌") && resized.contains('■'));
        assert_eq!(resized.contains("┐²mem┌"), height == 18);
    }
}

#[tokio::test]
async fn selecting_a_boat_never_writes_dashboard_state() {
    let _guard = multitop::password_store::lock_for_test_async().await;
    multitop::password_store::enable_mock_store();
    multitop::password_store::clear_mock_store();
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("missing/config.toml");
    let mut app = multitop::app::App::new(vec![multitop::boat::local_server()]);
    app.boat = Some(multitop::boat_fleet::State::default());
    // Even an accidentally assigned config path cannot make a Boat selection
    // persist. The real Boat boot does not assign a path in the first place.
    app.config_path = Some(config.clone());
    let (tx, _) = tokio::sync::mpsc::channel(1);
    let (_, rx) = tokio::sync::watch::channel((120, 40));
    let mut tasks = multitop::run::Tasks::new(1);
    multitop::run::handle_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('1'),
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut app,
        (120, 40),
        &std::sync::Arc::new(rx),
        &tx,
        &mut tasks,
    );
    assert!(!config.parent().unwrap().exists());
    assert_eq!(app.panels[0].notes, [] as [String; 0]);
}

#[test]
fn boat_upgrade_view_never_requests_credentials() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    multitop::password_store::clear_mock_store();
    let mut app = multitop::app::App::new(vec![
        multitop::boat::local_server(),
        parse_inventory(br#"{"sandboxes":[{"id":"bx_alpha","state":"idle"}]}"#)
            .unwrap()
            .remove(0)
            .server(),
    ]);
    app.boat = Some(multitop::boat_fleet::State::default());
    assert_eq!(
        app.enter_upgrade_view(),
        [] as [(usize, multitop::config::Server); 0]
    );
    assert_eq!(app.run_upgrade(), [] as [multitop::app::Command; 0]);
    assert!(app
        .panels
        .iter()
        .all(multitop::panel::Panel::needs_credential_load));
}

#[test]
fn boat_identity_survives_state_and_address_changes() {
    let running = parse_inventory(br#"{"sandboxes":[{"id":"bx_alpha","name":"Build box","state":"idle","sshEndpoint":"old:123"}]}"#).unwrap();
    let stopped = parse_inventory(br#"{"sandboxes":[{"id":"bx_alpha","name":"Build box","state":"stopped","sshEndpoint":null}]}"#).unwrap();
    let running = running[0].server();
    let stopped = stopped[0].server();
    assert_eq!(identity(&running.host), identity(&stopped.host));
    assert!(!offline(&running.host));
    assert!(offline(&stopped.host));
    let cmd = transport(&running).unwrap();
    let args: Vec<_> = cmd
        .as_std()
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["--no-update", "ssh", "bx_alpha"]);
    assert!(!args.iter().any(|a| a.contains("old:123")));
}

#[test]
fn inactive_boats_cannot_be_started_by_a_view_command() {
    let boats = parse_inventory(
        br#"{"sandboxes":[{"id":"bx_a","state":"pending"},{"id":"bx_b","state":"stopped"}]}"#,
    )
    .unwrap();
    for server in boats {
        assert_eq!(
            transport(&server.server()).unwrap().as_std().get_program(),
            "false"
        );
    }
}

#[test]
fn discovery_refuses_partial_duplicate_and_malformed_inventories() {
    for bytes in [
        br#"{"sandboxes":[],"pageInfo":{"hasMore":true}}"#.as_slice(),
        br#"{"sandboxes":[{"id":"bx_a","state":"idle"},{"id":"bx_a","state":"stopped"}]}"#,
        br#"{"sandboxes":[{"id":"-oProxyCommand=bad","state":"idle"}]}"#,
        br#"{"sandboxes":[{"id":"bx_a"}]}"#,
        br#"{"error":"not_authenticated"}"#,
    ] {
        assert!(parse_inventory(bytes).is_err());
    }
}

#[test]
fn empty_inventory_and_remote_names_are_plain_text() {
    let empty = parse_inventory(br#"{"sandboxes":[]}"#).unwrap();
    assert!(empty.is_empty());
    let boats = parse_inventory(
        br#"{"sandboxes":[{"id":"bx_a","name":"box\u001b[31m\n","state":"idle"}]}"#,
    )
    .unwrap();
    assert!(!boats[0].name().chars().any(char::is_control));
}

#[test]
fn stopped_before_launch_stays_hidden_but_newly_stopped_waits_for_acknowledgment() {
    let mut state = multitop::boat_fleet::State::default();
    let initial = parse_inventory(
        br#"{"sandboxes":[{"id":"bx_a","state":"idle"},{"id":"bx_b","state":"stopped"}]}"#,
    )
    .unwrap();
    let visible = state.select(initial);
    assert_eq!(visible.len(), 2);
    assert_eq!(identity(&visible[1].host), Some("bx_a"));
    let stopped = parse_inventory(
        br#"{"sandboxes":[{"id":"bx_a","state":"stopped"},{"id":"bx_b","state":"stopped"}]}"#,
    )
    .unwrap();
    let visible = state.select(stopped);
    assert_eq!(identity(&visible[1].host), Some("bx_a"));
    assert!(offline(&visible[1].host));
    state.dismiss("bx_a");
    let visible = state.servers();
    assert_eq!(visible.len(), 1);
    assert!(multitop::ssh::is_local(&visible[0]));
    let resumed = parse_inventory(br#"{"sandboxes":[{"id":"bx_a","state":"idle"}]}"#).unwrap();
    assert_eq!(identity(&state.select(resumed)[1].host), Some("bx_a"));
}

#[test]
fn overall_cpu_is_core_weighted_and_excludes_missing_or_expired_samples() {
    use multitop_agent::proto::Payload;
    use multitop_agent::render::Snapshot;
    use std::time::Instant;
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    multitop::password_store::clear_mock_store();
    let servers = parse_inventory(
        br#"{"sandboxes":[{"id":"bx_a","state":"idle"},{"id":"bx_b","state":"idle"}]}"#,
    )
    .unwrap();
    let mut app = multitop::app::App::new(
        servers
            .iter()
            .map(multitop::boat::Sandbox::server)
            .collect(),
    );
    let mut state = multitop::boat_fleet::State::default();
    state.select(servers);
    for (panel, cpu, cores) in [(0, 100.0, 2), (1, 0.0, 6)] {
        app.panels[panel].last_monitor = Some(Payload::Monitor(Snapshot {
            cpu_pct: cpu,
            cores: (0..cores).map(|i| (i, cpu, None)).collect(),
            ..Snapshot::default()
        }));
        state.samples.insert(
            identity(&app.panels[panel].server.host).unwrap().into(),
            Instant::now(),
        );
    }
    app.boat = Some(state);
    assert!(multitop::boat_fleet::summary(&app).contains("25.0% (2/2 reporting)"));
    app.boat.as_mut().unwrap().samples.insert(
        "bx_a".into(),
        Instant::now()
            .checked_sub(multitop::boat::POLL_INTERVAL * 2)
            .unwrap(),
    );
    assert!(multitop::boat_fleet::summary(&app).contains("0.0% (1/2 reporting)"));
}

#[test]
fn boat_titles_include_reported_size_for_running_and_stopped_machines() {
    let boats = parse_inventory(
        br#"{"sandboxes":[
        {"id":"bx_a","name":"Demo build","state":"idle","type":"large"},
        {"id":"bx_b","name":"Demo tests","state":"stopped","type":"xlarge"},
        {"id":"bx_c","name":"Demo default","state":"idle","type":"default"},
        {"id":"bx_d","name":"Demo missing","state":"idle"},
        {"id":"bx_e","name":"Demo escaped","state":"idle","type":"large\u001b\n"}
    ]}"#,
    )
    .unwrap();
    for (boat, size) in boats
        .iter()
        .zip(["large", "xlarge", "default", "unknown", "large"])
    {
        let server = boat.server();
        let name = server.name.as_deref().unwrap();
        assert!(name.ends_with(&format!("SIZE {size}")), "{name:?}");
        assert!(!name.chars().any(char::is_control));
    }
}

#[test]
fn long_boat_names_keep_size_and_swap_visible_in_tile_titles() {
    let _guard = multitop::password_store::lock_for_test();
    multitop::password_store::enable_mock_store();
    let servers = parse_inventory(br#"{"sandboxes":[{"id":"bx_demo","name":"Very long demo machine name for an integration test with a distinctive suffix","state":"idle","type":"xlarge"}]}"#).unwrap();
    let mut app = multitop::app::App::new(
        servers
            .iter()
            .map(multitop::boat::Sandbox::server)
            .collect(),
    );
    app.boat = Some(multitop::boat_fleet::State::default());
    app.apply(multitop::app::Msg::Packet {
        panel: 0,
        gen: 0,
        epoch: 0,
        dims: (78, 38),
        payload: multitop_agent::proto::Payload::Monitor(multitop_agent::render::Snapshot {
            swap: Some(multitop_agent::proc::Usage::new(8 << 30, 1 << 30)),
            ..multitop_agent::render::Snapshot::default()
        }),
    });
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 40)).unwrap();
    terminal.draw(|f| multitop::ui::draw(f, &mut app)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(text.contains("SIZE xlarge"));
    assert!(text.contains("SWAP 1.0GiB/8.0GiB"));
}

#[test]
fn list_draws_lease_and_live_metrics_on_one_row_above_stopped_boats() {
    use multitop::app::{App, Msg};
    use multitop_agent::{proc::Usage, proto::Payload, render::Snapshot};

    let sandboxes = parse_inventory(br#"{"sandboxes":[
        {"id":"bx_dead","name":"Finished","state":"stopped","type":"xlarge","subdomain":"hidden-dead"},
        {"id":"bx_live","name":"Active build","state":"idle","type":"large","subdomain":"hidden-live","archiveAfter":"2099-12-31T12:34:56.789Z"}
    ]}"#).unwrap();
    assert_eq!(sandboxes[1].lease_deadline(), Some(4_102_403_696));
    let mut state = multitop::boat_fleet::State::default();
    state.list_open = true;
    let mut app = App::new(state.select(sandboxes));
    app.boat = Some(state);
    assert!(app.apply(Msg::Packet {
        panel: 0,
        epoch: 0,
        gen: 0,
        dims: (100, 24),
        payload: Payload::Monitor(Snapshot {
            cpu_pct: 14.0,
            mem: Usage::new(32 << 30, 8 << 30),
            swap: Some(Usage::new(8 << 30, 1 << 30)),
            ..Snapshot::default()
        }),
    }));
    assert!(app.apply(Msg::Packet {
        panel: 1,
        epoch: 0,
        gen: 0,
        dims: (100, 24),
        payload: Payload::Monitor(Snapshot {
            cpu_pct: 37.0,
            mem: Usage::new(16 << 30, 6 << 30),
            ..Snapshot::default()
        }),
    }));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 12)).unwrap();
    terminal
        .draw(|frame| multitop::ui::draw(frame, &mut app))
        .unwrap();
    let screen = terminal.backend().buffer();
    let row = |y| {
        (0..120)
            .map(|x| screen[(x, y)].symbol())
            .collect::<String>()
    };
    assert!(row(0).contains("This Mac · Local"));
    let metrics = row(1);
    for value in ["CPU 14%", "Memory 8.0G/32.0G", "Swap 1.0G/8.0G"] {
        assert!(metrics.contains(value), "{metrics}");
    }
    assert_eq!(row(2), "━".repeat(120));
    assert!(row(3).contains("Boat sandboxes"));
    let header = row(4);
    assert!(header.contains("State") && header.contains("Time left") && header.contains("Name"));
    assert!(header.contains("Run time") && header.contains("Compute $"));
    let active = row(5);
    for value in ["idle", "37%", "6.0G/16.0G", " L ", "Active build"] {
        assert!(active.contains(value), "{active}");
    }
    assert!(active.contains("h "));
    assert!(row(6).contains("stopped") && row(6).contains(" XL ") && row(6).contains("Finished"));
    let frame = (0..12).map(row).collect::<String>();
    assert!(!frame.contains("bx_") && !frame.contains("hidden-"));

    let mut compact = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 7)).unwrap();
    app.boat.as_mut().unwrap().list_scroll = usize::MAX;
    compact.draw(|f| multitop::ui::draw(f, &mut app)).unwrap();
    let compact_screen = compact.backend().buffer();
    let compact_row = |y| {
        (0..80)
            .map(|x| compact_screen[(x, y)].symbol())
            .collect::<String>()
    };
    assert!(compact_row(0).contains("This Mac · Local"));
    assert_eq!(compact_row(2), "━".repeat(80));
    assert!(compact_row(4).contains("Run time") && compact_row(4).contains("Compute $"));
    assert!(compact_row(5).contains("Finished"));
}
