use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::call_tree::{build_call_tree, build_hot_spots, CallTreeConfig};
use super::event::InputMode;
use super::format::{format_bytes, format_duration_nano_f};
use openprofiler_core::jvm::*;
use openprofiler_core::model::*;
use openprofiler_core::protocol::{allocation_hotspots_from_agent_data, cpu_rows_from_agent_data};

const DEFAULT_PAGE_ROWS: usize = 20;
const MAX_THREAD_DUMPS: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Sidebar,
    Main,
}

pub struct App {
    pub selected_view: ViewId,
    pub expanded: Vec<Category>,
    pub selected_category_idx: usize,
    pub selected_item_idx: usize,
    pub list_offset: usize,
    pub jvms: Vec<JvmTarget>,
    pub selected_pid: Option<u32>,
    pub agent_port: u16,
    pub agent_enabled: bool,
    pub session_started_at: Instant,
    pub last_tick: Instant,
    pub last_thread_refresh: Instant,
    pub auto_update_secs: u64,
    pub logs: VecDeque<OperationLog>,
    pub input_mode: InputMode,
    pub focus_pane: FocusPane,
    pub filter_input: String,
    pub telemetry: TelemetryState,
    pub memory: MemoryState,
    pub cpu: CpuState,
    pub threads: ThreadState,
    pub database: DatabaseState,
    pub heap_snapshots: Vec<HeapSnapshotInfo>,
    pub heap_selected_snapshot: Option<usize>,
    pub telemetry_query: Option<QueryHandle>,
    pub memory_query: Option<QueryHandle>,
    pub thread_query: Option<QueryHandle>,
    pub jvms_query: Option<QueryHandle>,
    pub pending_queries: Vec<QueryHandle>,
    pub show_help: bool,
    pending_initial_cpu_start: bool,
    cpu_recording_start_pending: bool,
    cpu_recording_stop_pending: bool,
}

pub struct TelemetryState {
    pub samples: VecDeque<TelemetrySample>,
    pub last_process_cpu: Option<(f64, Instant)>,
}

pub struct MemoryState {
    pub rows: Vec<MemoryClassRow>,
    pub allocation_hotspots: Vec<AllocationHotSpotRow>,
    pub frozen: bool,
    pub recording: bool,
    pub previous_histogram: BTreeMap<String, (u64, u64)>,
    pub marked_heap: Option<BTreeMap<String, (u64, u64)>>,
    pub selected_row_idx: usize,
    pub sort_column: u8,
    pub sort_asc: bool,
    pub loaded_once: bool,
    pub expanded_hotspots: HashSet<i32>,
    pub row_offset: usize,
}

pub struct CpuState {
    pub recording: bool,
    pub thread_status: String,
    pub aggregation: String,
    pub samples: VecDeque<StackSample>,
    pub methods: Vec<CpuMethodRow>,
    pub method_edges: Vec<CpuMethodEdgeRow>,
    pub expanded_hotspots: HashSet<i32>,
    pub selected_hotspot_idx: usize,
    pub row_offset: usize,
    pub call_tree: CallTree,
    pub sort_column: u8,
    pub sort_asc: bool,
    pub sampling_interval_ms: f64,
    pub agent_port: u16,
}

pub struct ThreadState {
    pub dumps: Vec<ThreadDump>,
    pub selected_dump: Option<usize>,
    pub history: VecDeque<ThreadSummary>,
}

pub struct DatabaseState {
    pub recording: bool,
    pub events: VecDeque<DatabaseEvent>,
    pub selected_row_idx: usize,
    pub row_offset: usize,
}

impl App {
    pub fn new() -> Self {
        let expanded = vec![
            Category::Telemetries,
            Category::LiveMemory,
            Category::CpuViews,
            Category::Databases,
        ];
        Self {
            selected_view: ViewId::StartCenter,
            expanded,
            selected_category_idx: 0,
            selected_item_idx: 0,
            list_offset: 0,
            jvms: Vec::new(),
            selected_pid: None,
            agent_port: 8849,
            agent_enabled: false,
            session_started_at: Instant::now(),
            last_tick: Instant::now() - Duration::from_secs(60),
            last_thread_refresh: Instant::now() - Duration::from_secs(60),
            auto_update_secs: 2,
            logs: VecDeque::new(),
            input_mode: InputMode::Normal,
            focus_pane: FocusPane::Sidebar,
            filter_input: String::new(),
            telemetry: TelemetryState {
                samples: VecDeque::new(),
                last_process_cpu: None,
            },
            memory: MemoryState {
                rows: Vec::new(),
                allocation_hotspots: Vec::new(),
                frozen: false,
                recording: false,
                previous_histogram: BTreeMap::new(),
                marked_heap: None,
                selected_row_idx: 0,
                sort_column: 1,
                sort_asc: false,
                loaded_once: false,
                expanded_hotspots: HashSet::new(),
                row_offset: 0,
            },
            cpu: CpuState {
                recording: false,
                thread_status: "Runnable".to_string(),
                aggregation: "Methods".to_string(),
                samples: VecDeque::new(),
                methods: Vec::new(),
                method_edges: Vec::new(),
                expanded_hotspots: HashSet::new(),
                selected_hotspot_idx: 0,
                row_offset: 0,
                call_tree: CallTree {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                },
                sort_column: 1,
                sort_asc: false,
                sampling_interval_ms: 10.0,
                agent_port: 8849,
            },
            threads: ThreadState {
                dumps: Vec::new(),
                selected_dump: None,
                history: VecDeque::new(),
            },
            database: DatabaseState {
                recording: false,
                events: VecDeque::new(),
                selected_row_idx: 0,
                row_offset: 0,
            },
            heap_snapshots: Vec::new(),
            heap_selected_snapshot: None,
            telemetry_query: None,
            memory_query: None,
            thread_query: None,
            jvms_query: None,
            pending_queries: Vec::new(),
            show_help: false,
            pending_initial_cpu_start: false,
            cpu_recording_start_pending: false,
            cpu_recording_stop_pending: false,
        }
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.session_started_at.elapsed().as_secs_f64()
    }

    pub fn push_log(
        &mut self,
        level: LogLevel,
        action: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.logs.push_front(OperationLog {
            at: chrono::Local::now().format("%H:%M:%S").to_string(),
            level,
            action: action.into(),
            message: message.into(),
        });
        while self.logs.len() > 100 {
            self.logs.pop_back();
        }
    }

    pub fn active_recording_count(&self) -> usize {
        self.cpu.recording as usize
            + self.memory.recording as usize
            + self.database.recording as usize
    }

    fn selected_target(&self) -> Option<&JvmTarget> {
        let pid = self.selected_pid?;
        self.jvms.iter().find(|target| target.pid == pid)
    }

    pub fn current_category_views(&self) -> &'static [ViewId] {
        CATEGORIES[self
            .selected_category_idx
            .min(CATEGORIES.len().saturating_sub(1))]
        .2
    }

    // --- Navigation ---

    pub fn toggle_focus_pane(&mut self) {
        self.focus_pane = match self.focus_pane {
            FocusPane::Sidebar => FocusPane::Main,
            FocusPane::Main => FocusPane::Sidebar,
        };
    }

    pub fn next_category(&mut self) {
        self.selected_category_idx = (self.selected_category_idx + 1) % CATEGORIES.len();
        self.selected_item_idx = 0;
        self.list_offset = 0;
        self.go_to_first_view_in_category();
    }

    pub fn prev_category(&mut self) {
        self.selected_category_idx =
            (self.selected_category_idx + CATEGORIES.len() - 1) % CATEGORIES.len();
        self.selected_item_idx = 0;
        self.list_offset = 0;
        self.go_to_first_view_in_category();
    }

    pub fn navigate_down(&mut self) {
        self.move_selection(1);
    }

    pub fn navigate_up(&mut self) {
        self.move_selection(-1);
    }

    pub fn page_down(&mut self) {
        self.move_selection(20);
    }

    pub fn page_up(&mut self) {
        self.move_selection(-20);
    }

    fn move_selection(&mut self, delta: isize) {
        if self.selected_view == ViewId::StartCenter {
            if delta >= 0 {
                for _ in 0..delta.max(1) {
                    self.next_jvm();
                }
            } else {
                for _ in 0..(-delta).max(1) {
                    self.prev_jvm();
                }
            }
            return;
        }
        if self.focus_pane == FocusPane::Main
            && self.selected_view == ViewId::CpuHotSpots
            && !self.cpu.methods.is_empty()
        {
            let max_idx = self.visible_hotspot_row_count().saturating_sub(1);
            self.cpu.selected_hotspot_idx =
                move_index(self.cpu.selected_hotspot_idx, delta, max_idx);
            keep_visible(
                &mut self.cpu.row_offset,
                self.cpu.selected_hotspot_idx,
                DEFAULT_PAGE_ROWS,
            );
            return;
        }
        if self.focus_pane == FocusPane::Main
            && self.selected_view == ViewId::LiveAllocationHotSpots
        {
            let max_idx = self
                .visible_allocation_hotspot_rows()
                .len()
                .saturating_sub(1);
            self.memory.selected_row_idx = move_index(self.memory.selected_row_idx, delta, max_idx);
            keep_visible(
                &mut self.memory.row_offset,
                self.memory.selected_row_idx,
                DEFAULT_PAGE_ROWS,
            );
            return;
        }
        if self.focus_pane == FocusPane::Main && self.is_memory_view() {
            let max_idx = self.filtered_memory_rows().len().saturating_sub(1);
            self.memory.selected_row_idx = move_index(self.memory.selected_row_idx, delta, max_idx);
            keep_visible(
                &mut self.memory.row_offset,
                self.memory.selected_row_idx,
                DEFAULT_PAGE_ROWS,
            );
            return;
        }
        if self.focus_pane == FocusPane::Main && self.selected_view == ViewId::DatabasesJdbc {
            let max_idx = self.database_row_count().saturating_sub(1);
            self.database.selected_row_idx =
                move_index(self.database.selected_row_idx, delta, max_idx);
            keep_visible(
                &mut self.database.row_offset,
                self.database.selected_row_idx,
                DEFAULT_PAGE_ROWS,
            );
            return;
        }
        if delta < 0 {
            self.navigate_sidebar_up();
        } else {
            self.navigate_sidebar_down();
        }
    }

    fn navigate_sidebar_down(&mut self) {
        let views = self.current_category_views();
        if self.selected_item_idx + 1 < views.len() {
            self.selected_item_idx += 1;
        } else {
            self.next_category();
        }
        let views = self.current_category_views();
        self.selected_view = views[self.selected_item_idx];
    }

    fn navigate_sidebar_up(&mut self) {
        if self.selected_item_idx > 0 {
            self.selected_item_idx -= 1;
        } else {
            self.prev_category();
        }
        let views = self.current_category_views();
        self.selected_view = views[self.selected_item_idx];
    }

    pub fn navigate_left(&mut self) {
        if self.focus_pane == FocusPane::Main
            && self.selected_view == ViewId::CpuHotSpots
            && !self.cpu.methods.is_empty()
        {
            if let Some(method_id) = self.selected_hotspot_method_id() {
                self.cpu.expanded_hotspots.remove(&method_id);
            }
            return;
        }
        if self.focus_pane == FocusPane::Main
            && self.selected_view == ViewId::LiveAllocationHotSpots
        {
            if let Some(node_id) = self.selected_allocation_hotspot_node_id() {
                self.memory.expanded_hotspots.remove(&node_id);
            }
            return;
        }
        self.prev_category();
    }

    pub fn navigate_right(&mut self) {
        if self.focus_pane == FocusPane::Main
            && self.selected_view == ViewId::CpuHotSpots
            && !self.cpu.methods.is_empty()
        {
            self.toggle_selected_hotspot_expanded();
            return;
        }
        if self.focus_pane == FocusPane::Main
            && self.selected_view == ViewId::LiveAllocationHotSpots
        {
            self.toggle_selected_allocation_hotspot_expanded();
            return;
        }
        self.next_category();
    }

    pub fn select_current(&mut self) {
        if self.selected_view == ViewId::StartCenter {
            self.start_selected_jvm_session();
            return;
        }
        if self.focus_pane == FocusPane::Main
            && self.selected_view == ViewId::CpuHotSpots
            && !self.cpu.methods.is_empty()
        {
            self.toggle_selected_hotspot_expanded();
            return;
        }
        if self.focus_pane == FocusPane::Main
            && self.selected_view == ViewId::LiveAllocationHotSpots
        {
            self.toggle_selected_allocation_hotspot_expanded();
            return;
        }
        let views = self.current_category_views();
        if !views.is_empty() {
            self.selected_view = views[self.selected_item_idx];
        }
    }

    pub fn go_to_view(&mut self, view: ViewId) {
        self.selected_view = view;
        for (i, (cat, _, views)) in CATEGORIES.iter().enumerate() {
            if views.contains(&view) {
                self.selected_category_idx = i;
                self.selected_item_idx = views.iter().position(|v| *v == view).unwrap_or(0);
                if !self.expanded.contains(cat) {
                    self.expanded.push(*cat);
                }
                return;
            }
        }
    }

    fn go_to_first_view_in_category(&mut self) {
        let views = self.current_category_views();
        if !views.is_empty() {
            self.selected_view = views[0];
        }
    }

    pub fn enter_filter_mode(&mut self) {
        self.input_mode = InputMode::Filter;
        self.filter_input.clear();
    }

    pub fn next_jvm(&mut self) {
        if self.jvms.is_empty() {
            return;
        }
        let current = self.selected_pid.unwrap_or(0);
        if let Some(idx) = self.jvms.iter().position(|j| j.pid == current) {
            let next = (idx + 1) % self.jvms.len();
            self.selected_pid = Some(self.jvms[next].pid);
        } else {
            self.selected_pid = Some(self.jvms[0].pid);
        }
        self.on_selected_jvm_changed();
    }

    pub fn prev_jvm(&mut self) {
        if self.jvms.is_empty() {
            return;
        }
        let current = self.selected_pid.unwrap_or(0);
        if let Some(idx) = self.jvms.iter().position(|j| j.pid == current) {
            let prev = (idx + self.jvms.len() - 1) % self.jvms.len();
            self.selected_pid = Some(self.jvms[prev].pid);
        } else {
            self.selected_pid = Some(self.jvms[0].pid);
        }
        self.on_selected_jvm_changed();
    }

    fn on_selected_jvm_changed(&mut self) {
        self.cpu.methods.clear();
        self.cpu.method_edges.clear();
        self.cpu.expanded_hotspots.clear();
        self.cpu.selected_hotspot_idx = 0;
        if let Some(pid) = self.selected_pid {
            self.push_log(LogLevel::Ok, "Switch JVM", format!("selected JVM {pid}"));
        }
    }

    // --- Actions ---

    pub fn start_selected_jvm_session(&mut self) {
        self.ensure_default_jvm_selected();
        if self.selected_pid.is_none() {
            self.push_log(LogLevel::Warn, "Start Session", "select a JVM first");
            return;
        }
        self.go_to_view(ViewId::TeleOverview);
        self.spawn_refresh_telemetry();
        self.start_initial_cpu_recording();
    }

    pub fn refresh_current(&mut self) {
        self.spawn_refresh_jvms();
        if self.selected_pid.is_some() {
            self.spawn_refresh_telemetry();
        }
        let view = self.selected_view;
        if matches!(
            view,
            ViewId::LiveAllObjects
                | ViewId::LiveRecordedObjects
                | ViewId::LiveAllocationCallTree
                | ViewId::LiveAllocationHotSpots
                | ViewId::LiveClassTracker
                | ViewId::HeapClasses
        ) {
            self.spawn_refresh_live_memory();
        }
        if matches!(
            view,
            ViewId::ThreadsHistory | ViewId::ThreadsMonitor | ViewId::ThreadsDumps
        ) {
            self.spawn_capture_thread_dump();
        }
    }

    pub fn toggle_recording(&mut self) {
        if self.selected_view == ViewId::StartCenter {
            self.start_selected_jvm_session();
            return;
        }
        if self.cpu.recording {
            if self.cpu_recording_stop_pending {
                self.push_log(
                    LogLevel::Info,
                    "Stop CPU",
                    "CPU recording is already stopping",
                );
            } else {
                self.spawn_stop_cpu_recording();
            }
        } else if self.cpu_recording_start_pending {
            self.push_log(
                LogLevel::Info,
                "Start CPU",
                "CPU recording is already starting",
            );
        } else {
            self.spawn_start_cpu_recording("Start CPU");
        }
    }

    pub fn start_initial_cpu_recording(&mut self) {
        if self.cpu.recording {
            return;
        }
        if self.selected_pid.is_none() {
            self.push_log(
                LogLevel::Warn,
                "Initial CPU",
                "waiting for JVM discovery before starting CPU recording",
            );
            return;
        }
        self.spawn_start_cpu_recording("Initial CPU");
    }

    pub fn set_agent_port(&mut self, port: u16) {
        self.agent_port = port;
        self.cpu.agent_port = port;
    }

    pub fn run_gc(&mut self) {
        let Some(pid) = self.selected_pid else {
            self.push_log(LogLevel::Warn, "Run GC", "select a JVM first");
            return;
        };
        let handle = QueryHandle::spawn(move || {
            match hidden_command("jcmd")
                .arg(pid.to_string())
                .arg("GC.run")
                .output()
            {
                Ok(output) if output.status.success() => QueryResult::Gc {
                    success: true,
                    message: format!("GC requested for PID {pid}"),
                    pid,
                },
                Ok(output) => QueryResult::Gc {
                    success: false,
                    message: command_output_message("jcmd GC.run", &output),
                    pid,
                },
                Err(err) => QueryResult::Gc {
                    success: false,
                    message: format!("failed to execute jcmd: {err}"),
                    pid,
                },
            }
        });
        self.pending_queries.push(handle);
    }

    fn spawn_stop_cpu_recording(&mut self) {
        if self.cpu_recording_stop_pending {
            return;
        }
        let port = self.cpu.agent_port;
        self.cpu_recording_stop_pending = true;
        self.push_log(
            LogLevel::Info,
            "Stop CPU",
            format!("stopping CPU recording on localhost:{port} in background"),
        );
        let handle =
            QueryHandle::spawn(
                move || match stop_openprofiler_cpu_recording_on_port(port) {
                    Ok(()) => QueryResult::CpuRecording {
                        success: true,
                        action: "Stop CPU".to_string(),
                        message: format!("CPU recording stopped on localhost:{port}"),
                        pid: 0,
                        port,
                    },
                    Err(message) => QueryResult::CpuRecording {
                        success: false,
                        action: "Stop CPU".to_string(),
                        message,
                        pid: 0,
                        port,
                    },
                },
            );
        self.pending_queries.push(handle);
    }

    pub fn copy_selected_item(&mut self) {
        let Some(text) = self.current_view_copy_text() else {
            self.push_log(LogLevel::Warn, "Copy", "no table data to copy");
            return;
        };
        match copy_to_clipboard(&text) {
            Ok(()) => self.push_log(LogLevel::Ok, "Copy", "visible table copied as TSV"),
            Err(err) => self.push_log(LogLevel::Error, "Copy", err),
        }
    }

    pub fn mark_heap(&mut self) {
        if self.memory.rows.is_empty() {
            return;
        }
        self.memory.marked_heap = Some(
            self.memory
                .rows
                .iter()
                .map(|row| (row.name.clone(), (row.instances, row.bytes)))
                .collect(),
        );
        self.push_log(
            LogLevel::Ok,
            "Mark Heap",
            "current class histogram marked as baseline",
        );
    }

    // --- Query spawning ---

    pub fn spawn_refresh_jvms(&mut self) {
        let handle = QueryHandle::spawn(|| match hidden_command("jps").arg("-lv").output() {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                let targets: Vec<JvmTarget> = text
                    .lines()
                    .filter_map(|line| parse_jps_line(line))
                    .collect();
                QueryResult::Jvms(targets)
            }
            Ok(output) => QueryResult::JvmsError(command_output_message("jps -lv", &output)),
            Err(err) => QueryResult::JvmsError(format!("failed to execute jps: {err}")),
        });
        self.jvms_query = Some(handle);
    }

    pub fn spawn_refresh_telemetry(&mut self) {
        let pid = match self.selected_pid {
            Some(p) => p,
            None => return,
        };
        let elapsed_secs = self.elapsed_secs();
        let handle = QueryHandle::spawn(move || {
            let process = query_process(pid);
            let heap = query_heap(pid);
            let gc = query_gc(pid);
            let cpu_seconds = process.as_ref().map(|p| p.cpu_seconds);
            QueryResult::Telemetry {
                process,
                heap,
                gc,
                elapsed_secs,
                cpu_seconds,
            }
        });
        self.telemetry_query = Some(handle);
    }

    pub fn spawn_refresh_live_memory(&mut self) {
        let pid = match self.selected_pid {
            Some(p) => p,
            None => {
                self.push_log(LogLevel::Warn, "Refresh Live Memory", "select a JVM first");
                return;
            }
        };
        let previous = self.memory.previous_histogram.clone();
        let port = self.cpu.agent_port;
        self.memory.loaded_once = true;
        let handle = QueryHandle::spawn(move || match query_class_histogram(pid) {
            Ok(mut rows) => {
                for row in &mut rows {
                    if let Some((old_instances, old_bytes)) = previous.get(&row.name) {
                        row.delta_instances = row.instances as i64 - *old_instances as i64;
                        row.delta_bytes = row.bytes as i64 - *old_bytes as i64;
                    }
                }
                let allocation_hotspots = query_allocation_hotspots(port).unwrap_or_default();
                QueryResult::LiveMemory(Ok(LiveMemorySnapshot {
                    rows,
                    allocation_hotspots,
                }))
            }
            Err(e) => QueryResult::LiveMemory(Err(e)),
        });
        self.memory_query = Some(handle);
    }

    pub fn spawn_sample_cpu(&mut self) {
        let pid = match self.selected_pid {
            Some(p) => p,
            None => {
                self.push_log(LogLevel::Warn, "CPU Sample", "select a JVM first");
                return;
            }
        };
        let thread_status = self.cpu.thread_status.clone();
        let handle = QueryHandle::spawn(move || match query_thread_dump(pid) {
            Ok(dump) => QueryResult::CpuSample {
                result: Ok(dump),
                thread_status,
            },
            Err(e) => QueryResult::CpuSample {
                result: Err(e),
                thread_status,
            },
        });
        self.thread_query = Some(handle);
    }

    pub fn spawn_capture_thread_dump(&mut self) {
        let pid = match self.selected_pid {
            Some(p) => p,
            None => {
                self.push_log(LogLevel::Warn, "Thread Dump", "select a JVM first");
                return;
            }
        };
        let handle = QueryHandle::spawn(move || QueryResult::ThreadDump(query_thread_dump(pid)));
        self.thread_query = Some(handle);
    }

    pub fn spawn_fetch_cpu_instrumentation(&mut self) {
        let port = self.cpu.agent_port;
        let handle = QueryHandle::spawn(move || {
            use openprofiler_core::protocol::AgentClient;
            use std::net::SocketAddr;
            use std::thread;
            use std::time::{Duration, Instant};

            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            match AgentClient::connect(addr) {
                Ok(mut client) => {
                    let deadline = Instant::now() + Duration::from_millis(1500);
                    loop {
                        match client.get_cpu_data() {
                            Ok(cpu_data) => {
                                let has_data = !cpu_data.hot_spots.is_empty()
                                    || cpu_data
                                        .method_graph
                                        .as_ref()
                                        .is_some_and(|graph| !graph.nodes.is_empty());
                                if has_data || Instant::now() >= deadline {
                                    let (rows, method_edges) = cpu_rows_from_agent_data(&cpu_data);
                                    return QueryResult::CpuInstrumentation(Ok((
                                        rows,
                                        method_edges,
                                    )));
                                }
                            }
                            Err(e) => return QueryResult::CpuInstrumentation(Err(e)),
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                }
                Err(e) => QueryResult::CpuInstrumentation(Err(e)),
            }
        });
        self.thread_query = Some(handle);
    }

    fn spawn_start_cpu_recording(&mut self, action: &'static str) {
        if self.cpu_recording_start_pending {
            return;
        }
        let Some(pid) = self.selected_pid else {
            return;
        };
        let preferred_port = self.cpu.agent_port;
        let includes = self
            .selected_target()
            .and_then(|target| default_instrument_include(&target.main_class));
        self.cpu_recording_start_pending = true;
        self.push_log(
            LogLevel::Info,
            action,
            format!("starting CPU recording for JVM {pid} in background"),
        );
        let handle = QueryHandle::spawn(move || {
            let result = start_openprofiler_cpu_recording_for_pid(pid, preferred_port, includes);
            match result {
                Ok(port) => QueryResult::CpuRecording {
                    success: true,
                    action: action.to_string(),
                    message: format!("CPU recording enabled for JVM {pid} on localhost:{port}"),
                    pid,
                    port,
                },
                Err(message) => QueryResult::CpuRecording {
                    success: false,
                    action: action.to_string(),
                    message,
                    pid,
                    port: preferred_port,
                },
            }
        });
        self.pending_queries.push(handle);
    }

    // --- Poll queries ---

    pub fn poll_queries(&mut self) {
        let mut results = Vec::new();
        if let Some(handle) = self.jvms_query.take() {
            match handle.poll() {
                Some(result) => results.push(result),
                None => self.jvms_query = Some(handle),
            }
        }
        if let Some(handle) = self.telemetry_query.take() {
            match handle.poll() {
                Some(result) => results.push(result),
                None => self.telemetry_query = Some(handle),
            }
        }
        if let Some(handle) = self.memory_query.take() {
            match handle.poll() {
                Some(result) => results.push(result),
                None => self.memory_query = Some(handle),
            }
        }
        if let Some(handle) = self.thread_query.take() {
            match handle.poll() {
                Some(result) => results.push(result),
                None => self.thread_query = Some(handle),
            }
        }
        let mut still_pending = Vec::new();
        for handle in self.pending_queries.drain(..) {
            match handle.poll() {
                Some(result) => results.push(result),
                None => still_pending.push(handle),
            }
        }
        self.pending_queries = still_pending;
        for result in results {
            self.apply_result(result);
        }
    }

    fn apply_result(&mut self, result: QueryResult) {
        match result {
            QueryResult::Jvms(targets) => {
                let count = targets.len();
                self.jvms = targets;
                self.ensure_default_jvm_selected();
                self.push_log(
                    LogLevel::Ok,
                    "Refresh JVMs",
                    format!("{} local JVM(s) discovered", count),
                );
                if self.selected_view != ViewId::StartCenter
                    && self.selected_pid.is_some()
                    && !self.cpu.recording
                {
                    self.pending_initial_cpu_start = true;
                }
            }
            QueryResult::JvmsError(message) => {
                self.push_log(LogLevel::Error, "Refresh JVMs", message);
            }
            QueryResult::Telemetry {
                process,
                heap,
                gc,
                elapsed_secs,
                cpu_seconds,
            } => {
                let mut cpu_percent = 0.0;
                if let Some(cpu_secs) = cpu_seconds {
                    if let Some((last_cpu, last_at)) = self.telemetry.last_process_cpu {
                        let elapsed = Instant::now().duration_since(last_at).as_secs_f64();
                        if elapsed > 0.0 && cpu_secs >= last_cpu {
                            cpu_percent =
                                ((cpu_secs - last_cpu) / elapsed * 100.0).clamp(0.0, 800.0);
                        }
                    }
                    self.telemetry.last_process_cpu = Some((cpu_secs, Instant::now()));
                }
                let sample = TelemetrySample {
                    elapsed_secs,
                    heap_used_mb: heap.as_ref().map_or(0.0, |h| h.used_mb),
                    heap_committed_mb: heap.as_ref().map_or(0.0, |h| h.committed_mb),
                    process_mb: process.as_ref().map_or(0.0, |p| p.working_set_mb),
                    private_mb: process.as_ref().map_or(0.0, |p| p.private_mb),
                    cpu_percent,
                    thread_count: process.as_ref().map_or(0.0, |p| p.thread_count as f64),
                    handle_count: process.as_ref().map_or(0.0, |p| p.handle_count as f64),
                    class_count: self.memory.rows.len() as f64,
                    gc_count: gc.as_ref().map_or(0.0, |g| g.count as f64),
                    gc_time_ms: gc.as_ref().map_or(0.0, |g| g.time_ms as f64),
                };
                self.telemetry.samples.push_back(sample);
                while self.telemetry.samples.len() > 240 {
                    self.telemetry.samples.pop_front();
                }
            }
            QueryResult::LiveMemory(result) => match result {
                Ok(snapshot) => {
                    let rows = snapshot.rows;
                    self.memory.previous_histogram = rows
                        .iter()
                        .map(|row| (row.name.clone(), (row.instances, row.bytes)))
                        .collect();
                    let count = rows.len();
                    self.memory.rows = rows;
                    self.memory.allocation_hotspots = snapshot.allocation_hotspots;
                    let allocation_count = self.memory.allocation_hotspots.len();
                    let max_idx = if self.selected_view == ViewId::LiveAllocationHotSpots {
                        self.visible_allocation_hotspot_rows()
                            .len()
                            .saturating_sub(1)
                    } else {
                        self.filtered_memory_rows().len().saturating_sub(1)
                    };
                    self.memory.selected_row_idx = self.memory.selected_row_idx.min(max_idx);
                    keep_visible(
                        &mut self.memory.row_offset,
                        self.memory.selected_row_idx,
                        DEFAULT_PAGE_ROWS,
                    );
                    self.push_log(
                        LogLevel::Ok,
                        "Refresh Live Memory",
                        format!("{count} class row(s), {allocation_count} allocation site row(s) loaded"),
                    );
                }
                Err(message) => {
                    self.push_log(LogLevel::Error, "Live Memory", message);
                }
            },
            QueryResult::CpuSample {
                result,
                thread_status,
            } => match result {
                Ok(dump) => {
                    let mut added = 0;
                    for thread in &dump.threads {
                        if thread.frames.is_empty() {
                            continue;
                        }
                        if thread_status == "Runnable" && thread.state != "RUNNABLE" {
                            continue;
                        }
                        self.cpu.samples.push_back(StackSample {
                            thread: thread.name.clone(),
                            state: thread.state.clone(),
                            frames: thread.frames.clone(),
                            stack_trace: thread.stack_trace.clone(),
                            captured_at: dump.captured_at.clone(),
                            elapsed_secs: self.elapsed_secs(),
                        });
                        added += 1;
                    }
                    while self.cpu.samples.len() > 800 {
                        self.cpu.samples.pop_front();
                    }
                    self.rebuild_cpu_rows();
                    self.update_thread_history(&dump.threads);
                    self.push_thread_dump(dump);
                    self.push_log(
                        LogLevel::Ok,
                        "CPU Sample",
                        format!("{} stack sample(s) captured", added),
                    );
                }
                Err(message) => self.push_log(LogLevel::Error, "CPU Sample", message),
            },
            QueryResult::CpuInstrumentation(result) => match result {
                Ok((rows, method_edges)) => {
                    let count = rows.len();
                    self.cpu.methods = rows;
                    self.cpu.method_edges = method_edges;
                    let max_idx = self.visible_hotspot_row_count().saturating_sub(1);
                    self.cpu.selected_hotspot_idx = self.cpu.selected_hotspot_idx.min(max_idx);
                    keep_visible(
                        &mut self.cpu.row_offset,
                        self.cpu.selected_hotspot_idx,
                        DEFAULT_PAGE_ROWS,
                    );
                    let db_max_idx = self.database_row_count().saturating_sub(1);
                    self.database.selected_row_idx = self.database.selected_row_idx.min(db_max_idx);
                    keep_visible(
                        &mut self.database.row_offset,
                        self.database.selected_row_idx,
                        DEFAULT_PAGE_ROWS,
                    );
                    self.push_log(
                        LogLevel::Ok,
                        "CPU Instrumentation",
                        format!("{} hot spot(s) fetched", count),
                    );
                }
                Err(message) => {
                    self.push_log(LogLevel::Warn, "CPU Instrumentation", message);
                }
            },
            QueryResult::ThreadDump(result) => match result {
                Ok(dump) => {
                    let thread_count = dump.threads.len();
                    self.update_thread_history(&dump.threads);
                    self.push_thread_dump(dump);
                    self.push_log(
                        LogLevel::Ok,
                        "Thread Dump",
                        format!("{} thread(s) captured", thread_count),
                    );
                }
                Err(message) => self.push_log(LogLevel::Error, "Thread Dump", message),
            },
            QueryResult::HeapSnapshot {
                success, message, ..
            } => {
                if success {
                    self.push_log(LogLevel::Ok, "Take Snapshot", message);
                } else {
                    self.push_log(LogLevel::Error, "Take Snapshot", message);
                }
            }
            QueryResult::CpuRecording {
                success,
                action,
                message,
                pid: _,
                port,
            } => {
                if action == "Stop CPU" {
                    self.cpu_recording_stop_pending = false;
                    if success {
                        self.cpu.recording = false;
                        self.push_log(LogLevel::Ok, action, message);
                    } else {
                        self.push_log(LogLevel::Error, action, message);
                    }
                } else {
                    self.cpu_recording_start_pending = false;
                    if success {
                        self.set_agent_port(port);
                        self.agent_enabled = true;
                        self.cpu.recording = true;
                        self.push_log(LogLevel::Ok, action, message);
                        if self.thread_query.is_none() {
                            self.spawn_fetch_cpu_instrumentation();
                        }
                    } else {
                        self.cpu.recording = false;
                        self.push_log(LogLevel::Error, action, message);
                    }
                }
            }
            QueryResult::Gc {
                success, message, ..
            } => {
                if success {
                    self.push_log(LogLevel::Ok, "Run GC", message);
                } else {
                    self.push_log(LogLevel::Error, "Run GC", message);
                }
            }
        }
    }

    fn ensure_default_jvm_selected(&mut self) {
        if self.selected_pid.is_some() || self.jvms.is_empty() {
            return;
        }
        self.selected_pid = self
            .jvms
            .iter()
            .find(|jvm| is_likely_demo_server(jvm))
            .or_else(|| self.jvms.first())
            .map(|jvm| jvm.pid);
    }

    fn update_thread_history(&mut self, threads: &[ThreadInfo]) {
        let mut summary = ThreadSummary {
            elapsed_secs: self.elapsed_secs(),
            runnable: 0,
            waiting: 0,
            timed_waiting: 0,
            blocked: 0,
            other: 0,
        };
        for thread in threads {
            match thread.state.as_str() {
                "RUNNABLE" => summary.runnable += 1,
                "WAITING" => summary.waiting += 1,
                "TIMED_WAITING" => summary.timed_waiting += 1,
                "BLOCKED" => summary.blocked += 1,
                _ => summary.other += 1,
            }
        }
        self.threads.history.push_back(summary);
        while self.threads.history.len() > 240 {
            self.threads.history.pop_front();
        }
    }

    fn rebuild_cpu_rows(&mut self) {
        let thread_filter = self.cpu.thread_status.clone();
        let config = CallTreeConfig {
            total_sample_count: self.cpu.samples.len() as u64,
            sampling_interval_ms: self.cpu.sampling_interval_ms,
        };
        self.cpu.call_tree = build_call_tree(&self.cpu.samples, &thread_filter, &config);
        let mut rows = build_hot_spots(&self.cpu.call_tree, config.total_sample_count);
        rows.sort_by_key(|row| Reverse(row.total_samples));
        rows.truncate(250);
        self.cpu.methods = rows;
        self.cpu.method_edges.clear();
    }

    fn push_thread_dump(&mut self, dump: ThreadDump) {
        self.threads.dumps.push(dump);
        while self.threads.dumps.len() > MAX_THREAD_DUMPS {
            self.threads.dumps.remove(0);
        }
        self.threads.selected_dump = Some(self.threads.dumps.len().saturating_sub(1));
    }

    // --- Auto-update ---

    pub fn auto_update(&mut self) {
        if self.selected_pid.is_none() && !self.cpu.recording {
            return;
        }
        let now = Instant::now();
        if self.pending_initial_cpu_start && self.selected_view != ViewId::StartCenter {
            self.pending_initial_cpu_start = false;
            self.start_initial_cpu_recording();
        }
        if now.duration_since(self.last_tick) >= Duration::from_secs(self.auto_update_secs) {
            self.last_tick = now;
            if self.selected_pid.is_some() && self.telemetry_query.is_none() {
                self.spawn_refresh_telemetry();
            }
            if self.cpu.recording && self.thread_query.is_none() {
                self.spawn_fetch_cpu_instrumentation();
            }
        }
        if !self.memory.loaded_once && self.selected_pid.is_some() && self.is_memory_view() {
            self.memory.loaded_once = true;
            if self.memory_query.is_none() {
                self.spawn_refresh_live_memory();
            }
        }
        if matches!(
            self.selected_view,
            ViewId::ThreadsHistory | ViewId::ThreadsMonitor | ViewId::ThreadsDumps
        ) && now.duration_since(self.last_thread_refresh) >= Duration::from_secs(5)
        {
            self.last_thread_refresh = now;
            if self.thread_query.is_none() {
                self.spawn_capture_thread_dump();
            }
        }
    }

    pub fn filtered_cpu_rows(&self) -> Vec<CpuMethodRow> {
        let filter = if !self.filter_input.is_empty() {
            self.filter_input.to_lowercase()
        } else {
            String::new()
        };
        self.cpu
            .methods
            .iter()
            .filter(|row| !is_database_method(row))
            .filter(|row| filter.is_empty() || row.method.to_lowercase().contains(&filter))
            .cloned()
            .collect()
    }

    pub fn hotspot_children(&self, method_id: i32) -> Vec<(CpuMethodEdgeRow, CpuMethodRow)> {
        let methods_by_id: HashMap<i32, CpuMethodRow> = self
            .cpu
            .methods
            .iter()
            .filter(|row| !is_database_method(row))
            .filter_map(|row| row.method_id.map(|id| (id, row.clone())))
            .collect();
        let mut children: Vec<_> = self
            .cpu
            .method_edges
            .iter()
            .filter(|edge| edge.from_method_id == method_id)
            .filter_map(|edge| {
                methods_by_id
                    .get(&edge.to_method_id)
                    .map(|row| (edge.clone(), row.clone()))
            })
            .collect();
        children.sort_by(|a, b| b.0.total_duration_nano.cmp(&a.0.total_duration_nano));
        children
    }

    pub fn visible_hotspot_row_count(&self) -> usize {
        let mut count = 0usize;
        for row in self.filtered_cpu_rows() {
            count += 1;
            if let Some(method_id) = row.method_id {
                if self.cpu.expanded_hotspots.contains(&method_id) {
                    count += self.hotspot_children(method_id).len();
                }
            }
        }
        count
    }

    pub fn database_row_count(&self) -> usize {
        let sql_rows = self.filtered_sql_hotspot_rows();
        if !sql_rows.is_empty() {
            return sql_rows.len();
        }
        let filter = self.filter_input.to_lowercase();
        self.cpu
            .methods
            .iter()
            .filter(|row| is_database_method(row))
            .filter(|row| {
                let label = database_method_label(row).to_lowercase();
                filter.is_empty() || label.contains(&filter)
            })
            .count()
    }

    pub fn filtered_sql_hotspot_rows(&self) -> Vec<SqlHotSpotRow> {
        let filter = self.filter_input.to_lowercase();
        let mut rows = aggregate_sql_hotspots(&self.database.events);
        rows.retain(|row| filter.is_empty() || row.sql.to_lowercase().contains(&filter));
        rows
    }

    fn current_view_copy_text(&self) -> Option<String> {
        match self.selected_view {
            ViewId::CpuHotSpots => self.cpu_hotspots_copy_text(),
            ViewId::LiveAllocationHotSpots => self.allocation_hotspots_copy_text(),
            ViewId::DatabasesJdbc => self.database_hotspots_copy_text(),
            _ => None,
        }
    }

    fn cpu_hotspots_copy_text(&self) -> Option<String> {
        let mut lines = vec!["Hot Spot\tSelf Time\tAvg Time\tEvents".to_string()];
        for row in self.filtered_cpu_rows() {
            lines.push(format!(
                "{}\t{}\t{}\t{}",
                row.method,
                format_duration_nano_f(row.self_ms * 1_000_000.0),
                format_duration_nano_f(row.average_nanos),
                row.invocations
            ));
            if let Some(method_id) = row.method_id {
                if self.cpu.expanded_hotspots.contains(&method_id) {
                    for (edge, child) in self.hotspot_children(method_id) {
                        let avg_nanos = if edge.call_count > 0 {
                            edge.total_duration_nano as f64 / edge.call_count as f64
                        } else {
                            0.0
                        };
                        lines.push(format!(
                            "{}\t{}\t{}\t{}",
                            child.method,
                            format_duration_nano_f(edge.total_duration_nano as f64),
                            format_duration_nano_f(avg_nanos),
                            edge.call_count
                        ));
                    }
                }
            }
        }
        (lines.len() > 1).then(|| lines.join("\n"))
    }

    fn allocation_hotspots_copy_text(&self) -> Option<String> {
        let rows = self.visible_allocation_hotspot_rows();
        if rows.is_empty() {
            return None;
        }
        let mut lines = vec!["Hot Spot\tSelf Allocated Memory\tAllocations".to_string()];
        lines.extend(rows.iter().map(|row| {
            let prefix = if row.depth == 0 {
                String::new()
            } else {
                format!("{}- ", "  ".repeat(row.depth.min(8)))
            };
            format!(
                "{}\t{}\t{}",
                format!("{prefix}{}", row.name),
                format_bytes(row.bytes),
                row.allocations
            )
        }));
        Some(lines.join("\n"))
    }

    fn database_hotspots_copy_text(&self) -> Option<String> {
        let sql_rows = self.filtered_sql_hotspot_rows();
        if !sql_rows.is_empty() {
            let mut lines = vec!["Hot Spot\tTime\tAvg Time\tEvents".to_string()];
            lines.extend(sql_rows.iter().map(|row| {
                format!(
                    "{}\t{}\t{}\t{}",
                    row.sql,
                    format_duration_nano_f(row.total_ms * 1_000_000.0),
                    format_duration_nano_f(row.average_ms * 1_000_000.0),
                    row.events
                )
            }));
            return Some(lines.join("\n"));
        }

        let mut rows: Vec<_> = self
            .cpu
            .methods
            .iter()
            .filter(|row| is_database_method(row))
            .filter(|row| {
                let label = database_method_label(row).to_lowercase();
                let filter = self.filter_input.to_lowercase();
                filter.is_empty() || label.contains(&filter)
            })
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            b.self_ms
                .partial_cmp(&a.self_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if rows.is_empty() {
            return None;
        }
        let mut lines = vec!["Hot Spot\tTime\tAvg Time\tEvents".to_string()];
        lines.extend(rows.iter().map(|row| {
            format!(
                "{}\t{}\t{}\t{}",
                database_method_label(row),
                format_duration_nano_f(row.self_ms * 1_000_000.0),
                format_duration_nano_f(row.average_nanos),
                row.invocations
            )
        }));
        Some(lines.join("\n"))
    }

    pub fn allocation_hotspot_rows(&self) -> Vec<AllocationHotSpotRow> {
        if !self.memory.allocation_hotspots.is_empty() {
            let filter = self.filter_input.to_lowercase();
            return self
                .memory
                .allocation_hotspots
                .iter()
                .filter(|row| {
                    filter.is_empty()
                        || row.name.to_lowercase().contains(&filter)
                        || row.allocated_type.to_lowercase().contains(&filter)
                })
                .cloned()
                .collect();
        }

        let baseline = self.memory.marked_heap.as_ref();
        let mut rows: Vec<_> = self
            .filtered_memory_rows()
            .into_iter()
            .filter_map(|row| {
                let (bytes, allocations) = if let Some(marked) = baseline {
                    let (old_instances, old_bytes) =
                        marked.get(&row.name).copied().unwrap_or((0, 0));
                    (
                        row.bytes.saturating_sub(old_bytes),
                        row.instances.saturating_sub(old_instances),
                    )
                } else if row.delta_bytes > 0 || row.delta_instances > 0 {
                    (
                        row.delta_bytes.max(0) as u64,
                        row.delta_instances.max(0) as u64,
                    )
                } else {
                    (row.bytes, row.instances)
                };
                if bytes == 0 && allocations == 0 {
                    None
                } else {
                    Some(AllocationHotSpotRow {
                        node_id: row.rank as i32,
                        parent_id: None,
                        depth: 0,
                        name: row.name,
                        allocated_type: String::new(),
                        bytes,
                        allocations,
                    })
                }
            })
            .collect();
        rows.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        rows
    }

    pub fn visible_allocation_hotspot_rows(&self) -> Vec<AllocationHotSpotRow> {
        let rows = self.allocation_hotspot_rows();
        if !self.filter_input.is_empty() {
            return rows;
        }
        let children_by_parent = allocation_children_by_parent(&rows);
        let mut visible = Vec::new();
        append_visible_allocation_rows(
            None,
            &rows,
            &children_by_parent,
            &self.memory.expanded_hotspots,
            &mut visible,
        );
        visible
    }

    pub fn allocation_hotspot_has_children(&self, node_id: i32) -> bool {
        self.allocation_hotspot_rows()
            .iter()
            .any(|row| row.parent_id == Some(node_id))
    }

    pub fn selected_hotspot_method_id(&self) -> Option<i32> {
        let mut idx = 0usize;
        for row in self.filtered_cpu_rows() {
            if idx == self.cpu.selected_hotspot_idx {
                return row.method_id;
            }
            idx += 1;
            if let Some(method_id) = row.method_id {
                if self.cpu.expanded_hotspots.contains(&method_id) {
                    for _ in self.hotspot_children(method_id) {
                        if idx == self.cpu.selected_hotspot_idx {
                            return Some(method_id);
                        }
                        idx += 1;
                    }
                }
            }
        }
        None
    }

    pub fn toggle_selected_hotspot_expanded(&mut self) {
        if let Some(method_id) = self.selected_hotspot_method_id() {
            if self.hotspot_children(method_id).is_empty() {
                return;
            }
            if !self.cpu.expanded_hotspots.insert(method_id) {
                self.cpu.expanded_hotspots.remove(&method_id);
            }
        }
    }

    pub fn selected_allocation_hotspot_node_id(&self) -> Option<i32> {
        self.visible_allocation_hotspot_rows()
            .get(self.memory.selected_row_idx)
            .map(|row| row.node_id)
    }

    pub fn toggle_selected_allocation_hotspot_expanded(&mut self) {
        if let Some(node_id) = self.selected_allocation_hotspot_node_id() {
            if !self.allocation_hotspot_has_children(node_id) {
                return;
            }
            if !self.memory.expanded_hotspots.insert(node_id) {
                self.memory.expanded_hotspots.remove(&node_id);
            }
            let max_idx = self
                .visible_allocation_hotspot_rows()
                .len()
                .saturating_sub(1);
            self.memory.selected_row_idx = self.memory.selected_row_idx.min(max_idx);
        }
    }

    pub fn filtered_memory_rows(&self) -> Vec<MemoryClassRow> {
        let filter = self.filter_input.to_lowercase();
        self.memory
            .rows
            .iter()
            .filter(|row| filter.is_empty() || row.name.to_lowercase().contains(&filter))
            .cloned()
            .collect()
    }

    fn is_memory_view(&self) -> bool {
        matches!(
            self.selected_view,
            ViewId::LiveAllObjects
                | ViewId::LiveRecordedObjects
                | ViewId::LiveAllocationCallTree
                | ViewId::LiveAllocationHotSpots
                | ViewId::LiveClassTracker
                | ViewId::HeapClasses
        )
    }
}

pub fn is_database_method(row: &CpuMethodRow) -> bool {
    row.class_name == "SQL"
}

pub fn database_method_label(row: &CpuMethodRow) -> String {
    if row.class_name == "SQL" {
        row.method_name.clone()
    } else {
        row.method.clone()
    }
}

#[derive(Clone)]
pub struct SqlHotSpotRow {
    pub sql: String,
    pub total_ms: f64,
    pub average_ms: f64,
    pub events: u64,
}

fn aggregate_sql_hotspots(events: &VecDeque<DatabaseEvent>) -> Vec<SqlHotSpotRow> {
    let mut by_sql = HashMap::<String, (f64, u64)>::new();
    for event in events {
        if event.operation != "SQL" {
            continue;
        }
        let duration_ms = event.duration_ms.unwrap_or(0.0).max(0.0);
        let entry = by_sql.entry(event.message.clone()).or_insert((0.0, 0));
        entry.0 += duration_ms;
        entry.1 += 1;
    }
    let mut rows: Vec<_> = by_sql
        .into_iter()
        .map(|(sql, (total_ms, events))| SqlHotSpotRow {
            sql,
            total_ms,
            average_ms: if events > 0 {
                total_ms / events as f64
            } else {
                0.0
            },
            events,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.total_ms
            .partial_cmp(&a.total_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

fn query_allocation_hotspots(port: u16) -> Result<Vec<AllocationHotSpotRow>, String> {
    use openprofiler_core::protocol::AgentClient;
    use std::net::SocketAddr;

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut client = AgentClient::connect_with_timeout(
        addr,
        Duration::from_millis(300),
        Duration::from_secs(2),
    )?;
    let memory_data = client.get_memory_data()?;
    Ok(allocation_hotspots_from_agent_data(memory_data))
}

fn allocation_children_by_parent(
    rows: &[AllocationHotSpotRow],
) -> HashMap<Option<i32>, Vec<AllocationHotSpotRow>> {
    let mut children: HashMap<Option<i32>, Vec<AllocationHotSpotRow>> = HashMap::new();
    for row in rows {
        children.entry(row.parent_id).or_default().push(row.clone());
    }
    for child_rows in children.values_mut() {
        child_rows.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    }
    children
}

fn append_visible_allocation_rows(
    parent_id: Option<i32>,
    all_rows: &[AllocationHotSpotRow],
    children_by_parent: &HashMap<Option<i32>, Vec<AllocationHotSpotRow>>,
    expanded: &HashSet<i32>,
    visible: &mut Vec<AllocationHotSpotRow>,
) {
    let Some(children) = children_by_parent.get(&parent_id) else {
        return;
    };
    for row in children {
        visible.push(row.clone());
        if expanded.contains(&row.node_id)
            && all_rows
                .iter()
                .any(|child| child.parent_id == Some(row.node_id))
        {
            append_visible_allocation_rows(
                Some(row.node_id),
                all_rows,
                children_by_parent,
                expanded,
                visible,
            );
        }
    }
}

fn move_index(current: usize, delta: isize, max_idx: usize) -> usize {
    if delta < 0 {
        current.saturating_sub((-delta) as usize)
    } else {
        current.saturating_add(delta as usize).min(max_idx)
    }
}

fn keep_visible(offset: &mut usize, selected: usize, page_rows: usize) {
    let page_rows = page_rows.max(1);
    if selected < *offset {
        *offset = selected;
    } else if selected >= offset.saturating_add(page_rows) {
        *offset = selected.saturating_sub(page_rows - 1);
    }
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|err| format!("clipboard unavailable: {err}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|err| format!("clipboard copy failed: {err}"))
}

fn start_openprofiler_cpu_recording_for_pid(
    pid: u32,
    preferred_port: u16,
    includes: Option<String>,
) -> Result<u16, String> {
    let port = if let Some(port) = discover_openprofiler_agent_port(pid, preferred_port) {
        port
    } else {
        let port = select_attach_port(pid, preferred_port)
            .ok_or_else(|| "no free OpenProfiler TCP agent port in 8849..8999".to_string())?;
        attach_openprofiler_agent(pid, port, includes.as_deref())?;
        wait_for_openprofiler_agent(pid, port, Duration::from_secs(8))
            .map_err(|err| format!("attached agent did not become ready on port {port}: {err}"))?;
        port
    };

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut last_err = None;
    for attempt in 1..=3 {
        match openprofiler_core::protocol::AgentClient::connect_with_timeout(
            addr,
            Duration::from_millis(250),
            Duration::from_millis(1_200),
        )
        .and_then(|mut client| client.start_cpu_recording_fast())
        {
            Ok(()) => return Ok(port),
            Err(err) => {
                last_err = Some(err);
                if attempt < 3 {
                    std::thread::sleep(Duration::from_millis(300 * attempt));
                }
            }
        }
    }
    Err(format!(
        "agent command failed on {addr}: {}",
        last_err.unwrap_or_else(|| "unknown TCP agent error".to_string())
    ))
}

fn stop_openprofiler_cpu_recording_on_port(port: u16) -> Result<(), String> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    openprofiler_core::protocol::AgentClient::connect_with_timeout(
        addr,
        Duration::from_millis(250),
        Duration::from_millis(1_200),
    )
    .and_then(|mut client| client.stop_cpu_recording_fast())
    .map_err(|err| format!("agent stop command failed on {addr}: {err}"))
}

fn attach_openprofiler_agent(
    pid: u32,
    port: u16,
    includes: Option<&str>,
) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot resolve cwd: {e}"))?;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let paths = resolve_agent_runtime_paths(&cwd, exe_dir.as_deref())?;
    let mut agent_args = if let Some(native_dll) = paths.native_dll.as_ref() {
        format!(
            "port={},native={},jdbc=true,jdbcMode=native",
            port,
            native_dll.display()
        )
    } else {
        format!("port={},jdbc=true,jdbcMode=id", port)
    };
    if let Some(includes) = includes {
        if !includes.is_empty() {
            agent_args.push_str(",includes=");
            agent_args.push_str(includes);
        }
    }
    let output = hidden_command("java")
        .args([
            "--add-modules",
            "jdk.attach",
            "-cp",
            &paths.attach_classpath.display().to_string(),
            "AttachAgent",
            &pid.to_string(),
            &paths.agent_jar.display().to_string(),
            &agent_args,
        ])
        .output()
        .map_err(|e| format!("failed to execute java attach helper: {e}"))?;
    if output.status.success() {
        Ok(format!(
            "OpenProfiler agent attached to JVM {pid} on localhost:{port}"
        ))
    } else {
        Err(command_output_message("AttachAgent", &output))
    }
}

fn default_instrument_include(main_class: &str) -> Option<String> {
    let normalized = main_class
        .trim()
        .trim_end_matches(".jar")
        .replace('\\', "/");
    if normalized.is_empty()
        || normalized == "<unknown>"
        || normalized.ends_with("/java")
        || normalized.ends_with("/javaw")
    {
        return None;
    }
    if normalized.contains('/') && !normalized.contains('.') {
        return None;
    }
    let class_name = normalized.rsplit('/').next().unwrap_or(&normalized);
    let mut parts: Vec<&str> = class_name.split('.').collect();
    if parts.len() <= 1 {
        return None;
    }
    parts.pop();
    let package = parts.join(".");
    if package.starts_with("java.")
        || package.starts_with("javax.")
        || package.starts_with("jdk.")
        || package.starts_with("sun.")
        || package.starts_with("com.sun.")
        || package.starts_with("org.apache.")
        || package.starts_with("org.springframework.")
        || package.starts_with("com.google.")
    {
        return None;
    }
    Some(package)
}

struct AgentRuntimePaths {
    agent_jar: PathBuf,
    attach_classpath: PathBuf,
    native_dll: Option<PathBuf>,
}

fn resolve_agent_runtime_paths(
    cwd: &Path,
    exe_dir: Option<&Path>,
) -> Result<AgentRuntimePaths, String> {
    let mut roots = Vec::new();
    if let Some(exe_dir) = exe_dir {
        roots.push(exe_dir.to_path_buf());
    }
    roots.push(cwd.to_path_buf());
    roots.sort();
    roots.dedup();

    let mut agent_jar = None;
    let mut attach_classpath = None;
    let mut native_dll = None;

    for root in &roots {
        if agent_jar.is_none() {
            agent_jar = agent_jar_candidates(root)
                .into_iter()
                .find(|path| path.exists());
        }
        if attach_classpath.is_none() {
            attach_classpath = attach_classpath_candidates(root)
                .into_iter()
                .find(|path| path.join("AttachAgent.class").exists());
        }
        if native_dll.is_none() {
            native_dll = release_or_debug_native_dll(root);
        }
    }

    let agent_jar = agent_jar.ok_or_else(|| {
        format!(
            "java agent jar not found. Checked runtime roots: {}",
            roots
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let attach_classpath = attach_classpath.ok_or_else(|| {
        format!(
            "AttachAgent.class not found. Checked runtime roots: {}",
            roots
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    Ok(AgentRuntimePaths {
        agent_jar,
        attach_classpath,
        native_dll,
    })
}

fn agent_jar_candidates(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("java-agent-0.1.0.jar"),
        root.join("java-agent/target/java-agent-0.1.0.jar"),
    ]
}

fn attach_classpath_candidates(root: &Path) -> Vec<PathBuf> {
    vec![root.join("attach"), root.join("tools")]
}

fn release_or_debug_native_dll(cwd: &Path) -> Option<PathBuf> {
    let native_name = native_library_file_name();
    let candidates = [
        cwd.join(native_name),
        cwd.join("target").join("release").join(native_name),
        cwd.join("target").join("debug").join(native_name),
        cwd.join("crates/jvmti-agent")
            .join("target")
            .join("release")
            .join(native_name),
        cwd.join("crates/jvmti-agent")
            .join("target")
            .join("debug")
            .join(native_name),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn native_library_file_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "jvmti_agent_rust.dll"
    }
    #[cfg(target_os = "macos")]
    {
        "libjvmti_agent_rust.dylib"
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        "libjvmti_agent_rust.so"
    }
}

fn discover_openprofiler_agent_port(pid: u32, preferred_port: u16) -> Option<u16> {
    use openprofiler_core::protocol::AgentClient;
    use std::net::SocketAddr;

    let pid_ports = list_listening_ports_for_pid(pid);
    let mut candidates: Vec<u16> = pid_ports
        .iter()
        .copied()
        .filter(|port| (8840..=8999).contains(port))
        .collect();
    candidates.push(preferred_port);
    candidates.sort_unstable();
    candidates.dedup();
    if let Some(idx) = candidates.iter().position(|port| *port == preferred_port) {
        candidates.swap(0, idx);
    }

    for port in candidates {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let Ok(mut client) = AgentClient::connect_with_timeout(
            addr,
            Duration::from_millis(250),
            Duration::from_millis(700),
        ) else {
            continue;
        };
        if client.get_cpu_data().is_ok() {
            return Some(port);
        }
    }
    None
}

fn wait_for_openprofiler_agent(pid: u32, port: u16, timeout: Duration) -> Result<(), String> {
    use openprofiler_core::protocol::AgentClient;
    use std::net::SocketAddr;

    let deadline = Instant::now() + timeout;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut last_err = String::from("agent was not reachable");
    while Instant::now() < deadline {
        let owned_by_pid = port_owner_pids(port).is_empty_or_contains(pid);
        if owned_by_pid {
            match AgentClient::connect_with_timeout(
                addr,
                Duration::from_millis(250),
                Duration::from_millis(700),
            )
            .and_then(|mut client| client.get_cpu_data().map(|_| ()))
            {
                Ok(()) => return Ok(()),
                Err(err) => last_err = err,
            }
        } else {
            last_err = format!("port {port} is not owned by JVM {pid}");
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(last_err)
}

trait PortOwnerList {
    fn is_empty_or_contains(&self, pid: u32) -> bool;
}

impl PortOwnerList for Vec<u32> {
    fn is_empty_or_contains(&self, pid: u32) -> bool {
        self.is_empty() || self.contains(&pid)
    }
}

fn select_attach_port(pid: u32, preferred_port: u16) -> Option<u16> {
    if port_is_usable_for_attach(pid, preferred_port) {
        return Some(preferred_port);
    }
    (8849..=8999).find(|port| port_is_usable_for_attach(pid, *port))
}

fn port_is_usable_for_attach(pid: u32, port: u16) -> bool {
    let owners = port_owner_pids(port);
    if owners.iter().any(|owner| *owner != pid) {
        return false;
    }
    owners.is_empty() && can_bind_local_port(port)
}

fn can_bind_local_port(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
        && std::net::TcpListener::bind(("::1", port)).is_ok()
}

fn port_owner_pids(port: u16) -> Vec<u32> {
    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess -Unique"
        );
        let Ok(output) = hidden_command("powershell")
            .args(["-NoProfile", "-Command", script.as_str()])
            .output()
        else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        return String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().parse::<u32>().ok())
            .collect();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = port;
        Vec::new()
    }
}

fn list_listening_ports_for_pid(pid: u32) -> Vec<u16> {
    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object {{ $_.OwningProcess -eq {pid} }} | Select-Object -ExpandProperty LocalPort"
        );
        let Ok(output) = hidden_command("powershell")
            .args(["-NoProfile", "-Command", script.as_str()])
            .output()
        else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        return String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().parse::<u16>().ok())
            .collect();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
        Vec::new()
    }
}

#[cfg(test)]
mod agent_path_tests {
    use super::resolve_agent_runtime_paths;
    use std::fs;

    fn test_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("oprofiler-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        root
    }

    #[test]
    fn resolves_workspace_agent_layout() {
        let root = test_root("workspace-layout");
        let agent_dir = root.join("java-agent/target");
        let tools_dir = root.join("tools");
        fs::create_dir_all(&agent_dir).expect("create agent dir");
        fs::create_dir_all(&tools_dir).expect("create tools dir");
        fs::write(agent_dir.join("java-agent-0.1.0.jar"), b"jar").expect("write jar");
        fs::write(tools_dir.join("AttachAgent.class"), b"class").expect("write helper");

        let paths = resolve_agent_runtime_paths(&root, None).expect("resolve paths");

        assert_eq!(paths.agent_jar, agent_dir.join("java-agent-0.1.0.jar"));
        assert_eq!(paths.attach_classpath, tools_dir);
        assert!(paths.native_dll.is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolves_github_distribution_layout_from_exe_dir() {
        let cwd = test_root("dist-cwd");
        let exe_dir = test_root("dist-exe");
        let attach_dir = exe_dir.join("attach");
        fs::create_dir_all(&attach_dir).expect("create attach dir");
        fs::write(exe_dir.join("java-agent-0.1.0.jar"), b"jar").expect("write jar");
        fs::write(exe_dir.join("jvmti_agent_rust.dll"), b"dll").expect("write dll");
        fs::write(attach_dir.join("AttachAgent.class"), b"class").expect("write helper");

        let paths = resolve_agent_runtime_paths(&cwd, Some(&exe_dir)).expect("resolve paths");

        assert_eq!(paths.agent_jar, exe_dir.join("java-agent-0.1.0.jar"));
        assert_eq!(paths.attach_classpath, attach_dir);
        assert_eq!(paths.native_dll, Some(exe_dir.join("jvmti_agent_rust.dll")));

        let _ = fs::remove_dir_all(cwd);
        let _ = fs::remove_dir_all(exe_dir);
    }
}
