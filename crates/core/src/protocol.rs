use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use super::model::{AllocationHotSpotRow, CpuMethodEdgeRow, CpuMethodRow};
use crate::proto::com_openprofiler_protocol::command;
use crate::proto::com_openprofiler_protocol::profiling_data;
use crate::proto::com_openprofiler_protocol::start_cpu_recording_command;
use crate::proto::com_openprofiler_protocol::*;
use prost::Message;

pub struct AgentClient {
    stream: TcpStream,
}

pub fn cpu_rows_from_agent_data(cpu_data: &CpuData) -> (Vec<CpuMethodRow>, Vec<CpuMethodEdgeRow>) {
    let mut rows: Vec<CpuMethodRow> = cpu_data
        .hot_spots
        .iter()
        .map(|hs| {
            let method_id = cpu_data.method_graph.as_ref().and_then(|graph| {
                graph.nodes.iter().find_map(|node| {
                    if node.class_name == hs.class_name
                        && node.method_name == hs.method_name
                        && node.method_descriptor == hs.method_descriptor
                    {
                        Some(node.id)
                    } else {
                        None
                    }
                })
            });
            let self_ms = hs.self_duration_nano as f64 / 1_000_000.0;
            let total_ms = hs.total_duration_nano as f64 / 1_000_000.0;
            let average_nanos = if hs.invocations > 0 {
                hs.self_duration_nano as f64 / hs.invocations as f64
            } else {
                0.0
            };
            CpuMethodRow {
                method_id,
                method: format!(
                    "{}.{}{}",
                    hs.class_name, hs.method_name, hs.method_descriptor
                ),
                total_samples: hs.total_samples,
                self_samples: hs.self_samples,
                total_ms,
                self_ms,
                percent: (hs.percent / 100.0) as f32,
                class_name: hs.class_name.clone(),
                method_name: hs.method_name.clone(),
                descriptor: hs.method_descriptor.clone(),
                invocations: hs.invocations,
                average_nanos,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.self_ms
            .partial_cmp(&a.self_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows.truncate(250);

    let edges = cpu_data
        .method_graph
        .as_ref()
        .map(|graph| {
            graph
                .edges
                .iter()
                .map(|edge| CpuMethodEdgeRow {
                    from_method_id: edge.from_node_id,
                    to_method_id: edge.to_node_id,
                    call_count: edge.call_count,
                    total_duration_nano: edge.total_duration_nano,
                })
                .collect()
        })
        .unwrap_or_default();

    (rows, edges)
}

pub fn allocation_hotspots_from_agent_data(memory_data: MemoryData) -> Vec<AllocationHotSpotRow> {
    let Some(tree) = memory_data.allocation_tree else {
        return Vec::new();
    };
    let nodes = tree.nodes;
    nodes
        .iter()
        .map(|node| {
            let class_name = &node.class_name;
            let method_name = &node.method_name;
            let descriptor = &node.method_descriptor;
            let allocated_type = node.allocated_type.replace('/', ".");
            let name = if class_name.is_empty() && method_name.is_empty() {
                allocated_type.clone()
            } else {
                format!("{class_name}.{method_name}{descriptor}")
            };
            AllocationHotSpotRow {
                node_id: node.id,
                parent_id: (node.parent_id >= 0).then_some(node.parent_id),
                depth: allocation_depth(node.parent_id, &nodes),
                name,
                allocated_type,
                bytes: node.allocated_size,
                allocations: node.instance_count,
            }
        })
        .collect()
}

fn allocation_depth(mut parent_id: i32, nodes: &[AllocationTreeNode]) -> usize {
    let mut depth = 0usize;
    while parent_id >= 0 && depth < 16 {
        depth += 1;
        let Some(parent) = nodes.iter().find(|node| node.id == parent_id) else {
            break;
        };
        parent_id = parent.parent_id;
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_rows_preserve_invocations_and_sql_marker() {
        let cpu = CpuData {
            total_sample_count: 0,
            sampling_interval_ms: 10,
            method_graph: Some(MethodGraph {
                nodes: vec![MethodNode {
                    id: 7,
                    class_name: "SQL".to_string(),
                    method_name: "SELECT 1".to_string(),
                    method_descriptor: String::new(),
                    execution_count: 3,
                    total_duration_nano: 9_000_000,
                    self_duration_nano: 9_000_000,
                }],
                edges: Vec::new(),
            }),
            call_tree: None,
            hot_spots: vec![HotSpot {
                class_name: "SQL".to_string(),
                method_name: "SELECT 1".to_string(),
                method_descriptor: String::new(),
                self_samples: 9,
                total_samples: 9,
                self_duration_nano: 9_000_000,
                total_duration_nano: 9_000_000,
                percent: 100.0,
                invocations: 3,
            }],
        };

        let (rows, edges) = cpu_rows_from_agent_data(&cpu);

        assert!(edges.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].method_id, Some(7));
        assert_eq!(rows[0].class_name, "SQL");
        assert_eq!(rows[0].invocations, 3);
        assert_eq!(rows[0].average_nanos, 3_000_000.0);
    }

    #[test]
    fn allocation_rows_preserve_parent_depth() {
        let memory = MemoryData {
            allocation_tree: Some(AllocationTree {
                nodes: vec![
                    AllocationTreeNode {
                        id: 1,
                        parent_id: -1,
                        class_name: "app.Service".to_string(),
                        method_name: "run".to_string(),
                        method_descriptor: "()V".to_string(),
                        file_name: String::new(),
                        line_number: 0,
                        allocated_type: "java/lang/String".to_string(),
                        allocated_size: 64,
                        instance_count: 1,
                    },
                    AllocationTreeNode {
                        id: 2,
                        parent_id: 1,
                        class_name: "app.Repository".to_string(),
                        method_name: "load".to_string(),
                        method_descriptor: "()V".to_string(),
                        file_name: String::new(),
                        line_number: 0,
                        allocated_type: "java/lang/Object".to_string(),
                        allocated_size: 32,
                        instance_count: 2,
                    },
                ],
            }),
            heap_object_infos: Vec::new(),
            heap_used_bytes: 0,
            heap_committed_bytes: 0,
        };

        let rows = allocation_hotspots_from_agent_data(memory);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].parent_id, Some(1));
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[1].allocated_type, "java.lang.Object");
    }
}

impl AgentClient {
    pub fn connect(addr: SocketAddr) -> Result<Self, String> {
        Self::connect_with_timeout(addr, Duration::from_secs(5), Duration::from_secs(30))
    }

    pub fn connect_with_timeout(
        addr: SocketAddr,
        connect_timeout: Duration,
        io_timeout: Duration,
    ) -> Result<Self, String> {
        let stream = TcpStream::connect_timeout(&addr, connect_timeout)
            .map_err(|e| format!("TCP connect failed: {e}"))?;
        stream
            .set_read_timeout(Some(io_timeout))
            .map_err(|e| format!("set_read_timeout failed: {e}"))?;
        stream
            .set_write_timeout(Some(io_timeout))
            .map_err(|e| format!("set_write_timeout failed: {e}"))?;
        Ok(Self { stream })
    }

    pub fn send_command(&mut self, cmd: Command) -> Result<ProfilingData, String> {
        write_message(&mut self.stream, &cmd).map_err(|e| format!("Write failed: {e}"))?;
        read_message(&mut self.stream).map_err(|e| format!("Read failed: {e}"))
    }

    pub fn send_command_allow_response_timeout(&mut self, cmd: Command) -> Result<(), String> {
        write_message(&mut self.stream, &cmd).map_err(|e| format!("Write failed: {e}"))?;
        match read_message(&mut self.stream) {
            Ok(_) => Ok(()),
            Err(ReadMessageError::Io(err)) if is_timeout_error(&err) => Ok(()),
            Err(err) => Err(format!("Read failed: {err}")),
        }
    }

    pub fn get_cpu_data(&mut self) -> Result<CpuData, String> {
        let cmd = Command {
            r#type: command::CommandType::GetCpuData as i32,
            session_id: String::new(),
            payload: Some(command::Payload::GetCpuData(GetCpuDataCommand {
                thread_filter: String::new(),
            })),
        };
        let data = self.send_command(cmd)?;
        match data.payload {
            Some(profiling_data::Payload::CpuData(cpu)) => Ok(cpu),
            _ => Err("Unexpected response type".to_string()),
        }
    }

    pub fn get_memory_data(&mut self) -> Result<MemoryData, String> {
        let cmd = Command {
            r#type: command::CommandType::GetMemoryData as i32,
            session_id: String::new(),
            payload: None,
        };
        let data = self.send_command(cmd)?;
        match data.payload {
            Some(profiling_data::Payload::MemoryData(memory)) => Ok(memory),
            _ => Err("Unexpected response type".to_string()),
        }
    }

    pub fn start_cpu_recording(&mut self) -> Result<(), String> {
        let cmd = Command {
            r#type: command::CommandType::StartCpuRecording as i32,
            session_id: String::new(),
            payload: Some(command::Payload::StartCpuRecording(
                StartCpuRecordingCommand {
                    mode: start_cpu_recording_command::CpuMode::Instrumentation as i32,
                    sampling_interval_ms: 10,
                },
            )),
        };
        self.send_command(cmd)?;
        Ok(())
    }

    pub fn start_cpu_recording_fast(&mut self) -> Result<(), String> {
        let cmd = Command {
            r#type: command::CommandType::StartCpuRecording as i32,
            session_id: String::new(),
            payload: Some(command::Payload::StartCpuRecording(
                StartCpuRecordingCommand {
                    mode: start_cpu_recording_command::CpuMode::Instrumentation as i32,
                    sampling_interval_ms: 10,
                },
            )),
        };
        self.send_command_allow_response_timeout(cmd)
    }

    pub fn stop_cpu_recording(&mut self) -> Result<(), String> {
        let cmd = Command {
            r#type: command::CommandType::StopCpuRecording as i32,
            session_id: String::new(),
            payload: None,
        };
        self.send_command(cmd)?;
        Ok(())
    }

    pub fn stop_cpu_recording_fast(&mut self) -> Result<(), String> {
        let cmd = Command {
            r#type: command::CommandType::StopCpuRecording as i32,
            session_id: String::new(),
            payload: None,
        };
        self.send_command_allow_response_timeout(cmd)
    }
}

fn write_message(stream: &mut TcpStream, msg: &impl prost::Message) -> Result<(), std::io::Error> {
    let data = msg.encode_to_vec();
    let len = data.len() as u32;
    stream.write_all(&[
        ((len >> 24) & 0xFF) as u8,
        ((len >> 16) & 0xFF) as u8,
        ((len >> 8) & 0xFF) as u8,
        (len & 0xFF) as u8,
    ])?;
    stream.write_all(&data)?;
    stream.flush()?;
    Ok(())
}

enum ReadMessageError {
    Io(std::io::Error),
    Decode(prost::DecodeError),
    TooLarge(u32),
}

impl std::fmt::Display for ReadMessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadMessageError::Io(err) => write!(f, "{err}"),
            ReadMessageError::Decode(err) => write!(f, "{err}"),
            ReadMessageError::TooLarge(len) => write!(f, "Message too large: {len}"),
        }
    }
}

fn is_timeout_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )
}

fn read_message(stream: &mut TcpStream) -> Result<ProfilingData, ReadMessageError> {
    let mut len_bytes = [0u8; 4];
    stream
        .read_exact(&mut len_bytes)
        .map_err(ReadMessageError::Io)?;
    let len = ((len_bytes[0] as u32) << 24)
        | ((len_bytes[1] as u32) << 16)
        | ((len_bytes[2] as u32) << 8)
        | (len_bytes[3] as u32);
    if len > 50_000_000 {
        return Err(ReadMessageError::TooLarge(len));
    }
    let mut data = vec![0u8; len as usize];
    stream.read_exact(&mut data).map_err(ReadMessageError::Io)?;
    ProfilingData::decode(&data[..]).map_err(ReadMessageError::Decode)
}
