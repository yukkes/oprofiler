pub mod app;
pub mod benchmark;
pub mod call_tree;
pub mod event;
pub mod format;
pub mod render;

use std::io;
use std::net::SocketAddr;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{poll, read, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use self::app::App;
use self::format::{format_count, format_duration_nano_f};
use openprofiler_core::protocol::AgentClient;

pub fn main_impl() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--database-once") {
        return print_database_once(&args);
    }
    if args.iter().any(|arg| arg == "--hotspots-once") {
        return print_hotspots_once(&args);
    }
    if args.iter().any(|arg| arg == "--benchmark") {
        let port = arg_value(&args, "--port")
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(8849);
        return benchmark::run_benchmark(port);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    if let Some(port) = arg_value(&args, "--port").and_then(|value| value.parse::<u16>().ok()) {
        app.set_agent_port(port);
    }
    app.spawn_refresh_jvms();

    let result = run_terminal(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn print_hotspots_once(args: &[String]) -> anyhow::Result<()> {
    let port = arg_value(args, "--port")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8849);
    let record_ms = arg_value(args, "--record-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1500);
    let limit = arg_value(args, "--limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut client = AgentClient::connect(addr).map_err(anyhow::Error::msg)?;
    client.start_cpu_recording().map_err(anyhow::Error::msg)?;
    thread::sleep(Duration::from_millis(record_ms));
    let cpu_data = client.get_cpu_data().map_err(anyhow::Error::msg)?;
    client.stop_cpu_recording().map_err(anyhow::Error::msg)?;

    let mut rows: Vec<_> = cpu_data
        .hot_spots
        .into_iter()
        .filter(|row| row.class_name != "SQL")
        .collect();
    rows.sort_by(|a, b| b.self_duration_nano.cmp(&a.self_duration_nano));
    let total_self = rows
        .iter()
        .map(|row| row.self_duration_nano)
        .sum::<u64>()
        .max(1);

    for row in rows.into_iter().take(limit) {
        let percent = row.self_duration_nano as f64 * 100.0 / total_self as f64;
        let avg_nanos = if row.invocations > 0 {
            row.self_duration_nano as f64 / row.invocations as f64
        } else {
            0.0
        };
        println!(
            "{percent:.1}% - {} - {} hot spot inv. {}.{}{} avg {}",
            format_duration_nano_f(row.self_duration_nano as f64),
            format_count(row.invocations),
            row.class_name,
            row.method_name,
            row.method_descriptor,
            format_duration_nano_f(avg_nanos),
        );
    }

    Ok(())
}

fn print_database_once(args: &[String]) -> anyhow::Result<()> {
    let port = arg_value(args, "--port")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8849);
    let record_ms = arg_value(args, "--record-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1500);
    let limit = arg_value(args, "--limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut client = AgentClient::connect(addr).map_err(anyhow::Error::msg)?;
    client.start_cpu_recording().map_err(anyhow::Error::msg)?;
    thread::sleep(Duration::from_millis(record_ms));
    let cpu_data = client.get_cpu_data().map_err(anyhow::Error::msg)?;
    client.stop_cpu_recording().map_err(anyhow::Error::msg)?;

    let mut rows: Vec<_> = cpu_data
        .hot_spots
        .into_iter()
        .filter(|row| row.class_name == "SQL")
        .collect();
    rows.sort_by(|a, b| b.self_duration_nano.cmp(&a.self_duration_nano));
    let total_self = rows
        .iter()
        .map(|row| row.self_duration_nano)
        .sum::<u64>()
        .max(1);

    for row in rows.into_iter().take(limit) {
        let percent = row.self_duration_nano as f64 * 100.0 / total_self as f64;
        let avg_nanos = if row.invocations > 0 {
            row.self_duration_nano as f64 / row.invocations as f64
        } else {
            0.0
        };
        println!(
            "{percent:.1}% - {} - {} hot spot evt. {} avg {}",
            format_duration_nano_f(row.self_duration_nano as f64),
            format_count(row.invocations),
            row.method_name,
            format_duration_nano_f(avg_nanos),
        );
    }

    Ok(())
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

fn run_terminal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    let mut key_debounce = KeyDebounce::default();
    loop {
        app.poll_queries();
        terminal.draw(|f| render::render(f, app))?;
        app.auto_update();

        if poll(Duration::from_millis(200))? {
            if let Event::Key(key) = read()? {
                if !key_debounce.should_accept(key) {
                    continue;
                }
                app.poll_queries();
                if key.code == KeyCode::Char('?') && app.input_mode == event::InputMode::Normal {
                    app.show_help = !app.show_help;
                    continue;
                }
                if app.show_help && key.code == KeyCode::Esc {
                    app.show_help = false;
                    continue;
                }
                if !app.show_help {
                    let result = event::handle_key(app, key);
                    if result.should_quit() {
                        return Ok(());
                    }
                    app.input_mode = result.input_mode;
                }
            }
        }
    }
}

#[derive(Default)]
struct KeyDebounce {
    last: Option<(KeyCode, KeyModifiers, Instant)>,
}

impl KeyDebounce {
    fn should_accept(&mut self, key: KeyEvent) -> bool {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return false;
        }
        if key.kind == KeyEventKind::Repeat || is_navigation_key(key.code) {
            return true;
        }

        let now = Instant::now();
        let is_duplicate = self.last.as_ref().is_some_and(|(code, modifiers, at)| {
            *code == key.code
                && *modifiers == key.modifiers
                && now.duration_since(*at) < Duration::from_millis(180)
        });
        if is_duplicate {
            return false;
        }

        self.last = Some((key.code, key.modifiers, now));
        true
    }
}

fn is_navigation_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Char('h')
            | KeyCode::Char('j')
            | KeyCode::Char('k')
            | KeyCode::Char('l')
    )
}

#[cfg(test)]
mod integration_tests {
    use std::thread;
    use std::time::Duration;

    use super::app::{App, FocusPane};
    use openprofiler_core::model::ViewId;

    #[test]
    fn test_focus_navigation_model_for_cpu_hotspots() {
        let mut app = App::new();
        assert!(app.selected_view == ViewId::StartCenter);
        assert!(matches!(app.focus_pane, FocusPane::Sidebar));

        app.go_to_view(ViewId::TeleOverview);
        app.navigate_down();
        assert!(app.selected_view == ViewId::CpuHotSpots);
        app.navigate_down();
        assert!(app.selected_view == ViewId::LiveAllocationHotSpots);
        app.navigate_down();
        assert!(app.selected_view == ViewId::DatabasesJdbc);

        app.toggle_focus_pane();
        assert!(matches!(app.focus_pane, FocusPane::Main));
        app.go_to_view(ViewId::CpuHotSpots);
        app.cpu.methods = vec![
            openprofiler_core::model::CpuMethodRow {
                method_id: Some(1),
                method: "a.A.foo()V".to_string(),
                total_samples: 0,
                self_samples: 0,
                total_ms: 20.0,
                self_ms: 20.0,
                percent: 1.0,
                class_name: "a.A".to_string(),
                method_name: "foo".to_string(),
                descriptor: "()V".to_string(),
                invocations: 1,
                average_nanos: 20_000_000.0,
            },
            openprofiler_core::model::CpuMethodRow {
                method_id: Some(2),
                method: "a.B.bar()V".to_string(),
                total_samples: 0,
                self_samples: 0,
                total_ms: 10.0,
                self_ms: 10.0,
                percent: 0.5,
                class_name: "a.B".to_string(),
                method_name: "bar".to_string(),
                descriptor: "()V".to_string(),
                invocations: 1,
                average_nanos: 10_000_000.0,
            },
        ];
        app.cpu.method_edges = vec![openprofiler_core::model::CpuMethodEdgeRow {
            from_method_id: 1,
            to_method_id: 2,
            call_count: 1,
            total_duration_nano: 10_000_000,
        }];

        app.navigate_down();
        assert_eq!(app.cpu.selected_hotspot_idx, 1);
        app.navigate_up();
        assert_eq!(app.cpu.selected_hotspot_idx, 0);
        app.navigate_right();
        assert!(app.cpu.expanded_hotspots.contains(&1));
        app.navigate_left();
        assert!(!app.cpu.expanded_hotspots.contains(&1));

        app.jvms = vec![
            openprofiler_core::model::JvmTarget {
                pid: 100,
                display_name: "first".to_string(),
                main_class: "First".to_string(),
                arguments: String::new(),
                profiled: false,
            },
            openprofiler_core::model::JvmTarget {
                pid: 200,
                display_name: "second".to_string(),
                main_class: "Second".to_string(),
                arguments: String::new(),
                profiled: false,
            },
        ];
        app.selected_pid = Some(100);
        app.go_to_view(ViewId::StartCenter);
        app.navigate_down();
        assert_eq!(app.selected_pid, Some(200));
    }

    #[test]
    fn test_enter_on_start_center_uses_initial_jvm_selection_once() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new();
        app.jvms = vec![openprofiler_core::model::JvmTarget {
            pid: 100,
            display_name: "first".to_string(),
            main_class: "First".to_string(),
            arguments: String::new(),
            profiled: false,
        }];

        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let started_at = std::time::Instant::now();
        super::event::handle_key(&mut app, key);

        assert!(
            started_at.elapsed() < std::time::Duration::from_millis(100),
            "Enter should switch views before agent attach/recording work starts"
        );
        assert_eq!(app.selected_pid, Some(100));
        assert!(app.selected_view == ViewId::TeleOverview);
        assert!(!app.cpu.recording);
    }

    #[test]
    fn test_main_focus_navigation_for_memory_and_database() {
        let mut app = App::new();
        app.go_to_view(ViewId::LiveAllocationHotSpots);
        app.memory.rows = vec![
            openprofiler_core::model::MemoryClassRow {
                rank: 1,
                name: "a.A".to_string(),
                instances: 10,
                bytes: 100,
                delta_instances: 0,
                delta_bytes: 0,
            },
            openprofiler_core::model::MemoryClassRow {
                rank: 2,
                name: "b.B".to_string(),
                instances: 20,
                bytes: 200,
                delta_instances: 0,
                delta_bytes: 0,
            },
        ];
        app.toggle_focus_pane();
        assert!(matches!(app.focus_pane, FocusPane::Main));
        app.navigate_down();
        assert!(app.selected_view == ViewId::LiveAllocationHotSpots);
        assert_eq!(app.memory.selected_row_idx, 1);

        app.go_to_view(ViewId::DatabasesJdbc);
        app.cpu.methods = vec![
            openprofiler_core::model::CpuMethodRow {
                method_id: None,
                method: "SQL.SELECT 1".to_string(),
                total_samples: 0,
                self_samples: 0,
                total_ms: 10.0,
                self_ms: 10.0,
                percent: 0.5,
                class_name: "SQL".to_string(),
                method_name: "SELECT 1".to_string(),
                descriptor: String::new(),
                invocations: 1,
                average_nanos: 10_000_000.0,
            },
            openprofiler_core::model::CpuMethodRow {
                method_id: None,
                method: "SQL.SELECT 2".to_string(),
                total_samples: 0,
                self_samples: 0,
                total_ms: 5.0,
                self_ms: 5.0,
                percent: 0.25,
                class_name: "SQL".to_string(),
                method_name: "SELECT 2".to_string(),
                descriptor: String::new(),
                invocations: 1,
                average_nanos: 5_000_000.0,
            },
        ];
        app.navigate_down();
        assert!(app.selected_view == ViewId::DatabasesJdbc);
        assert_eq!(app.database.selected_row_idx, 1);
    }

    #[test]
    fn test_memory_hotspots_start_collapsed_and_page_scroll() {
        let mut app = App::new();
        app.go_to_view(ViewId::LiveAllocationHotSpots);
        app.toggle_focus_pane();
        app.memory.allocation_hotspots = (1..=25)
            .map(|id| openprofiler_core::model::AllocationHotSpotRow {
                node_id: id,
                parent_id: None,
                depth: 0,
                name: format!("root{id}"),
                allocated_type: "java.lang.Object".to_string(),
                bytes: 1_000 - id as u64,
                allocations: id as u64,
            })
            .collect();
        app.memory
            .allocation_hotspots
            .push(openprofiler_core::model::AllocationHotSpotRow {
                node_id: 100,
                parent_id: Some(1),
                depth: 1,
                name: "child".to_string(),
                allocated_type: "java.lang.String".to_string(),
                bytes: 100,
                allocations: 1,
            });

        assert_eq!(app.visible_allocation_hotspot_rows().len(), 25);
        assert!(app.allocation_hotspot_has_children(1));

        app.navigate_right();
        assert!(app.memory.expanded_hotspots.contains(&1));
        assert_eq!(app.visible_allocation_hotspot_rows().len(), 26);

        app.page_down();
        assert_eq!(app.memory.selected_row_idx, 20);
        assert_eq!(app.memory.row_offset, 1);

        app.page_up();
        assert_eq!(app.memory.selected_row_idx, 0);
        assert_eq!(app.memory.row_offset, 0);
    }

    #[test]
    fn test_key_repeat_is_accepted_for_fast_navigation() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

        let mut debounce = super::KeyDebounce::default();
        let press = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let repeat = KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Repeat,
            state: crossterm::event::KeyEventState::NONE,
        };

        assert!(debounce.should_accept(press));
        assert!(debounce.should_accept(press));
        assert!(debounce.should_accept(repeat));
    }

    #[test]
    fn test_cpu_and_database_hotspots_are_separated() {
        let mut app = App::new();
        app.cpu.methods = vec![
            openprofiler_core::model::CpuMethodRow {
                method_id: Some(1),
                method: "app.Service.run()V".to_string(),
                total_samples: 0,
                self_samples: 0,
                total_ms: 20.0,
                self_ms: 20.0,
                percent: 0.7,
                class_name: "app.Service".to_string(),
                method_name: "run".to_string(),
                descriptor: "()V".to_string(),
                invocations: 2,
                average_nanos: 10_000_000.0,
            },
            openprofiler_core::model::CpuMethodRow {
                method_id: Some(2),
                method: "SQL.SELECT * FROM inventory".to_string(),
                total_samples: 0,
                self_samples: 0,
                total_ms: 10.0,
                self_ms: 10.0,
                percent: 0.3,
                class_name: "SQL".to_string(),
                method_name: "SELECT * FROM inventory".to_string(),
                descriptor: String::new(),
                invocations: 1,
                average_nanos: 10_000_000.0,
            },
            openprofiler_core::model::CpuMethodRow {
                method_id: Some(3),
                method: "javax.persistence.Query.getResultList()Ljava/util/List;".to_string(),
                total_samples: 0,
                self_samples: 0,
                total_ms: 5.0,
                self_ms: 5.0,
                percent: 0.1,
                class_name: "javax.persistence.Query".to_string(),
                method_name: "getResultList".to_string(),
                descriptor: "()Ljava/util/List;".to_string(),
                invocations: 1,
                average_nanos: 5_000_000.0,
            },
        ];
        app.cpu.method_edges = vec![openprofiler_core::model::CpuMethodEdgeRow {
            from_method_id: 1,
            to_method_id: 2,
            call_count: 1,
            total_duration_nano: 10_000_000,
        }];

        let cpu_rows = app.filtered_cpu_rows();
        assert_eq!(cpu_rows.len(), 2);
        assert!(cpu_rows.iter().all(|row| row.class_name != "SQL"));
        assert!(app.hotspot_children(1).is_empty());
        assert_eq!(app.database_row_count(), 1);
    }

    #[test]
    fn test_s_key_toggles_cpu_recording_against_agent_protocol() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::mpsc;

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use prost::Message;

        use super::event;
        use openprofiler_core::proto::com_openprofiler_protocol::{
            command, Command as AgentCommand, CpuData, HeartbeatData, HotSpot, ProfilingData,
        };

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake agent");
        listener
            .set_nonblocking(true)
            .expect("set fake agent nonblocking");
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while std::time::Instant::now() < deadline {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(20));
                    continue;
                };
                stream
                    .set_nonblocking(false)
                    .expect("set fake stream blocking");
                let mut len_bytes = [0u8; 4];
                stream
                    .read_exact(&mut len_bytes)
                    .expect("read command length");
                let len = u32::from_be_bytes(len_bytes) as usize;
                let mut data = vec![0u8; len];
                stream.read_exact(&mut data).expect("read command data");
                let command = AgentCommand::decode(&data[..]).expect("decode command");
                tx.send(command.r#type).unwrap();

                let response = if command.r#type == command::CommandType::GetCpuData as i32 {
                    ProfilingData {
                        r#type:
                            openprofiler_core::proto::com_openprofiler_protocol::profiling_data::DataType::CpuData
                                as i32,
                        timestamp_nano: 0,
                        payload: Some(
                            openprofiler_core::proto::com_openprofiler_protocol::profiling_data::Payload::CpuData(
                                CpuData {
                                    hot_spots: vec![HotSpot {
                                        class_name: "example.Target".to_string(),
                                        method_name: "work".to_string(),
                                        method_descriptor: "()V".to_string(),
                                        invocations: 1,
                                        self_duration_nano: 1,
                                        total_duration_nano: 1,
                                        ..HotSpot::default()
                                    }],
                                    ..CpuData::default()
                                },
                            ),
                        ),
                    }
                } else {
                    ProfilingData {
                        r#type: openprofiler_core::proto::com_openprofiler_protocol::profiling_data::DataType::Heartbeat as i32,
                        timestamp_nano: 0,
                        payload: Some(openprofiler_core::proto::com_openprofiler_protocol::profiling_data::Payload::Heartbeat(
                            HeartbeatData {
                                uptime_nano: 1,
                                active_recordings: 1,
                            },
                        )),
                    }
                };
                let bytes = response.encode_to_vec();
                stream
                    .write_all(&(bytes.len() as u32).to_be_bytes())
                    .expect("write response length");
                stream.write_all(&bytes).expect("write response data");
            }
        });

        let mut app = App::new();
        app.set_agent_port(port);
        app.selected_pid = Some(std::process::id());

        let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        event::handle_key(&mut app, key);
        app.auto_update();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !app.cpu.recording && std::time::Instant::now() < deadline {
            app.poll_queries();
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            app.cpu.recording,
            "logs: {}",
            app.logs
                .iter()
                .map(|log| format!("{}: {}", log.action, log.message))
                .collect::<Vec<_>>()
                .join(" | ")
        );
        assert!(app.selected_view == ViewId::TeleOverview);
        assert_command_received(
            &rx,
            command::CommandType::StartCpuRecording as i32,
            Duration::from_secs(5),
        );
        assert_command_received(
            &rx,
            command::CommandType::GetCpuData as i32,
            Duration::from_secs(5),
        );

        event::handle_key(&mut app, key);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while app.cpu.recording && std::time::Instant::now() < deadline {
            app.poll_queries();
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!app.cpu.recording);
        assert_command_received(
            &rx,
            command::CommandType::StopCpuRecording as i32,
            Duration::from_secs(5),
        );
        server.join().unwrap();
    }

    fn assert_command_received(
        rx: &std::sync::mpsc::Receiver<i32>,
        expected: i32,
        timeout: Duration,
    ) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(
                !remaining.is_zero(),
                "timed out waiting for command type {expected}"
            );
            let received = rx.recv_timeout(remaining).expect("receive command");
            if received == expected {
                return;
            }
        }
    }
}
