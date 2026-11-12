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
}

#[tokio::test]
async fn selecting_a_boat_never_writes_dashboard_state() {
    let _guard = multitop::password_store::lock_for_test_async().await;
    multitop::password_store::enable_mock_store();
    multitop::password_store::clear_mock_store();
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("missing/config.toml");
    let mut app = multitop::app::App::new(vec![multitop::boat::waiting("Boat")]);
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
            .remove(0),
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
    assert_eq!(identity(&running[0].host), identity(&stopped[0].host));
    assert!(!offline(&running[0].host));
    assert!(offline(&stopped[0].host));
    let cmd = transport(&running[0]).unwrap();
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
        assert_eq!(transport(&server).unwrap().as_std().get_program(), "false");
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
fn empty_inventory_is_visible_and_remote_names_are_plain_text() {
    let empty = parse_inventory(br#"{"sandboxes":[]}"#).unwrap();
    assert!(offline(&empty[0].host));
    assert!(empty[0].name.as_ref().unwrap().contains("No boats"));
    let boats = parse_inventory(
        br#"{"sandboxes":[{"id":"bx_a","name":"box\u001b[31m\n","state":"idle"}]}"#,
    )
    .unwrap();
    assert!(!boats[0]
        .name
        .as_ref()
        .unwrap()
        .chars()
        .any(char::is_control));
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
    let visible = state.select(stopped.clone());
    assert_eq!(identity(&visible[1].host), Some("bx_a"));
    assert!(offline(&visible[1].host));
    state.visible.remove("bx_a");
    let visible = state.select(stopped);
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
    let mut app = multitop::app::App::new(servers.clone());
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
