use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Telemetries,
    CpuViews,
    LiveMemory,
    Databases,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewId {
    StartCenter,
    TeleOverview,
    TeleMemory,
    TeleGc,
    TeleClasses,
    TeleThreads,
    TeleCpuLoad,
    LiveAllObjects,
    LiveRecordedObjects,
    LiveAllocationCallTree,
    LiveAllocationHotSpots,
    LiveClassTracker,
    HeapStart,
    HeapClasses,
    HeapBiggestObjects,
    HeapReferences,
    CpuCallTree,
    CpuHotSpots,
    CpuCallGraph,
    CpuTracer,
    CpuComplexity,
    CpuOutliers,
    ThreadsHistory,
    ThreadsMonitor,
    ThreadsDumps,
    DatabasesJdbc,
    DatabasesJpa,
    DatabasesMongo,
    DatabasesCassandra,
}

pub const CATEGORIES: [(Category, &str, &[ViewId]); 4] = [
    (
        Category::Telemetries,
        "Telemetries",
        &[ViewId::TeleOverview],
    ),
    (Category::CpuViews, "CPU", &[ViewId::CpuHotSpots]),
    (
        Category::LiveMemory,
        "Memory",
        &[ViewId::LiveAllocationHotSpots],
    ),
    (Category::Databases, "Databases", &[ViewId::DatabasesJdbc]),
];

impl ViewId {
    pub fn label(&self) -> &'static str {
        match self {
            ViewId::StartCenter => "Start Center",
            ViewId::TeleOverview => "Overview",
            ViewId::TeleMemory => "Memory",
            ViewId::TeleGc => "GC Activity",
            ViewId::TeleClasses => "Classes",
            ViewId::TeleThreads => "Threads",
            ViewId::TeleCpuLoad => "CPU Load",
            ViewId::LiveAllObjects => "All Objects",
            ViewId::LiveRecordedObjects => "Recorded Objects",
            ViewId::LiveAllocationCallTree => "Alloc Call Tree",
            ViewId::LiveAllocationHotSpots => "Alloc Hot Spots",
            ViewId::LiveClassTracker => "Class Tracker",
            ViewId::HeapStart => "Start",
            ViewId::HeapClasses => "Classes",
            ViewId::HeapBiggestObjects => "Biggest Objects",
            ViewId::HeapReferences => "References",
            ViewId::CpuCallTree => "Call Tree",
            ViewId::CpuHotSpots => "Hot Spots",
            ViewId::CpuCallGraph => "Call Graph",
            ViewId::CpuOutliers => "Outliers",
            ViewId::CpuComplexity => "Complexity",
            ViewId::CpuTracer => "Call Tracer",
            ViewId::ThreadsHistory => "Thread History",
            ViewId::ThreadsMonitor => "Thread Monitor",
            ViewId::ThreadsDumps => "Thread Dumps",
            ViewId::DatabasesJdbc => "JDBC",
            ViewId::DatabasesJpa => "JPA",
            ViewId::DatabasesMongo => "MongoDB",
            ViewId::DatabasesCassandra => "Cassandra",
        }
    }
}

#[derive(Clone)]
pub struct JvmTarget {
    pub pid: u32,
    pub display_name: String,
    pub main_class: String,
    pub arguments: String,
    pub profiled: bool,
}

#[derive(Clone)]
pub struct OperationLog {
    pub at: String,
    pub level: LogLevel,
    pub action: String,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Ok,
    Warn,
    Error,
}

#[derive(Clone)]
pub struct TelemetrySample {
    pub elapsed_secs: f64,
    pub heap_used_mb: f64,
    pub heap_committed_mb: f64,
    pub process_mb: f64,
    pub private_mb: f64,
    pub cpu_percent: f64,
    pub thread_count: f64,
    pub handle_count: f64,
    pub class_count: f64,
    pub gc_count: f64,
    pub gc_time_ms: f64,
}

#[derive(Clone)]
pub struct ProcessSnapshot {
    pub cpu_seconds: f64,
    pub working_set_mb: f64,
    pub private_mb: f64,
    pub thread_count: u64,
    pub handle_count: u64,
}

#[derive(Clone)]
pub struct HeapSnapshot {
    pub used_mb: f64,
    pub committed_mb: f64,
}

#[derive(Clone)]
pub struct GcSnapshot {
    pub count: u64,
    pub time_ms: u64,
}

#[derive(Clone)]
pub struct MemoryClassRow {
    pub rank: usize,
    pub name: String,
    pub instances: u64,
    pub bytes: u64,
    pub delta_instances: i64,
    pub delta_bytes: i64,
}

#[derive(Clone)]
pub struct AllocationHotSpotRow {
    pub node_id: i32,
    pub parent_id: Option<i32>,
    pub depth: usize,
    pub name: String,
    pub allocated_type: String,
    pub bytes: u64,
    pub allocations: u64,
}

#[derive(Clone)]
pub struct LiveMemorySnapshot {
    pub rows: Vec<MemoryClassRow>,
    pub allocation_hotspots: Vec<AllocationHotSpotRow>,
}

#[derive(Clone)]
pub struct HeapSnapshotInfo {
    pub id: usize,
    pub created_at: String,
    pub path: Option<PathBuf>,
    pub classes: Vec<MemoryClassRow>,
    pub total_instances: u64,
    pub total_bytes: u64,
}

#[derive(Clone)]
pub struct StackTraceElement {
    pub class_name: String,
    pub method_name: String,
    pub descriptor: String,
    pub file_name: Option<String>,
    pub line_number: Option<u32>,
    pub native_method: bool,
    pub raw: String,
}

impl StackTraceElement {
    pub fn full_name(&self) -> String {
        if self.descriptor.is_empty() {
            format!("{}.{}", self.class_name, self.method_name)
        } else {
            format!(
                "{}.{}{}",
                self.class_name, self.method_name, self.descriptor
            )
        }
    }

    pub fn location(&self) -> String {
        match (self.file_name.as_ref(), self.line_number) {
            (Some(file), Some(line)) => format!("({}:{})", file, line),
            (Some(file), None) => format!("({})", file),
            (None, Some(line)) => format!("Unknown:{}", line),
            (None, None) if self.native_method => "(Native Method)".to_string(),
            (None, None) => "(Unknown Source)".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct ThreadInfo {
    pub name: String,
    pub state: String,
    pub nid: String,
    pub top_frame: String,
    pub frames: Vec<String>,
    pub stack_trace: Vec<StackTraceElement>,
    pub lock_info: Option<LockInfo>,
}

#[derive(Clone)]
pub struct LockInfo {
    pub lock_class: String,
    pub lock_identity: u64,
    pub waiting_on: Option<String>,
    pub owning: Option<String>,
}

#[derive(Clone)]
pub struct ThreadDump {
    pub captured_at: String,
    pub raw: String,
    pub threads: Vec<ThreadInfo>,
}

#[derive(Clone)]
pub struct ThreadSummary {
    pub elapsed_secs: f64,
    pub runnable: usize,
    pub waiting: usize,
    pub timed_waiting: usize,
    pub blocked: usize,
    pub other: usize,
}

#[derive(Clone)]
pub struct CpuMethodRow {
    pub method_id: Option<i32>,
    pub method: String,
    pub total_samples: u64,
    pub self_samples: u64,
    pub total_ms: f64,
    pub self_ms: f64,
    pub percent: f32,
    pub class_name: String,
    pub method_name: String,
    pub descriptor: String,
    pub invocations: u64,
    pub average_nanos: f64,
}

#[derive(Clone)]
pub struct CpuMethodEdgeRow {
    pub from_method_id: i32,
    pub to_method_id: i32,
    pub call_count: u64,
    pub total_duration_nano: u64,
}

#[derive(Clone)]
pub struct CallTreeNode {
    pub id: usize,
    pub parent_id: Option<usize>,
    pub class_name: String,
    pub method_name: String,
    pub descriptor: String,
    pub file_name: Option<String>,
    pub line_number: Option<u32>,
    pub self_duration_ms: f64,
    pub total_duration_ms: f64,
    pub call_count: u64,
    pub children: Vec<usize>,
}

impl CallTreeNode {
    pub fn full_name(&self) -> String {
        if self.descriptor.is_empty() {
            format!("{}.{}", self.class_name, self.method_name)
        } else {
            format!(
                "{}.{}{}",
                self.class_name, self.method_name, self.descriptor
            )
        }
    }

    pub fn location(&self) -> String {
        match (self.file_name.as_ref(), self.line_number) {
            (Some(file), Some(line)) => format!("({}:{})", file, line),
            (Some(file), None) => format!("({})", file),
            (None, Some(line)) => format!("Unknown:{}", line),
            (None, None) => "(Unknown Source)".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct MethodEdge {
    pub from_id: usize,
    pub to_id: usize,
    pub call_count: u64,
    pub total_duration_ms: f64,
}

#[derive(Clone)]
pub struct CallTree {
    pub nodes: Vec<CallTreeNode>,
    pub edges: Vec<MethodEdge>,
}

impl CallTree {
    pub fn root_nodes(&self) -> Vec<&CallTreeNode> {
        self.nodes
            .iter()
            .filter(|n| n.parent_id.is_none())
            .collect()
    }

    pub fn children_of(&self, node_id: usize) -> Vec<&CallTreeNode> {
        self.nodes[node_id]
            .children
            .iter()
            .map(|&id| &self.nodes[id])
            .collect()
    }
}

#[derive(Clone)]
pub struct StackSample {
    pub thread: String,
    pub state: String,
    pub frames: Vec<String>,
    pub stack_trace: Vec<StackTraceElement>,
    pub captured_at: String,
    pub elapsed_secs: f64,
}

#[derive(Clone)]
pub struct DatabaseEvent {
    pub at: String,
    pub probe: String,
    pub operation: String,
    pub duration_ms: Option<f64>,
    pub status: String,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JfrRecordingState {
    Idle,
    Running,
    Stopped,
}
