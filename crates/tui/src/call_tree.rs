use std::collections::{BTreeMap, VecDeque};

use openprofiler_core::model::*;

pub struct CallTreeConfig {
    pub total_sample_count: u64,
    pub sampling_interval_ms: f64,
}

pub fn build_call_tree(
    samples: &VecDeque<StackSample>,
    thread_filter: &str,
    config: &CallTreeConfig,
) -> CallTree {
    let mut node_map: BTreeMap<String, usize> = BTreeMap::new();
    let mut nodes: Vec<CallTreeNode> = Vec::new();
    let mut edges: Vec<MethodEdge> = Vec::new();
    let mut edge_map: BTreeMap<(usize, usize), usize> = BTreeMap::new();

    let root_id = nodes.len();
    nodes.push(CallTreeNode {
        id: root_id,
        parent_id: None,
        class_name: "root".to_string(),
        method_name: "all".to_string(),
        descriptor: String::new(),
        file_name: None,
        line_number: None,
        self_duration_ms: 0.0,
        total_duration_ms: 0.0,
        call_count: 0,
        children: Vec::new(),
    });
    node_map.insert("root".to_string(), root_id);

    for sample in samples {
        if !thread_filter.is_empty() && thread_filter != "Runnable" && sample.state != "RUNNABLE" {
            continue;
        }

        let mut parent_idx = root_id;
        let frames: Vec<&StackTraceElement> = sample
            .stack_trace
            .iter()
            .filter(|f| !f.class_name.contains("java.lang.Thread.State"))
            .collect();

        let total = frames.len();
        for (i, frame) in frames.into_iter().rev().enumerate() {
            let key = format!(
                "{}::{}{}",
                frame.class_name, frame.method_name, frame.descriptor
            );
            let is_leaf = i == total.saturating_sub(1);

            let node_idx = if let Some(&idx) = node_map.get(&key) {
                let node = &mut nodes[idx];
                node.total_duration_ms += config.sampling_interval_ms;
                if is_leaf {
                    node.self_duration_ms += config.sampling_interval_ms;
                }
                node.call_count += 1;
                idx
            } else {
                let idx = nodes.len();
                nodes.push(CallTreeNode {
                    id: idx,
                    parent_id: Some(parent_idx),
                    class_name: frame.class_name.clone(),
                    method_name: frame.method_name.clone(),
                    descriptor: frame.descriptor.clone(),
                    file_name: frame.file_name.clone(),
                    line_number: frame.line_number,
                    self_duration_ms: if is_leaf {
                        config.sampling_interval_ms
                    } else {
                        0.0
                    },
                    total_duration_ms: config.sampling_interval_ms,
                    call_count: 1,
                    children: Vec::new(),
                });
                node_map.insert(key.clone(), idx);
                if let Some(parent) = nodes.get_mut(parent_idx) {
                    if !parent.children.contains(&idx) {
                        parent.children.push(idx);
                    }
                }
                idx
            };

            if parent_idx != root_id && parent_idx != node_idx {
                let edge_key = (parent_idx, node_idx);
                if let Some(&edge_idx) = edge_map.get(&edge_key) {
                    let edge = &mut edges[edge_idx];
                    edge.call_count += 1;
                    edge.total_duration_ms += config.sampling_interval_ms;
                } else {
                    let edge_idx = edges.len();
                    edges.push(MethodEdge {
                        from_id: parent_idx,
                        to_id: node_idx,
                        call_count: 1,
                        total_duration_ms: config.sampling_interval_ms,
                    });
                    edge_map.insert(edge_key, edge_idx);
                }
            }

            parent_idx = node_idx;
        }
    }

    CallTree { nodes, edges }
}

pub fn build_hot_spots(tree: &CallTree, total_sample_count: u64) -> Vec<CpuMethodRow> {
    let denominator = total_sample_count.max(1);

    tree.nodes
        .iter()
        .skip(1)
        .filter(|n| n.call_count > 0)
        .map(|n| {
            let percent = (n.call_count as f64 / denominator as f64) as f32;
            CpuMethodRow {
                method_id: None,
                method: n.full_name(),
                total_samples: n.call_count,
                self_samples: (n.self_duration_ms / 10.0) as u64,
                total_ms: n.total_duration_ms,
                self_ms: n.self_duration_ms,
                percent,
                class_name: n.class_name.clone(),
                method_name: n.method_name.clone(),
                descriptor: n.descriptor.clone(),
                invocations: n.call_count,
                average_nanos: if n.call_count > 0 {
                    (n.self_duration_ms * 1_000_000.0) / n.call_count as f64
                } else {
                    0.0
                },
            }
        })
        .collect()
}
