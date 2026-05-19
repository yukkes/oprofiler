use std::collections::VecDeque;

use super::model::*;
pub use super::platform::hidden_command;

use std::sync::mpsc;

pub fn parse_jps_line(line: &str) -> Option<JvmTarget> {
    let mut parts = line.split_whitespace();
    let pid = parts.next()?.parse::<u32>().ok()?;
    let main = parts.next().unwrap_or("<unknown>").to_string();
    let rest = parts.collect::<Vec<_>>().join(" ");
    if main.contains("Jps")
        || main.contains("sun.tools.jps")
        || main.contains("jdk.jcmd")
        || main.contains("sun.tools.jcmd")
        || is_profiler_ui_process(&main, &rest)
    {
        return None;
    }
    let display_name = if main.ends_with(".jar") {
        std::path::PathBuf::from(&main)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| main.clone())
    } else {
        main.rsplit('.').next().unwrap_or(&main).to_string()
    };
    let profiled =
        rest.contains("oprofiler") || rest.contains("oprofilerti") || main.contains("oprofiler");
    Some(JvmTarget {
        pid,
        display_name,
        main_class: main,
        arguments: rest,
        profiled,
    })
}

fn is_profiler_ui_process(main: &str, arguments: &str) -> bool {
    let text = format!("{main} {arguments}").to_lowercase();
    text.contains("oprofiler.exe") || text.contains("oprofiler.jar")
}

pub fn is_likely_demo_server(jvm: &JvmTarget) -> bool {
    let text = format!("{} {}", jvm.main_class, jvm.arguments).to_lowercase();
    text.contains("demo") || text.contains("server")
}

pub fn query_process(pid: u32) -> Option<ProcessSnapshot> {
    let script = format!(
        "$p=Get-Process -Id {pid} -ErrorAction Stop; Write-Output ($p.CPU.ToString() + '|' + $p.WorkingSet64.ToString() + '|' + $p.PrivateMemorySize64.ToString() + '|' + $p.Threads.Count.ToString() + '|' + $p.HandleCount.ToString())"
    );
    let output = hidden_command("powershell")
        .args(["-NoProfile", "-Command", script.as_str()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.trim().split('|');
    let cpu_seconds = parts.next()?.trim().parse().ok()?;
    let working_set = parts.next()?.trim().parse::<f64>().ok()?;
    let private = parts.next()?.trim().parse::<f64>().ok()?;
    let thread_count = parts.next()?.trim().parse().ok()?;
    let handle_count = parts.next()?.trim().parse().ok()?;
    Some(ProcessSnapshot {
        cpu_seconds,
        working_set_mb: working_set / 1024.0 / 1024.0,
        private_mb: private / 1024.0 / 1024.0,
        thread_count,
        handle_count,
    })
}

pub fn query_heap(pid: u32) -> Option<HeapSnapshot> {
    let output = hidden_command("jcmd")
        .arg(pid.to_string())
        .arg("GC.heap_info")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut used_k = 0.0;
    let mut total_k = 0.0;
    for line in text.lines() {
        if line.contains("used ") || line.contains("used=") {
            used_k += extract_k_value_after(line, "used ")
                .or_else(|| extract_k_value_after(line, "used="))
                .unwrap_or(0.0);
        }
        if line.contains("total ") || line.contains("committed ") {
            total_k += extract_k_value_after(line, "total ")
                .or_else(|| extract_k_value_after(line, "committed "))
                .unwrap_or(0.0);
        }
    }
    Some(HeapSnapshot {
        used_mb: used_k / 1024.0,
        committed_mb: total_k / 1024.0,
    })
}

fn extract_k_value_after(line: &str, marker: &str) -> Option<f64> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let token = rest
        .split(|c: char| c == ',' || c.is_whitespace())
        .find(|part| !part.is_empty())?
        .trim_end_matches('K')
        .trim_end_matches('k');
    token.parse::<f64>().ok()
}

pub fn query_gc(pid: u32) -> Option<GcSnapshot> {
    let output = hidden_command("jcmd")
        .arg(pid.to_string())
        .arg("PerfCounter.print")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut count = 0;
    let mut time_ms = 0;
    for line in text.lines() {
        if line.contains("sun.gc.collector") && line.contains("invocations") {
            if let Some(value) = line
                .split_whitespace()
                .last()
                .and_then(|v| v.parse::<u64>().ok())
            {
                count += value;
            }
        }
        if line.contains("sun.gc.collector") && line.contains(".time") {
            if let Some(value) = line
                .split_whitespace()
                .last()
                .and_then(|v| v.parse::<u64>().ok())
            {
                time_ms += value / 1_000_000;
            }
        }
    }
    Some(GcSnapshot { count, time_ms })
}

pub fn query_class_histogram(pid: u32) -> Result<Vec<MemoryClassRow>, String> {
    let output = hidden_command("jcmd")
        .arg(pid.to_string())
        .arg("GC.class_histogram")
        .output()
        .map_err(|err| format!("failed to execute jcmd GC.class_histogram: {err}"))?;
    if !output.status.success() {
        return Err(command_output_message("jcmd GC.class_histogram", &output));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let mut parts = trimmed.split_whitespace();
        let rank_token = parts.next().unwrap_or_default();
        if !rank_token.ends_with(':') {
            continue;
        }
        let rank = rank_token
            .trim_end_matches(':')
            .parse::<usize>()
            .unwrap_or(rows.len() + 1);
        let instances = parts
            .next()
            .and_then(|p| p.parse::<u64>().ok())
            .unwrap_or(0);
        let bytes = parts
            .next()
            .and_then(|p| p.parse::<u64>().ok())
            .unwrap_or(0);
        let name = parts.collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            continue;
        }
        rows.push(MemoryClassRow {
            rank,
            name,
            instances,
            bytes,
            delta_instances: 0,
            delta_bytes: 0,
        });
    }
    if rows.is_empty() {
        Err("class histogram returned no rows".to_string())
    } else {
        Ok(rows)
    }
}

pub fn query_thread_dump(pid: u32) -> Result<ThreadDump, String> {
    let output = hidden_command("jcmd")
        .arg(pid.to_string())
        .arg("Thread.print")
        .arg("-l")
        .output()
        .map_err(|err| format!("failed to execute jcmd Thread.print: {err}"))?;
    if !output.status.success() {
        return Err(command_output_message("jcmd Thread.print", &output));
    }
    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    let threads = parse_thread_dump(&raw);
    let captured_at = chrono::Local::now().format("%H:%M:%S").to_string();
    Ok(ThreadDump {
        captured_at,
        raw,
        threads,
    })
}

pub fn parse_thread_dump(raw: &str) -> Vec<ThreadInfo> {
    let mut threads = Vec::new();
    let mut current: Option<ThreadInfo> = None;
    for line in raw.lines() {
        if line.starts_with('"') {
            if let Some(thread) = current.take() {
                threads.push(thread);
            }
            let name = line.split('"').nth(1).unwrap_or("<unnamed>").to_string();
            let nid = line
                .split_whitespace()
                .find(|part| part.starts_with("nid="))
                .map(|part| part.trim_start_matches("nid=").to_string())
                .unwrap_or_else(|| "-".to_string());
            current = Some(ThreadInfo {
                name,
                state: "UNKNOWN".to_string(),
                nid,
                top_frame: String::new(),
                frames: Vec::new(),
                stack_trace: Vec::new(),
                lock_info: None,
            });
        } else if let Some(thread) = current.as_mut() {
            let trimmed = line.trim();
            if let Some(state) = trimmed.strip_prefix("java.lang.Thread.State:") {
                thread.state = state
                    .split_whitespace()
                    .next()
                    .unwrap_or("UNKNOWN")
                    .to_string();
            } else if let Some(frame_str) = trimmed.strip_prefix("at ") {
                let frame = frame_str.to_string();
                let ste = parse_stack_trace_element(frame_str);
                if thread.top_frame.is_empty() {
                    thread.top_frame = frame.clone();
                }
                thread.frames.push(frame);
                thread.stack_trace.push(ste);
            } else if trimmed.starts_with("- waiting on")
                || trimmed.starts_with("- waiting to lock")
                || trimmed.starts_with("- locked")
                || trimmed.starts_with("   locking")
            {
                let lock_class =
                    extract_lock_class(trimmed).unwrap_or_else(|| "Unknown".to_string());
                let lock_identity = extract_lock_identity(trimmed).unwrap_or(0);
                let waiting_on = if trimmed.starts_with("- waiting") {
                    Some(trimmed.to_string())
                } else {
                    None
                };
                let owning = if trimmed.starts_with("- locked") || trimmed.starts_with("   locking")
                {
                    Some(trimmed.to_string())
                } else {
                    None
                };
                if thread.lock_info.is_none() {
                    thread.lock_info = Some(LockInfo {
                        lock_class,
                        lock_identity,
                        waiting_on,
                        owning,
                    });
                }
            }
        }
    }
    if let Some(thread) = current.take() {
        threads.push(thread);
    }
    threads
}

fn parse_stack_trace_element(frame: &str) -> StackTraceElement {
    let frame = frame.trim();
    let mut class_name = String::new();
    let mut descriptor = String::new();
    let mut file_name = None;
    let mut line_number = None;
    let mut native_method = false;

    let (before_paren, in_paren) = if let Some(open_idx) = frame.rfind('(') {
        let close_idx = frame.rfind(')').unwrap_or(frame.len());
        (&frame[..open_idx], &frame[open_idx + 1..close_idx])
    } else {
        (frame, "")
    };

    if in_paren == "Native Method" {
        native_method = true;
    } else if let Some(colon) = in_paren.rfind(':') {
        file_name = Some(in_paren[..colon].to_string());
        line_number = in_paren[colon + 1..].parse().ok();
    } else if !in_paren.is_empty() {
        file_name = Some(in_paren.to_string());
    }

    let method_name = if let Some(last_dot) = before_paren.rfind('.') {
        class_name = before_paren[..last_dot].to_string();
        let mut name = before_paren[last_dot + 1..].to_string();
        if let Some(paren_pos) = name.find('(') {
            descriptor = name[paren_pos..].to_string();
            name = name[..paren_pos].to_string();
        }
        name
    } else {
        before_paren.to_string()
    };

    StackTraceElement {
        class_name,
        method_name,
        descriptor,
        file_name,
        line_number,
        native_method,
        raw: frame.to_string(),
    }
}

fn extract_lock_class(line: &str) -> Option<String> {
    let start = line.find("(a ")?;
    let rest = &line[start + 3..];
    let end = rest.find(')')?;
    Some(rest[..end].to_string())
}

fn extract_lock_identity(line: &str) -> Option<u64> {
    let start = line.find("<0x")?;
    let rest = &line[start + 3..];
    let end = rest.find('>')?;
    u64::from_str_radix(&rest[..end], 16).ok()
}

pub fn command_output_message(command: &str, output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut message = format!("{command} exited with {}", output.status);
    let detail = if !stderr.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    if !detail.is_empty() {
        message.push_str(": ");
        message.push_str(detail);
    }
    message
}

pub fn append_command_output(
    logs: &mut VecDeque<OperationLog>,
    action: &str,
    output: &std::process::Output,
) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut lines: Vec<String> = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|line| !line.trim().is_empty())
        .take(8)
        .map(|line| line.trim().to_string())
        .collect();
    if lines.is_empty() {
        return;
    }
    lines.reverse();
    for line in lines {
        logs.push_front(OperationLog {
            at: chrono::Local::now().format("%H:%M:%S").to_string(),
            level: LogLevel::Info,
            action: action.to_string(),
            message: line,
        });
    }
}

pub fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

pub fn start_jfr_recording(pid: u32) -> Result<String, String> {
    let output = start_jfr_recording_with_args(&[
        pid.to_string().as_str(),
        "JFR.start",
        "name=openprofiler",
        "settings=profile",
    ])?;
    let output = if output.status.success() {
        output
    } else {
        start_jfr_recording_with_args(&[
            pid.to_string().as_str(),
            "JFR.start",
            "name=openprofiler",
            "settings=profile",
        ])?
    };
    if !output.status.success() {
        return Err(command_output_message("jcmd JFR.start", &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn start_jfr_recording_with_args(args: &[&str]) -> Result<std::process::Output, String> {
    hidden_command("jcmd")
        .args(args)
        .output()
        .map_err(|e| format!("failed to start JFR: {e}"))
}

pub fn dump_jfr_recording(pid: u32, path: &str) -> Result<String, String> {
    let output = hidden_command("jcmd")
        .args([
            pid.to_string().as_str(),
            "JFR.dump",
            "name=openprofiler",
            &format!("filename={path}"),
        ])
        .output()
        .map_err(|e| format!("failed to dump JFR: {e}"))?;
    if !output.status.success() {
        return Err(command_output_message("jcmd JFR.dump", &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn stop_jfr_recording(pid: u32) -> Result<String, String> {
    let output = hidden_command("jcmd")
        .args([pid.to_string().as_str(), "JFR.stop", "name=openprofiler"])
        .output()
        .map_err(|e| format!("failed to stop JFR: {e}"))?;
    if !output.status.success() {
        return Err(command_output_message("jcmd JFR.stop", &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn parse_jfr_execution_samples(path: &str) -> Result<Vec<CpuMethodRow>, String> {
    let output = hidden_command("jfr")
        .args(["print", "--events", "jdk.ExecutionSample", path])
        .output()
        .map_err(|e| format!("failed to run jfr print: {e}"))?;
    if !output.status.success() {
        return Err(command_output_message("jfr print", &output));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut counts: std::collections::HashMap<String, (u64, u64, String, String)> =
        std::collections::HashMap::new();

    let mut in_event = false;
    let mut in_stack = false;
    let mut stack_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "jdk.ExecutionSample {" {
            in_event = true;
            in_stack = false;
            stack_lines.clear();
            continue;
        }
        if !in_event {
            continue;
        }
        if trimmed.starts_with("stackTrace = [") {
            in_stack = true;
            continue;
        }
        if in_stack && trimmed == "]" {
            in_stack = false;
            // Process leaf frame (first element in JFR stackTrace is the top frame)
            if let Some(leaf) = stack_lines.first() {
                let ste = parse_stack_trace_element(leaf);
                let key = ste.full_name();
                let entry = counts.entry(key).or_insert((
                    0,
                    0,
                    ste.class_name.clone(),
                    ste.method_name.clone(),
                ));
                entry.0 += 1; // samples
                entry.1 += 1; // invocations (estimated from samples)
            }
            in_event = false;
            stack_lines.clear();
            continue;
        }
        if in_stack {
            stack_lines.push(trimmed.to_string());
        }
        if trimmed == "}" && !in_stack {
            in_event = false;
            stack_lines.clear();
        }
    }

    let total_samples = counts.values().map(|v| v.0).sum::<u64>().max(1);
    let sample_interval_ns = 20_000_000u64; // 20ms default for profile settings

    let mut rows: Vec<CpuMethodRow> = counts
        .into_iter()
        .map(
            |(method, (samples, invocations, class_name, method_name))| {
                let self_nanos = samples.saturating_mul(sample_interval_ns);
                let percent = (samples as f64 / total_samples as f64) as f32;
                CpuMethodRow {
                    method_id: None,
                    method,
                    total_samples: samples,
                    self_samples: samples,
                    total_ms: self_nanos as f64 / 1_000_000.0,
                    self_ms: self_nanos as f64 / 1_000_000.0,
                    percent,
                    class_name,
                    method_name,
                    descriptor: String::new(),
                    invocations,
                    average_nanos: if invocations > 0 {
                        self_nanos as f64 / invocations as f64
                    } else {
                        0.0
                    },
                }
            },
        )
        .collect();

    rows.sort_by(|a, b| b.self_samples.cmp(&a.self_samples));
    rows.truncate(250);
    Ok(rows)
}

pub enum QueryResult {
    Jvms(Vec<JvmTarget>),
    JvmsError(String),
    Telemetry {
        process: Option<ProcessSnapshot>,
        heap: Option<HeapSnapshot>,
        gc: Option<GcSnapshot>,
        elapsed_secs: f64,
        cpu_seconds: Option<f64>,
    },
    LiveMemory(Result<LiveMemorySnapshot, String>),
    CpuSample {
        result: Result<ThreadDump, String>,
        thread_status: String,
    },
    CpuInstrumentation(Result<(Vec<CpuMethodRow>, Vec<CpuMethodEdgeRow>), String>),
    ThreadDump(Result<ThreadDump, String>),
    HeapSnapshot {
        success: bool,
        message: String,
        path: Option<std::path::PathBuf>,
        pid: u32,
    },
    CpuRecording {
        success: bool,
        action: String,
        message: String,
        pid: u32,
        port: u16,
    },
    Gc {
        success: bool,
        message: String,
        pid: u32,
    },
}

pub struct QueryHandle {
    rx: mpsc::Receiver<QueryResult>,
}

impl QueryHandle {
    pub fn spawn<F>(f: F) -> Self
    where
        F: FnOnce() -> QueryResult + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = f();
            let _ = tx.send(result);
        });
        Self { rx }
    }

    pub fn poll(&self) -> Option<QueryResult> {
        self.rx.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::parse_jps_line;

    #[test]
    fn parse_jps_line_excludes_profiler_ui_and_tools() {
        assert!(parse_jps_line(
            r#"10640 D:\Work\jpro_base2\example\OProfiler14.0.6\bin\OProfiler.exe -Dexe4j.moduleName=D:\Work\jpro_base2\example\OProfiler14.0.6\bin\OProfiler.exe"#
        )
        .is_none());
        assert!(parse_jps_line(
            r#"16720 jdk.jcmd/sun.tools.jps.Jps -Dapplication.home=C:\Program Files\Amazon Corretto\jdk17.0.19_10"#
        )
        .is_none());

        let target = parse_jps_line(
            r#"9996 com.ejt.demo.server.gui.GuiDemoServer -agentpath:D:\x\oprofilerti.dll=port=63881"#,
        )
        .expect("demo server should be retained");
        assert_eq!(target.pid, 9996);
        assert_eq!(target.display_name, "GuiDemoServer");
    }
}
