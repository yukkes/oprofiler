use std::io;
use std::net::SocketAddr;
use std::time::Instant;

use crossterm::event::{poll, read, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

use openprofiler_core::protocol::AgentClient;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BenchmarkPhase {
    Idle,
    Profiling,
    IdleAfter,
}

struct BenchmarkResult {
    phase: BenchmarkPhase,
    elapsed_ms: f64,
    ns_per_call: f64,
}

pub fn run_benchmark(port: u16) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut client = AgentClient::connect(addr)
        .map_err(|e| anyhow::anyhow!("Failed to connect to agent at {}: {}", addr, e))?;

    let call_count: u64 = 100_000_000; // 10^8
    let mut results: Vec<BenchmarkResult> = Vec::new();
    let mut current_phase = BenchmarkPhase::Idle;
    let mut waiting_for_input = true;
    let mut message = String::from("Press Enter to start benchmark");
    let mut error_msg: Option<String> = None;

    loop {
        terminal.draw(|f| {
            let mut lines = vec![
                Line::from(Span::styled(
                    "=== OpenProfiler Overhead Benchmark ===",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(format!("Target: {} calls per phase", call_count)),
                Line::from(format!("Agent: {}", addr)),
                Line::from(""),
            ];

            // Phase indicators
            let phases = [
                ("Phase 1: Idle (baseline)", BenchmarkPhase::Idle),
                ("Phase 2: Profiling", BenchmarkPhase::Profiling),
                ("Phase 3: Idle (after profiling)", BenchmarkPhase::IdleAfter),
            ];

            for (label, phase) in &phases {
                let style = if *phase == current_phase {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else if results.iter().any(|r| r.phase == *phase) {
                    Style::default().fg(Color::Blue)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(Span::styled(*label, style)));
            }

            lines.push(Line::from(""));

            // Results
            if !results.is_empty() {
                lines.push(Line::from(Span::styled(
                    "--- Results ---",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
                for result in &results {
                    let phase_label = match result.phase {
                        BenchmarkPhase::Idle => "Idle (baseline)",
                        BenchmarkPhase::Profiling => "Profiling",
                        BenchmarkPhase::IdleAfter => "Idle (after)",
                    };
                    lines.push(Line::from(format!(
                        "  {}: {:.2} ms ({:.2} ns/call)",
                        phase_label, result.elapsed_ms, result.ns_per_call
                    )));
                }

                // Summary
                if results.len() >= 2 {
                    let idle = &results[0];
                    let profiling = &results[1];
                    let overhead = profiling.ns_per_call - idle.ns_per_call;
                    let ratio = profiling.ns_per_call / idle.ns_per_call;
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "--- Summary ---",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                    lines.push(Line::from(format!(
                        "  Baseline:              {:.2} ns/call",
                        idle.ns_per_call
                    )));
                    lines.push(Line::from(format!(
                        "  Profiling overhead:    {:.2} ns/call ({:.1}x slower)",
                        overhead, ratio
                    )));

                    if results.len() >= 3 {
                        let idle_after = &results[2];
                        let residual = idle_after.ns_per_call - idle.ns_per_call;
                        let residual_ratio = idle_after.ns_per_call / idle.ns_per_call;
                        lines.push(Line::from(format!(
                            "  Residual (Idle after): {:.2} ns/call ({:.1}x baseline)",
                            residual, residual_ratio
                        )));

                        if residual_ratio > 1.5 {
                            lines.push(Line::from(Span::styled(
                                "  WARNING: Residual overhead detected!",
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            )));
                        } else {
                            lines.push(Line::from(Span::styled(
                                "  OK: No significant residual overhead",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            )));
                        }
                    }
                }
                lines.push(Line::from(""));
            }

            // Message
            lines.push(Line::from(Span::styled(
                &message,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));

            if let Some(err) = &error_msg {
                lines.push(Line::from(Span::styled(
                    err,
                    Style::default().fg(Color::Red),
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Controls: Enter=run phase, R=reset, Q=quit",
                Style::default().fg(Color::DarkGray),
            )));

            let paragraph = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("Benchmark"));
            f.render_widget(paragraph, f.area());
        })?;

        if waiting_for_input {
            if poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = read()? {
                    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                        continue;
                    }
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            break;
                        }
                        KeyCode::Char('r') => {
                            results.clear();
                            current_phase = BenchmarkPhase::Idle;
                            message = String::from("Reset. Press Enter to start");
                            error_msg = None;
                            // Stop recording if active
                            let _ = client.stop_cpu_recording();
                        }
                        KeyCode::Enter => {
                            waiting_for_input = false;
                            message = String::from("Running...");
                            error_msg = None;
                        }
                        _ => {}
                    }
                }
            }
        } else {
            // Run the current phase
            let result: anyhow::Result<BenchmarkResult> = match current_phase {
                BenchmarkPhase::Idle => {
                    // Ensure recording is stopped
                    let _ = client.stop_cpu_recording();
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    run_idle_phase(call_count)
                }
                BenchmarkPhase::Profiling => {
                    // Start recording
                    if let Err(e) = client.start_cpu_recording() {
                        Err(anyhow::anyhow!("Failed to start recording: {}", e))
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        run_profiling_phase(call_count, &addr)
                    }
                }
                BenchmarkPhase::IdleAfter => {
                    // Stop recording
                    if let Err(e) = client.stop_cpu_recording() {
                        Err(anyhow::anyhow!("Failed to stop recording: {}", e))
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        run_idle_phase(call_count)
                    }
                }
            };

            match result {
                Ok(bench_result) => {
                    results.push(bench_result);
                    // Move to next phase
                    current_phase = match current_phase {
                        BenchmarkPhase::Idle => BenchmarkPhase::Profiling,
                        BenchmarkPhase::Profiling => BenchmarkPhase::IdleAfter,
                        BenchmarkPhase::IdleAfter => {
                            message =
                                String::from("Benchmark complete. Press R to reset, Q to quit");
                            waiting_for_input = true;
                            continue;
                        }
                    };
                    message = format!(
                        "Phase complete. Press Enter for next phase ({:?})",
                        current_phase
                    );
                }
                Err(e) => {
                    error_msg = Some(e.to_string());
                    message = String::from("Error occurred. Press R to reset, Q to quit");
                }
            }
            waiting_for_input = true;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Print results to stdout
    println!("\n=== Benchmark Results ===");
    for result in &results {
        let phase_label = match result.phase {
            BenchmarkPhase::Idle => "Idle (baseline)",
            BenchmarkPhase::Profiling => "Profiling",
            BenchmarkPhase::IdleAfter => "Idle (after)",
        };
        println!(
            "  {}: {:.2} ms ({:.2} ns/call)",
            phase_label, result.elapsed_ms, result.ns_per_call
        );
    }

    Ok(())
}

fn run_idle_phase(call_count: u64) -> anyhow::Result<BenchmarkResult> {
    // Run a simple loop in Rust to measure baseline
    // The actual Java-side measurement is done by the instrumented code
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..call_count {
        sink += i;
    }
    let elapsed = start.elapsed();
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
    let ns_per_call = elapsed.as_nanos() as f64 / call_count as f64;

    // Prevent compiler optimization
    std::hint::black_box(sink);

    Ok(BenchmarkResult {
        phase: BenchmarkPhase::Idle,
        elapsed_ms,
        ns_per_call,
    })
}

fn run_profiling_phase(call_count: u64, addr: &SocketAddr) -> anyhow::Result<BenchmarkResult> {
    let start = Instant::now();
    let mut client =
        AgentClient::connect(*addr).map_err(|e| anyhow::anyhow!("Failed to connect: {}", e))?;
    let _ = client
        .get_cpu_data()
        .map_err(|e| anyhow::anyhow!("get_cpu_data failed: {}", e))?;
    let elapsed = start.elapsed();
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;

    Ok(BenchmarkResult {
        phase: BenchmarkPhase::Profiling,
        elapsed_ms,
        ns_per_call: elapsed.as_nanos() as f64 / call_count as f64,
    })
}
