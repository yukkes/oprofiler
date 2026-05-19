use std::collections::VecDeque;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table},
    Frame,
};
use std::cmp::Reverse;

use super::app::{database_method_label, is_database_method, App, FocusPane};
use super::format::*;
use openprofiler_core::model::*;

pub fn render(f: &mut Frame, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    render_toolbar(f, app, outer[0]);
    render_main(f, app, outer[1]);
    render_status_bar(f, app, outer[2]);
}

fn render_toolbar(f: &mut Frame, app: &App, area: Rect) {
    let recording = app.active_recording_count() > 0;
    let status = if app.selected_pid.is_some() {
        "Profiling"
    } else {
        "Disconnected"
    };
    let pid_str = app
        .selected_pid
        .map_or("VM # -".to_string(), |p| format!("VM #{p}"));

    let spans = vec![
        Span::styled(" [1] Tele ", Style::default().fg(Color::Cyan)),
        Span::styled(" [2] CPU ", Style::default().fg(Color::Rgb(238, 164, 24))),
        Span::styled(" [3] Mem ", Style::default().fg(Color::Rgb(255, 137, 0))),
        Span::styled(" [4] DB ", Style::default().fg(Color::Rgb(30, 168, 221))),
        Span::raw("  "),
        if recording {
            Span::styled(
                " ● REC ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(" ● IDLE ", Style::default().fg(Color::Green))
        },
        Span::raw("  "),
        Span::raw(&pid_str),
        Span::raw("  "),
        Span::raw(status),
        Span::raw(format!("  Agent :{}", app.cpu.agent_port)),
        Span::raw("  "),
        Span::styled(
            match app.focus_pane {
                FocusPane::Sidebar => " Focus: Nav ",
                FocusPane::Main => " Focus: Main ",
            },
            Style::default().fg(Color::Yellow),
        ),
    ];

    let toolbar =
        Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Rgb(40, 44, 52)));
    f.render_widget(toolbar, area);
}

fn render_main(f: &mut Frame, app: &App, area: Rect) {
    if app.selected_view == ViewId::StartCenter {
        render_content(f, app, area);
        return;
    }
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(0)])
        .split(area);

    render_sidebar(f, app, main[0]);
    render_content(f, app, main[1]);
}

fn render_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();

    for (i, (_cat, label, views)) in CATEGORIES.iter().enumerate() {
        let is_current_cat =
            i == app.selected_category_idx || views.iter().any(|view| *view == app.selected_view);

        let cat_style = if is_current_cat {
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(43, 87, 151))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(180, 200, 220))
        };

        let icon = if is_current_cat { ">" } else { " " };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!(" {icon} "), cat_style),
            Span::styled(*label, cat_style),
        ])));
    }

    items.push(ListItem::new(Line::from("")));
    items.push(ListItem::new(Line::from(Span::styled(
        " Keys",
        Style::default().fg(Color::Rgb(120, 140, 160)),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        " q  Quit",
        Style::default().fg(Color::Rgb(120, 140, 160)),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        " r  Refresh",
        Style::default().fg(Color::Rgb(120, 140, 160)),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        " s  Start/Stop",
        Style::default().fg(Color::Rgb(120, 140, 160)),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        " c  Copy TSV",
        Style::default().fg(Color::Rgb(120, 140, 160)),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        " g  Run GC",
        Style::default().fg(Color::Rgb(120, 140, 160)),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        " f  Filter",
        Style::default().fg(Color::Rgb(120, 140, 160)),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        " Tab  Focus",
        Style::default().fg(Color::Rgb(120, 140, 160)),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        " Up/Down  Select",
        Style::default().fg(Color::Rgb(120, 140, 160)),
    ))));
    items.push(ListItem::new(Line::from(Span::styled(
        " Right  Expand",
        Style::default().fg(Color::Rgb(120, 140, 160)),
    ))));

    let border_style = if app.focus_pane == FocusPane::Sidebar {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Rgb(60, 70, 80))
    };
    let sidebar = List::new(items).block(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(border_style)
            .style(Style::default().bg(Color::Rgb(30, 34, 42))),
    );
    f.render_widget(sidebar, area);
}

fn render_content(f: &mut Frame, app: &App, area: Rect) {
    if app.show_help {
        render_help(f, area);
        return;
    }

    match app.selected_view {
        ViewId::StartCenter => render_start_center(f, app, area),
        ViewId::TeleOverview
        | ViewId::TeleMemory
        | ViewId::TeleGc
        | ViewId::TeleClasses
        | ViewId::TeleThreads
        | ViewId::TeleCpuLoad => render_telemetry(f, app, area),
        ViewId::LiveAllObjects | ViewId::LiveRecordedObjects | ViewId::LiveAllocationHotSpots => {
            render_memory(f, app, area)
        }
        ViewId::LiveAllocationCallTree => render_allocation_call_tree(f, app, area),
        ViewId::LiveClassTracker => render_class_tracker(f, app, area),
        ViewId::HeapStart
        | ViewId::HeapClasses
        | ViewId::HeapBiggestObjects
        | ViewId::HeapReferences => render_heap(f, app, area),
        ViewId::CpuCallTree => render_cpu_call_tree(f, app, area),
        ViewId::CpuHotSpots => render_cpu_hotspots(f, app, area),
        ViewId::CpuCallGraph => render_cpu_call_graph_placeholder(f, app, area),
        ViewId::CpuOutliers => render_cpu_outliers(f, app, area),
        ViewId::CpuComplexity => render_cpu_complexity(f, app, area),
        ViewId::CpuTracer => render_cpu_tracer(f, app, area),
        ViewId::ThreadsHistory | ViewId::ThreadsMonitor => render_threads(f, app, area),
        ViewId::ThreadsDumps => render_thread_dumps(f, app, area),
        ViewId::DatabasesJdbc => render_database(f, app, area),
        ViewId::DatabasesJpa | ViewId::DatabasesMongo | ViewId::DatabasesCassandra => {
            render_probe_placeholder(f, app, area)
        }
    }
}

fn render_help(f: &mut Frame, area: Rect) {
    let help_text = vec![
        Line::from(Span::styled(
            "OpenProfiler TUI - Help",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Navigation:"),
        Line::from("  Tab          Toggle focus between left navigation and main window"),
        Line::from("  j/Down       Navigation focus: next view, Main focus: next row"),
        Line::from("  k/Up         Navigation focus: previous view, Main focus: previous row"),
        Line::from("  h/Left       Main focus: collapse selected tree row"),
        Line::from("  l/Right      Main focus: expand selected tree row"),
        Line::from("  1-4          Jump to main view"),
        Line::from(""),
        Line::from("Actions:"),
        Line::from("  r            Refresh current view"),
        Line::from("  s            Start/stop recording"),
        Line::from("  g            Run GC"),
        Line::from("  m            Mark heap baseline"),
        Line::from("  c            Copy selected row"),
        Line::from("  f or /       Enter filter mode"),
        Line::from("  Esc          Exit filter mode"),
        Line::from("  q            Quit"),
        Line::from(""),
        Line::from("Press Esc to close this help."),
    ];
    let paragraph = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Help")
            .style(Style::default().bg(Color::Rgb(30, 30, 40))),
    );
    f.render_widget(paragraph, area);
}

fn render_start_center(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
        ])
        .split(area);

    let title = Paragraph::new(vec![
        Line::from(Span::styled(
            " Start Center",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Quick Attach - select a local HotSpot/OpenJ9 JVM",
            Style::default().fg(Color::Rgb(180, 190, 205)),
        )),
    ])
    .style(Style::default().bg(Color::Rgb(25, 28, 35)));
    f.render_widget(title, chunks[0]);

    let controls = Paragraph::new(Line::from(vec![
        Span::styled(" On this computer ", Style::default().fg(Color::Cyan)),
        Span::styled(" Status: ", Style::default().fg(Color::Gray)),
        Span::styled(
            "All detected JVMs",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  [r=Refresh]", Style::default().fg(Color::DarkGray)),
    ]));
    f.render_widget(controls, chunks[1]);

    if app.jvms.is_empty() {
        render_empty(f, chunks[2], "No local JVMs detected", "Press r to refresh");
    } else {
        let header = Row::new(vec![
            Cell::from("PID").style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Cell::from("Process Name").style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Cell::from("Status").style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        let rows = app.jvms.iter().map(|jvm| {
            let marker = if app.selected_pid == Some(jvm.pid) {
                "> "
            } else {
                "  "
            };
            let style = if app.selected_pid == Some(jvm.pid) {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(43, 87, 151))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(190, 200, 210))
            };
            let status = if jvm.profiled {
                "agent/profiler"
            } else {
                "detected"
            };
            Row::new(vec![
                Cell::from(format!("{marker}{}", jvm.pid)),
                Cell::from(truncate_str(
                    &format!("{}  {}", jvm.display_name, jvm.main_class),
                    72,
                )),
                Cell::from(status),
            ])
            .style(style)
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Min(40),
                Constraint::Length(16),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Quick Attach ")
                .style(Style::default().bg(Color::Rgb(18, 20, 26))),
        );
        f.render_widget(table, chunks[2]);
    }

    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" Up/Down ", Style::default().fg(Color::Yellow)),
            Span::raw("select JVM   "),
            Span::styled(" Enter ", Style::default().fg(Color::Green)),
            Span::raw("start CPU recording and open Telemetries   "),
            Span::styled(" r ", Style::default().fg(Color::Cyan)),
            Span::raw("refresh"),
        ]),
        Line::from(Span::styled(
            "Selecting a JVM starts CPU recording automatically.",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Actions ")
            .style(Style::default().bg(Color::Rgb(25, 28, 35))),
    );
    f.render_widget(footer, chunks[3]);
}

fn render_telemetry(f: &mut Frame, app: &App, area: Rect) {
    if app.selected_pid.is_none() {
        render_empty(
            f,
            area,
            "No JVM selected",
            "Return to Start Center and select a JVM",
        );
        return;
    }

    let view_label = app.selected_view.label();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" Telemetry: {} ", view_label),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" [r=Refresh]", Style::default().fg(Color::DarkGray)),
    ]));
    f.render_widget(header, chunks[0]);

    if app.telemetry.samples.is_empty() {
        render_empty(
            f,
            chunks[1],
            "No telemetry data yet",
            "Press 'r' to refresh",
        );
        return;
    }

    let samples = &app.telemetry.samples;
    let metrics: Vec<(&str, Box<dyn Fn(&TelemetrySample) -> f64>, Color, bool)> =
        match app.selected_view {
            ViewId::TeleMemory => vec![
                (
                    "Heap Used",
                    Box::new(|s| s.heap_used_mb),
                    Color::Rgb(0, 158, 25),
                    false,
                ),
                (
                    "Heap Committed",
                    Box::new(|s| s.heap_committed_mb),
                    Color::Rgb(80, 190, 80),
                    false,
                ),
            ],
            ViewId::TeleGc => vec![
                (
                    "GC Count",
                    Box::new(|s| s.gc_count),
                    Color::Rgb(0, 175, 0),
                    false,
                ),
                (
                    "GC Time",
                    Box::new(|s| s.gc_time_ms),
                    Color::Rgb(210, 80, 20),
                    false,
                ),
            ],
            ViewId::TeleClasses => vec![(
                "Classes",
                Box::new(|s| s.class_count),
                Color::Rgb(0, 185, 0),
                true,
            )],
            ViewId::TeleThreads => vec![
                (
                    "Threads",
                    Box::new(|s| s.thread_count),
                    Color::Rgb(45, 135, 230),
                    false,
                ),
                (
                    "Handles",
                    Box::new(|s| s.handle_count),
                    Color::Rgb(80, 120, 180),
                    false,
                ),
            ],
            ViewId::TeleCpuLoad => vec![(
                "CPU %",
                Box::new(|s| s.cpu_percent),
                Color::Rgb(210, 80, 20),
                false,
            )],
            _ => vec![
                (
                    "Memory MB",
                    Box::new(|s| s.heap_used_mb.max(s.process_mb)),
                    Color::Rgb(0, 185, 10),
                    true,
                ),
                (
                    "Heap Used",
                    Box::new(|s| s.heap_used_mb),
                    Color::Rgb(0, 158, 25),
                    false,
                ),
                (
                    "CPU %",
                    Box::new(|s| s.cpu_percent),
                    Color::Rgb(210, 80, 20),
                    false,
                ),
                (
                    "Threads",
                    Box::new(|s| s.thread_count),
                    Color::Rgb(45, 135, 230),
                    false,
                ),
                (
                    "GC Count",
                    Box::new(|s| s.gc_count),
                    Color::Rgb(0, 175, 0),
                    false,
                ),
            ],
        };

    let row_height = 3;
    let available = chunks[1].height as usize / row_height;
    let count = available.min(metrics.len());

    let mut lines = Vec::new();
    for i in 0..count {
        let (label, extract, color, _filled) = &metrics[i];
        if let Some(last) = samples.back() {
            let value = extract(last);
            let mut sparkline_chars = String::new();
            let bar_width = (chunks[1].width as usize).saturating_sub(24);
            let max_val = samples
                .iter()
                .map(|s| extract(s))
                .fold(0.0f64, f64::max)
                .max(0.01);
            let step = (samples.len() as f64 / bar_width as f64).max(1.0);
            for i in 0..bar_width {
                let idx = (i as f64 * step) as usize;
                if idx < samples.len() {
                    let v = extract(&samples[idx]);
                    sparkline_chars.push(spark_char(v / max_val));
                }
            }
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {:>14} ", label),
                    Style::default().fg(*color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:>8.1} ", value),
                    Style::default().fg(Color::White),
                ),
                Span::styled(sparkline_chars, Style::default().fg(*color)),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines).style(Style::default().bg(Color::Rgb(18, 20, 26)));
    f.render_widget(paragraph, chunks[1]);
}

fn render_memory(f: &mut Frame, app: &App, area: Rect) {
    if app.selected_pid.is_none() {
        render_empty(
            f,
            area,
            "No JVM selected",
            "Return to Start Center and select a JVM",
        );
        return;
    }
    if app.selected_view == ViewId::LiveAllocationHotSpots {
        render_allocation_hotspots(f, app, area);
        return;
    }
    if app.memory.rows.is_empty() {
        render_empty(f, area, "No class histogram loaded", "Press 'r' to refresh");
        return;
    }

    let rows = app.filtered_memory_rows();
    let mut sorted = rows;
    match app.memory.sort_column {
        0 => sorted.sort_by(|a, b| {
            if app.memory.sort_asc {
                a.name.cmp(&b.name)
            } else {
                b.name.cmp(&a.name)
            }
        }),
        1 => sorted.sort_by(|a, b| {
            if app.memory.sort_asc {
                a.instances.cmp(&b.instances)
            } else {
                b.instances.cmp(&a.instances)
            }
        }),
        _ => sorted.sort_by(|a, b| {
            if app.memory.sort_asc {
                a.bytes.cmp(&b.bytes)
            } else {
                b.bytes.cmp(&b.bytes)
            }
        }),
    }

    let is_delta = app.selected_view == ViewId::LiveRecordedObjects;
    let filtered_sorted: Vec<MemoryClassRow> = if is_delta {
        sorted
            .into_iter()
            .filter(|r| r.delta_instances > 0 || r.delta_bytes > 0)
            .collect()
    } else {
        sorted
    };

    let header_cells = if is_delta {
        vec!["Name", "New Instances", "New Size"]
    } else {
        vec!["Name", "Instances", "Size"]
    };
    let header = Row::new(header_cells.iter().map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    }));

    let data_rows: Vec<Row> = filtered_sorted
        .iter()
        .enumerate()
        .skip(app.memory.row_offset)
        .take(area.height.saturating_sub(3) as usize)
        .map(|(idx, r)| {
            let row_style =
                if app.focus_pane == FocusPane::Main && idx == app.memory.selected_row_idx {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(43, 87, 151))
                } else {
                    Style::default()
                };
            if is_delta {
                Row::new(vec![
                    Cell::from(truncate_str(&r.name, 60)),
                    Cell::from(format_signed_count(r.delta_instances))
                        .style(Style::default().fg(Color::Yellow)),
                    Cell::from(format_signed_bytes(r.delta_bytes))
                        .style(Style::default().fg(Color::Rgb(200, 100, 0))),
                ])
                .style(row_style)
            } else {
                let max_inst = filtered_sorted
                    .iter()
                    .map(|r| r.instances)
                    .max()
                    .unwrap_or(1);
                let bar_len = (r.instances as f64 / max_inst as f64 * 30.0) as usize;
                let bar: String = "█".repeat(bar_len);
                Row::new(vec![
                    Cell::from(truncate_str(&r.name, 60)),
                    Cell::from(format!(
                        "{} {} ({:.0}%) ",
                        bar,
                        format_count(r.instances),
                        r.instances as f64
                            / filtered_sorted.iter().map(|r| r.instances).sum::<u64>() as f64
                            * 100.0
                    )),
                    Cell::from(format_bytes(r.bytes)),
                ])
                .style(row_style)
            }
        })
        .collect();

    let table = Table::new(
        data_rows,
        [
            Constraint::Min(40),
            Constraint::Min(30),
            Constraint::Min(12),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Live Memory ({}) ", app.selected_view.label()))
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(table, area);
}

fn render_allocation_hotspots(f: &mut Frame, app: &App, area: Rect) {
    let rows = app.visible_allocation_hotspot_rows();
    if rows.is_empty() {
        render_empty(
            f,
            area,
            "No allocation hot spots loaded",
            "Start recording, execute the workload, then press r",
        );
        return;
    }

    let total_bytes = rows
        .iter()
        .filter(|row| row.parent_id.is_none())
        .map(|row| row.bytes)
        .sum::<u64>()
        .max(1);
    let max_bytes = rows.iter().map(|row| row.bytes).max().unwrap_or(1);

    let header = Row::new(vec![
        Cell::from("Hot Spot").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Self Allocated Memory").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Allocations").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let all_rows = app.allocation_hotspot_rows();
    let data_rows = rows
        .iter()
        .enumerate()
        .skip(app.memory.row_offset)
        .take(area.height.saturating_sub(3) as usize)
        .map(|(idx, row)| {
            let bar_len = (row.bytes as f64 / max_bytes as f64 * 20.0) as usize;
            let percent = row.bytes as f64 / total_bytes as f64 * 100.0;
            let has_children = all_rows
                .iter()
                .any(|child| child.parent_id == Some(row.node_id));
            let marker = if has_children {
                if app.memory.expanded_hotspots.contains(&row.node_id) {
                    "v "
                } else {
                    "> "
                }
            } else {
                "  "
            };
            let prefix = if row.depth == 0 {
                marker.to_string()
            } else {
                format!("{}{}", "  ".repeat(row.depth.min(8)), marker)
            };
            let row_style =
                if app.focus_pane == FocusPane::Main && idx == app.memory.selected_row_idx {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(43, 87, 151))
                } else {
                    Style::default()
                };
            Row::new(vec![
                Cell::from(truncate_str(&format!("{prefix}{}", row.name), 72)),
                Cell::from(format!(
                    "{} {} ({percent:.0}%)",
                    "█".repeat(bar_len.min(20)),
                    format_bytes(row.bytes),
                ))
                .style(Style::default().fg(Color::Rgb(155, 0, 0))),
                Cell::from(format_count(row.allocations)),
            ])
            .style(row_style)
        });

    let subtitle = if !app.memory.allocation_hotspots.is_empty() {
        "allocation sites with callers"
    } else if app.memory.marked_heap.is_some() {
        "since marked baseline"
    } else {
        "class histogram fallback; press r after recording"
    };
    let title = if app.focus_pane == FocusPane::Main {
        format!(" Allocation Hot Spots ({subtitle})  [Main: Up/Down PgUp/PgDn=select Enter/Right=expand Left=collapse c=copy TSV Tab=nav] ")
    } else {
        format!(" Allocation Hot Spots ({subtitle})  [Tab focuses this table] ")
    };
    let border_style = if app.focus_pane == FocusPane::Main {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Rgb(60, 70, 80))
    };
    let table = Table::new(
        data_rows,
        [
            Constraint::Min(42),
            Constraint::Length(34),
            Constraint::Length(14),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style)
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(table, area);
}

fn render_allocation_call_tree(f: &mut Frame, app: &App, area: Rect) {
    let rows = app.allocation_hotspot_rows();
    if rows.is_empty() {
        render_empty(
            f,
            area,
            "No allocation call tree has been recorded",
            "Start recording, execute the workload, then press r",
        );
        return;
    }
    let mut lines = Vec::new();
    for row in rows.iter().take(80) {
        let prefix = if row.depth == 0 {
            String::new()
        } else {
            format!("{}- ", "  ".repeat(row.depth.min(8)))
        };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::raw(format!("{} allocations  ", format_count(row.allocations))),
            Span::styled(&row.name, Style::default().fg(Color::Rgb(180, 200, 220))),
            Span::styled(
                format!(" {}", format_bytes(row.bytes)),
                Style::default().fg(Color::Rgb(200, 100, 0)),
            ),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from("No allocation data recorded yet"));
    }
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Allocation Call Tree ")
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(paragraph, area);
}

fn render_class_tracker(f: &mut Frame, app: &App, area: Rect) {
    if app.memory.rows.is_empty() {
        render_empty(f, area, "No class data", "Press 'r' to refresh All Objects");
        return;
    }
    let rows = app.filtered_memory_rows();
    let Some(tracked) = rows.first().or_else(|| app.memory.rows.first()) else {
        return;
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Class Tracker: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(&tracked.name, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![Span::raw(format!(
            "Instances: {}   Size: {}",
            format_count(tracked.instances),
            format_bytes(tracked.bytes)
        ))]),
    ];
    if !app.telemetry.samples.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Instance trend:",
            Style::default().fg(Color::Green),
        )));
        for sample in app.telemetry.samples.iter().rev().take(10) {
            let bar_len = (sample.class_count / 10.0) as usize;
            let bar: String = "█".repeat(bar_len.min(50));
            lines.push(Line::from(format!(
                "  {:.0}s {} ({:.0})",
                sample.elapsed_secs, bar, sample.class_count
            )));
        }
    }
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Class Tracker ")
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(paragraph, area);
}

fn render_heap(f: &mut Frame, app: &App, area: Rect) {
    match app.selected_view {
        ViewId::HeapStart => {
            let lines = vec![
                Line::from(Span::styled(
                    "Heap Walker",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("No snapshot has been taken."),
                Line::from(""),
                Line::from("Press 'm' to mark the current heap baseline"),
                Line::from(Span::styled(
                    "(baseline comparison uses the live class histogram)",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let paragraph = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Heap Walker ")
                    .style(Style::default().bg(Color::Rgb(18, 20, 26))),
            );
            f.render_widget(paragraph, area);
        }
        ViewId::HeapClasses | ViewId::HeapBiggestObjects => {
            let snapshot = app.heap_snapshots.first();
            if snapshot.is_none() || app.memory.rows.is_empty() {
                render_empty(
                    f,
                    area,
                    "No snapshot taken",
                    "Switch to Heap Start and take a snapshot first",
                );
                return;
            }
            let mut rows = app.memory.rows.clone();
            if app.selected_view == ViewId::HeapBiggestObjects {
                rows.sort_by_key(|r| Reverse(r.bytes));
            }
            let header = Row::new(vec![
                Cell::from("Name").style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from("Instances").style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from("Size").style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]);
            let data_rows: Vec<Row> = rows
                .iter()
                .take(area.height as usize / 2)
                .map(|r| {
                    Row::new(vec![
                        Cell::from(truncate_str(&r.name, 60)),
                        Cell::from(format_count(r.instances)),
                        Cell::from(format_bytes(r.bytes)),
                    ])
                })
                .collect();
            let table = Table::new(
                data_rows,
                [
                    Constraint::Min(40),
                    Constraint::Length(15),
                    Constraint::Length(12),
                ],
            )
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Heap {} ", app.selected_view.label()))
                    .style(Style::default().bg(Color::Rgb(18, 20, 26))),
            );
            f.render_widget(table, area);
        }
        ViewId::HeapReferences => {
            render_empty(
                f,
                area,
                "Object references not yet implemented",
                "Heap Walker references view",
            );
        }
        _ => {}
    }
}

fn render_cpu_call_tree(f: &mut Frame, app: &App, area: Rect) {
    if app.cpu.call_tree.nodes.len() <= 1 {
        render_empty(
            f,
            area,
            "No CPU call tree data",
            "Press 's' to start recording",
        );
        return;
    }
    let tree = &app.cpu.call_tree;
    let Some(root) = tree.nodes.first() else {
        return;
    };
    let max_total = root.total_duration_ms.max(1.0);

    let mut lines = Vec::new();
    fn draw_node<'a>(
        nodes: &'a [openprofiler_core::model::CallTreeNode],
        idx: usize,
        depth: usize,
        max_total: f64,
        lines: &mut Vec<Line<'a>>,
    ) {
        let node = &nodes[idx];
        let pct = node.total_duration_ms / max_total * 100.0;
        let indent = "  ".repeat(depth);
        let bar_len = (pct / 100.0 * 20.0) as usize;
        let bar = "█".repeat(bar_len.min(20));
        lines.push(Line::from(vec![
            Span::raw(indent),
            Span::styled(
                if !node.children.is_empty() {
                    "v "
                } else {
                    "  "
                },
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:.1}% ", pct),
                Style::default().fg(Color::Rgb(155, 0, 0)),
            ),
            Span::styled(
                format!("{} ", bar),
                Style::default().fg(Color::Rgb(155, 0, 0)),
            ),
            Span::styled(
                format!("{} ", format_duration_ms(node.total_duration_ms)),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                truncate_str(&node.full_name(), 80),
                Style::default().fg(Color::White),
            ),
        ]));
        for child_idx in &node.children {
            draw_node(nodes, *child_idx, depth + 1, max_total, lines);
        }
    }

    for child_idx in &root.children {
        draw_node(&tree.nodes, *child_idx, 0, max_total, &mut lines);
    }

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" CPU Call Tree ")
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(paragraph, area);
}

fn render_cpu_hotspots(f: &mut Frame, app: &App, area: Rect) {
    if app.cpu.methods.is_empty() {
        let hint = if app.cpu.recording {
            "Recording is active. Fetching CPU Hot Spots..."
        } else {
            "Press 's' to start recording"
        };
        render_empty(f, area, "No CPU data recorded", hint);
        return;
    }
    let rows = app.filtered_cpu_rows();
    if rows.is_empty() {
        render_empty(
            f,
            area,
            "No CPU method hot spots recorded",
            "SQL hot spots are shown in Databases",
        );
        return;
    }
    let max_pct = rows.iter().map(|r| r.percent).fold(0.01f32, f32::max);

    let header = Row::new(vec![
        Cell::from("Hot Spot").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Self Time").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Avg Time").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Events").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let mut visible_rows: Vec<Row> = Vec::new();
    let mut visible_idx = 0usize;
    let max_rows = area.height.saturating_sub(3) as usize;
    for r in rows.iter() {
        let is_selected = visible_idx == app.cpu.selected_hotspot_idx;
        let children = r
            .method_id
            .map(|id| app.hotspot_children(id))
            .unwrap_or_default();
        let expanded = r
            .method_id
            .is_some_and(|id| app.cpu.expanded_hotspots.contains(&id));
        let marker = if children.is_empty() {
            "  "
        } else if expanded {
            "v "
        } else {
            "> "
        };
        let row_style = if is_selected {
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(43, 87, 151))
        } else {
            Style::default()
        };
        let bar_len = (r.percent / max_pct * 20.0) as usize;
        let bar = "█".repeat(bar_len.min(20));
        if visible_idx >= app.cpu.row_offset && visible_rows.len() < max_rows {
            visible_rows.push(
                Row::new(vec![
                    Cell::from(truncate_str(&format!("{marker}{}", r.method), 72)),
                    Cell::from(format!(
                        "{} {:.1}% {}",
                        bar,
                        r.percent * 100.0,
                        format_duration_nano_f(r.self_ms * 1_000_000.0)
                    ))
                    .style(Style::default().fg(Color::Rgb(155, 0, 0))),
                    Cell::from(format_duration_nano_f(r.average_nanos))
                        .style(Style::default().fg(Color::Rgb(180, 180, 180))),
                    Cell::from(format_count(r.invocations)),
                ])
                .style(row_style),
            );
        }
        visible_idx += 1;

        if expanded {
            for (edge, child) in children.into_iter().take(12) {
                let is_selected = visible_idx == app.cpu.selected_hotspot_idx;
                let edge_percent = if r.total_ms > 0.0 {
                    (edge.total_duration_nano as f64 / (r.total_ms * 1_000_000.0) * 100.0)
                        .clamp(0.0, 999.9)
                } else {
                    0.0
                };
                let avg_nanos = if edge.call_count > 0 {
                    edge.total_duration_nano as f64 / edge.call_count as f64
                } else {
                    0.0
                };
                let child_style = if is_selected {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(43, 87, 151))
                } else {
                    Style::default().fg(Color::Rgb(210, 215, 220))
                };
                if visible_idx >= app.cpu.row_offset && visible_rows.len() < max_rows {
                    visible_rows.push(
                        Row::new(vec![
                            Cell::from(truncate_str(
                                &format!(
                                    "  - {:.1}% - {} - {} hot spot inv. {}",
                                    edge_percent,
                                    format_duration_nano_f(edge.total_duration_nano as f64),
                                    format_count(edge.call_count),
                                    child.method
                                ),
                                82,
                            )),
                            Cell::from(""),
                            Cell::from(format_duration_nano_f(avg_nanos))
                                .style(Style::default().fg(Color::Rgb(180, 180, 180))),
                            Cell::from(format_count(edge.call_count)),
                        ])
                        .style(child_style),
                    );
                }
                visible_idx += 1;
            }
        }
        if visible_rows.len() >= max_rows && visible_idx > app.cpu.selected_hotspot_idx {
            break;
        }
    }

    let title = if app.focus_pane == FocusPane::Main {
        " CPU Hot Spots  [Main: Up/Down PgUp/PgDn=select Enter/Right=expand Left=collapse c=copy TSV Tab=nav] "
    } else {
        " CPU Hot Spots  [Tab focuses this table; Nav: Up/Down switches views] "
    };
    let border_style = if app.focus_pane == FocusPane::Main {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Rgb(60, 70, 80))
    };
    let table = Table::new(
        visible_rows,
        [
            Constraint::Min(42),
            Constraint::Length(34),
            Constraint::Length(12),
            Constraint::Length(9),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style)
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(table, area);
}

fn render_cpu_call_graph_placeholder(f: &mut Frame, _app: &App, area: Rect) {
    render_empty(
        f,
        area,
        "Call Graph",
        "Not available in TUI (requires graphical rendering)",
    );
}

fn render_cpu_outliers(f: &mut Frame, app: &App, area: Rect) {
    if app.cpu.methods.is_empty() {
        render_empty(
            f,
            area,
            "No CPU data recorded",
            "Press 's' to start recording",
        );
        return;
    }
    let rows = app.filtered_cpu_rows();
    let mut lines = Vec::new();
    for row in rows.iter().filter(|r| r.percent > 0.05).take(60) {
        lines.push(Line::from(vec![
            Span::styled("slow ", Style::default().fg(Color::Rgb(170, 70, 0))),
            Span::styled(
                format!("{:.1}% ", row.percent * 100.0),
                Style::default().fg(Color::Rgb(155, 0, 0)),
            ),
            Span::styled(
                truncate_str(&row.method, 80),
                Style::default().fg(Color::White),
            ),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from("No outliers detected"));
    }
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Outlier Detection ")
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(paragraph, area);
}

fn render_cpu_complexity(f: &mut Frame, app: &App, area: Rect) {
    if app.cpu.methods.is_empty() {
        render_empty(
            f,
            area,
            "No CPU data recorded",
            "Press 's' to start recording",
        );
        return;
    }
    let rows = app.filtered_cpu_rows();
    let mut lines = vec![
        Line::from(Span::styled(
            "Complexity Analysis",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for row in rows.iter().take(40) {
        let assessment = if row.total_samples > 10 {
            "O(n) suspected"
        } else {
            "stable"
        };
        lines.push(Line::from(vec![
            Span::styled(
                truncate_str(&row.method, 70),
                Style::default().fg(Color::White),
            ),
            Span::raw("  "),
            Span::styled(
                assessment,
                Style::default().fg(if row.total_samples > 10 {
                    Color::Red
                } else {
                    Color::Green
                }),
            ),
            Span::raw(format!(" ({} samples)", row.total_samples)),
        ]));
    }
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Complexity ")
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(paragraph, area);
}

fn render_cpu_tracer(f: &mut Frame, app: &App, area: Rect) {
    if app.cpu.samples.is_empty() {
        render_empty(
            f,
            area,
            "No call trace samples recorded",
            "Press 's' to start recording",
        );
        return;
    }
    let mut lines = Vec::new();
    for sample in app.cpu.samples.iter().rev().take(40) {
        lines.push(Line::from(vec![
            Span::styled(&sample.captured_at, Style::default().fg(Color::DarkGray)),
            Span::raw(" "),
            Span::styled(&sample.thread, Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            Span::styled(
                &sample.state,
                Style::default().fg(state_color(&sample.state)),
            ),
        ]));
        for ste in sample.stack_trace.iter().take(8) {
            lines.push(Line::from(vec![
                Span::raw("    at "),
                Span::styled(ste.full_name(), Style::default().fg(Color::White)),
                Span::raw(" "),
                Span::styled(ste.location(), Style::default().fg(Color::DarkGray)),
            ]));
        }
        lines.push(Line::from(""));
    }
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Call Tracer ")
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(paragraph, area);
}

fn render_threads(f: &mut Frame, app: &App, area: Rect) {
    let threads = app
        .threads
        .dumps
        .last()
        .map(|d| d.threads.clone())
        .unwrap_or_default();
    if threads.is_empty() {
        render_empty(
            f,
            area,
            "No thread data",
            "Press 'r' to capture thread dump",
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(area);

    if !app.threads.history.is_empty() {
        let history_lines = render_thread_history_lines(&app.threads.history);
        let history_paragraph = Paragraph::new(history_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Thread History ")
                .style(Style::default().bg(Color::Rgb(18, 20, 26))),
        );
        f.render_widget(history_paragraph, chunks[0]);
    }

    let filter = app.filter_input.to_lowercase();
    let header = Row::new(vec![
        Cell::from("Name").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("State").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("NID").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Top Frame").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let data_rows: Vec<Row> = threads
        .iter()
        .filter(|t| {
            filter.is_empty()
                || t.name.to_lowercase().contains(&filter)
                || t.top_frame.to_lowercase().contains(&filter)
        })
        .take(chunks[1].height as usize / 2)
        .map(|t| {
            Row::new(vec![
                Cell::from(truncate_str(&t.name, 30)),
                Cell::from(t.state.clone()).style(Style::default().fg(state_color(&t.state))),
                Cell::from(t.nid.clone()),
                Cell::from(truncate_str(&t.top_frame, 50))
                    .style(Style::default().fg(Color::Rgb(180, 200, 220))),
            ])
        })
        .collect();

    let table = Table::new(
        data_rows,
        [
            Constraint::Min(30),
            Constraint::Length(16),
            Constraint::Length(8),
            Constraint::Min(30),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Threads ({}) ", app.selected_view.label()))
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(table, chunks[1]);
}

fn render_thread_dumps(f: &mut Frame, app: &App, area: Rect) {
    if app.threads.dumps.is_empty() {
        render_empty(
            f,
            area,
            "No thread dumps captured",
            "Press 'r' to capture thread dump",
        );
        return;
    }
    let idx = app
        .threads
        .selected_dump
        .unwrap_or(app.threads.dumps.len().saturating_sub(1));
    let dump = &app.threads.dumps[idx];
    let lines: Vec<Line> = dump
        .raw
        .lines()
        .take(area.height as usize - 2)
        .map(|line| {
            Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(Color::Rgb(180, 200, 220)),
            ))
        })
        .collect();
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " Thread Dump #{} - {} threads ",
                idx + 1,
                dump.threads.len()
            ))
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(paragraph, area);
}

fn render_database(f: &mut Frame, app: &App, area: Rect) {
    if app.cpu.methods.is_empty() && app.database.events.is_empty() {
        render_empty(
            f,
            area,
            "No database hot spots recorded",
            "Start CPU recording and refresh after JDBC traffic is executed",
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Hot Spot").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Time").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Avg Time").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Events").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let filter = app.filter_input.to_lowercase();
    let sql_rows = app.filtered_sql_hotspot_rows();
    if !sql_rows.is_empty() {
        let max_ms = sql_rows
            .iter()
            .map(|row| row.total_ms)
            .fold(0.01f64, f64::max);
        let data_rows = sql_rows
            .iter()
            .enumerate()
            .skip(app.database.row_offset)
            .take(area.height.saturating_sub(3) as usize)
            .map(|(idx, row)| {
                let bar_len = (row.total_ms / max_ms * 20.0) as usize;
                let row_style =
                    if app.focus_pane == FocusPane::Main && idx == app.database.selected_row_idx {
                        Style::default()
                            .fg(Color::White)
                            .bg(Color::Rgb(43, 87, 151))
                    } else {
                        Style::default()
                    };
                Row::new(vec![
                    Cell::from(truncate_str(&row.sql, 72)),
                    Cell::from(format!(
                        "{} {}",
                        "█".repeat(bar_len.min(20)),
                        format_duration_nano_f(row.total_ms * 1_000_000.0)
                    ))
                    .style(Style::default().fg(Color::Rgb(155, 0, 0))),
                    Cell::from(format_duration_nano_f(row.average_ms * 1_000_000.0)),
                    Cell::from(format_count(row.events)),
                ])
                .style(row_style)
            });

        let title = if app.focus_pane == FocusPane::Main {
            " Database Hot Spots  [Main: Up/Down PgUp/PgDn=select c=copy TSV Tab=nav] "
        } else {
            " Database Hot Spots  [Tab focuses this table; Nav: Up/Down switches views] "
        };
        let border_style = if app.focus_pane == FocusPane::Main {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Rgb(60, 70, 80))
        };
        let table = Table::new(
            data_rows,
            [
                Constraint::Min(42),
                Constraint::Length(34),
                Constraint::Length(12),
                Constraint::Length(10),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style)
                .style(Style::default().bg(Color::Rgb(18, 20, 26))),
        );
        f.render_widget(table, area);
        return;
    }

    let mut rows: Vec<_> = app
        .cpu
        .methods
        .iter()
        .filter(|row| is_database_method(row))
        .filter(|row| {
            let label = database_method_label(row).to_lowercase();
            filter.is_empty() || label.contains(&filter)
        })
        .cloned()
        .collect();
    rows.sort_by(|a, b| {
        b.self_ms
            .partial_cmp(&a.self_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let max_ms = rows.iter().map(|row| row.self_ms).fold(0.01f64, f64::max);
    if rows.is_empty() {
        render_empty(
            f,
            area,
            "No SQL hot spots recorded",
            "Database shows SQL statements only",
        );
        return;
    }

    let data_rows = rows
        .iter()
        .enumerate()
        .skip(app.database.row_offset)
        .take(area.height.saturating_sub(3) as usize)
        .map(|(idx, row)| {
            let bar_len = (row.self_ms / max_ms * 20.0) as usize;
            let row_style =
                if app.focus_pane == FocusPane::Main && idx == app.database.selected_row_idx {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(43, 87, 151))
                } else {
                    Style::default()
                };
            Row::new(vec![
                Cell::from(truncate_str(&database_method_label(row), 72)),
                Cell::from(format!(
                    "{} {:.1}% {}",
                    "█".repeat(bar_len.min(20)),
                    row.percent * 100.0,
                    format_duration_nano_f(row.self_ms * 1_000_000.0)
                ))
                .style(Style::default().fg(Color::Rgb(155, 0, 0))),
                Cell::from(format_duration_nano_f(row.average_nanos)),
                Cell::from(format_count(row.invocations)),
            ])
            .style(row_style)
        });

    let title = if app.focus_pane == FocusPane::Main {
        " Database Hot Spots  [Main: Up/Down PgUp/PgDn=select c=copy TSV Tab=nav] "
    } else {
        " Database Hot Spots  [Tab focuses this table; Nav: Up/Down switches views] "
    };
    let border_style = if app.focus_pane == FocusPane::Main {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Rgb(60, 70, 80))
    };
    let table = Table::new(
        data_rows,
        [
            Constraint::Min(42),
            Constraint::Length(34),
            Constraint::Length(12),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style)
            .style(Style::default().bg(Color::Rgb(18, 20, 26))),
    );
    f.render_widget(table, area);
}

fn render_probe_placeholder(f: &mut Frame, app: &App, area: Rect) {
    let label = app.selected_view.label();
    render_empty(
        f,
        area,
        &format!("No data for {label} probe"),
        "Press 's' to start recording",
    );
}

fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let status = if app.selected_pid.is_some() {
        "Profiling"
    } else {
        "Disconnected"
    };
    let recording = app.active_recording_count();
    let elapsed = app.elapsed_secs();
    let pid_str = app
        .selected_pid
        .map_or("VM # -".to_string(), |p| format!("VM #{p}"));
    let now = chrono::Local::now().format("%H:%M:%S");
    let mode = match app.input_mode {
        super::event::InputMode::Normal => "NORMAL",
        super::event::InputMode::Filter => "FILTER",
    };
    let last_log = app
        .logs
        .front()
        .map(|log| format!(" | {}: {}", log.action, truncate_str(&log.message, 40)))
        .unwrap_or_default();

    let spans = vec![
        Span::styled(
            format!(" {status}"),
            Style::default()
                .fg(if app.selected_pid.is_some() {
                    Color::Green
                } else {
                    Color::Red
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("{recording} rec"),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(" | "),
        Span::styled(&pid_str, Style::default().fg(Color::Cyan)),
        Span::raw(" | "),
        Span::raw(format!(
            "{:02}:{:02}",
            (elapsed as u64) / 60,
            (elapsed as u64) % 60
        )),
        Span::raw(" | "),
        Span::styled(app.selected_view.label(), Style::default().fg(Color::White)),
        if !app.filter_input.is_empty() {
            Span::styled(
                format!(" | Filter: {}", app.filter_input),
                Style::default().fg(Color::Yellow),
            )
        } else {
            Span::raw("")
        },
        Span::styled(format!(" [{mode}]"), Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {now}")),
        Span::styled(last_log, Style::default().fg(Color::Rgb(180, 190, 210))),
    ];
    let status_bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Rgb(30, 34, 42)).fg(Color::White));
    f.render_widget(status_bar, area);
}

fn render_empty(f: &mut Frame, area: Rect, title: &str, message: &str) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {title}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("  {message}"),
            Style::default().fg(Color::Rgb(160, 170, 180)),
        )),
    ];
    let paragraph = Paragraph::new(lines).style(Style::default().bg(Color::Rgb(18, 20, 26)));
    f.render_widget(paragraph, area);
}

fn spark_char(value: f64) -> char {
    let v = value.clamp(0.0, 1.0);
    match (v * 8.0) as u8 {
        0 => ' ',
        1 => '\u{2581}',
        2 => '\u{2582}',
        3 => '\u{2583}',
        4 => '\u{2584}',
        5 => '\u{2585}',
        6 => '\u{2586}',
        7 => '\u{2587}',
        _ => '\u{2588}',
    }
}

fn state_color(state: &str) -> Color {
    match state {
        "RUNNABLE" => Color::Rgb(0, 150, 0),
        "WAITING" => Color::Rgb(50, 115, 220),
        "TIMED_WAITING" => Color::Rgb(210, 140, 20),
        "BLOCKED" => Color::Rgb(190, 40, 30),
        _ => Color::Gray,
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

fn render_thread_history_lines(history: &VecDeque<ThreadSummary>) -> Vec<Line<'static>> {
    if history.len() < 2 {
        return vec![Line::from("  Waiting for data...")];
    }
    let max_total = history
        .iter()
        .map(|h| h.runnable + h.waiting + h.timed_waiting + h.blocked + h.other)
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    let width = 60;
    let step = (history.len() as f64 / width as f64).max(1.0);
    let mut lines = Vec::new();
    let mut runnable_bars = String::new();
    let mut waiting_bars = String::new();
    for i in 0..width {
        let idx = (i as f64 * step) as usize;
        if idx < history.len() {
            let h = &history[idx];
            runnable_bars.push(spark_char(h.runnable as f64 / max_total));
            waiting_bars.push(spark_char(h.waiting as f64 / max_total));
        }
    }
    lines.push(Line::from(vec![
        Span::styled("Runnable: ", Style::default().fg(Color::Green)),
        Span::styled(runnable_bars, Style::default().fg(Color::Green)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Waiting:  ", Style::default().fg(Color::Blue)),
        Span::styled(waiting_bars, Style::default().fg(Color::Blue)),
    ]));
    if let Some(last) = history.back() {
        lines.push(Line::from(format!(
            "  R:{} W:{} TW:{} B:{}",
            last.runnable, last.waiting, last.timed_waiting, last.blocked
        )));
    }
    lines
}
