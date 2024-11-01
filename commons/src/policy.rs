use crate::graph::Graph;
use crate::metadata;
use std::collections::HashSet;

/// Prune outgoing edges from "deadend" nodes.
pub fn filter_deadends(input: Graph) -> Graph {
    let mut graph = input;
    let mut deadends = HashSet::new();

    for (index, release) in graph.nodes.iter().enumerate() {
        if release.metadata.get(metadata::DEADEND) == Some(&"true".into()) {
            deadends.insert(index);
        }
    }

    graph.edges.retain(|(from, _to)| {
        let index = *from as usize;
        !deadends.contains(&index)
    });
    graph.edges.shrink_to_fit();

    graph
}

/// Conditionally prune incoming edges towards throttled rollouts.
pub fn throttle_rollouts(input: Graph, client_wariness: f64) -> Graph {
    let mut graph = input;
    let mut hidden = HashSet::new();
    let now = chrono::Utc::now().timestamp();

    for (index, release) in graph.nodes.iter().enumerate() {
        // Skip if this release is not being rolled out.
        if !release.metadata.contains_key(metadata::ROLLOUT) {
            continue;
        };

        // Start epoch defaults to 0.
        let start_epoch = match release.metadata.get(metadata::START_EPOCH) {
            Some(epoch) => epoch.parse::<i64>().unwrap_or(0),
            None => 0i64,
        };

        // Start value defaults to 0.0.
        let start_value = match release.metadata.get(metadata::START_VALUE) {
            Some(val) => val.parse::<f64>().unwrap_or(0f64),
            None => 0f64,
        };

        // Duration has no default (i.e. no progress).
        let mut minutes: Option<u64> = None;
        if let Some(mins) = release.metadata.get(metadata::DURATION) {
            if let Ok(m) = mins.parse::<u64>() {
                minutes = Some(m.max(1));
            }
        }

        let throttling: f64;
        if let Some(mins) = minutes {
            let end = start_epoch + (mins.saturating_mul(60)) as i64;
            let rate = (1.0 - start_value) / (end.saturating_sub(start_epoch)) as f64;
            if now < start_epoch {
                throttling = 0.0;
            } else if now > end {
                throttling = 1.0;
            } else {
                throttling = start_value + rate * (now - start_epoch) as f64;
            }
        } else {
            // Without duration, rollout does not progress past initial value.
            if now < start_epoch {
                throttling = 0.0;
            } else {
                throttling = start_value
            }
        }

        if client_wariness > throttling {
            hidden.insert(index);
        }
    }

    graph.edges.retain(|(_from, to)| {
        let index = *to as usize;
        !hidden.contains(&index)
    });
    graph.edges.shrink_to_fit();

    graph
}
