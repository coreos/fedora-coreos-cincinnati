use crate::graph::Graph;
use crate::metadata;
use failure::{bail, Fallible};

/// Prune outgoing edges from "deadend" nodes.
pub fn filter_deadends(input: Graph) -> Graph {
    use std::collections::HashSet;

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

/// Pick relevant payload for requested basearch.
pub fn pick_basearch(input: Graph, basearch: String) -> Fallible<Graph> {
    let mut graph = input;
    let key = format!("{}.{}", metadata::ARCH_PREFIX, &basearch);

    if basearch != "x86_64" {
        bail!("unexpected basearch '{}", basearch);
    }

    for mut release in &mut graph.nodes {
        if let Some(payload) = release.metadata.remove(&key) {
            release.payload = payload;
            release
                .metadata
                .insert(metadata::SCHEME.to_string(), "checksum".to_string());
        }
        release
            .metadata
            .retain(|k, _| !k.starts_with(metadata::ARCH_PREFIX));
    }

    Ok(graph)
}

/// Conditionally prune incoming edges towards throttled rollouts.
pub fn throttle_rollouts(input: Graph, client_wariness: f64) -> Graph {
    use std::collections::HashSet;

    let mut graph = input;
    let mut hidden = HashSet::new();
    let now = chrono::Utc::now().timestamp();

    for (index, release) in graph.nodes.iter().enumerate() {
        // Skip if this release is not being rolled out.
        if release.metadata.get(metadata::ROLLOUT).is_none() {
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
