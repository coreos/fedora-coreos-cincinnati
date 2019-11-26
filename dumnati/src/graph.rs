use crate::metadata;
use failure::Fallible;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct CincinnatiPayload {
    pub(crate) version: String,
    pub(crate) metadata: HashMap<String, String>,
    pub(crate) payload: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Graph {
    pub(crate) nodes: Vec<CincinnatiPayload>,
    pub(crate) edges: Vec<(u64, u64)>,
}

impl Default for Graph {
    fn default() -> Self {
        Self {
            nodes: vec![],
            edges: vec![],
        }
    }
}

impl Graph {
    pub fn from_metadata(
        releases: Vec<metadata::Release>,
        updates: metadata::UpdatesJSON,
    ) -> Fallible<Self> {
        let nodes = releases
            .into_iter()
            .enumerate()
            .map(|(age_index, entry)| {
                let mut current = CincinnatiPayload {
                    version: entry.version,
                    payload: "".to_string(),
                    metadata: hashmap! {
                        metadata::AGE_INDEX.to_string() => age_index.to_string(),
                    },
                };
                for commit in entry.commits {
                    if commit.architecture.is_empty() || commit.checksum.is_empty() {
                        continue;
                    }
                    let key = format!("{}.{}", metadata::ARCH_PREFIX, commit.architecture);
                    let value = commit.checksum;
                    current.metadata.insert(key, value);
                }

                // Augment with dead-ends metadata.
                Self::inject_deadend_reason(&updates, &mut current);

                // Augment with barriers metadata.
                Self::inject_barrier_reason(&updates, &mut current);

                // Augment with rollouts metadata.
                Self::inject_throttling_params(&updates, &mut current);

                current
            })
            .collect();

        // Compute the update graph.
        let edges = Self::compute_edges(&nodes)?;

        let graph = Graph { nodes, edges };
        Ok(graph)
    }

    fn compute_edges(nodes: &Vec<CincinnatiPayload>) -> Fallible<Vec<(u64, u64)>> {
        use std::collections::BTreeSet;

        // Collect all rollouts and barriers.
        let mut rollouts = BTreeSet::<u64>::new();
        let mut barriers = BTreeSet::<u64>::new();
        for (index, release) in nodes.iter().enumerate() {
            if release.metadata.contains_key(metadata::ROLLOUT) {
                rollouts.insert(index as u64);
            }
            if release.metadata.contains_key(metadata::BARRIER) {
                barriers.insert(index as u64);
            }
        }

        // Add edges targeting rollouts, back till the last barrier.
        let mut edges = vec![];
        for (index, _release) in nodes.iter().enumerate().rev() {
            let age = index as u64;
            if !rollouts.contains(&age) {
                continue;
            }

            let last_barrier = barriers.iter().last().cloned().unwrap_or(0);
            for i in last_barrier..age {
                edges.push((i, age))
            }
        }

        // Add edges targeting barriers, back till the previous barrier.
        let mut start = 0;
        for target in barriers {
            for i in start..target {
                edges.push((i, target))
            }
            start = target;
        }

        Ok(edges)
    }

    fn inject_barrier_reason(updates: &metadata::UpdatesJSON, release: &mut CincinnatiPayload) {
        for entry in &updates.releases {
            if entry.version != release.version {
                continue;
            }

            if let Some(barrier) = &entry.metadata.barrier {
                let reason = if barrier.reason.is_empty() {
                    "generic"
                } else {
                    &barrier.reason
                };

                release
                    .metadata
                    .insert(metadata::BARRIER.to_string(), true.to_string());
                release
                    .metadata
                    .insert(metadata::BARRIER_REASON.to_string(), reason.to_string());
            }
        }
    }

    fn inject_deadend_reason(updates: &metadata::UpdatesJSON, release: &mut CincinnatiPayload) {
        for entry in &updates.releases {
            if entry.version != release.version {
                continue;
            }

            if let Some(deadend) = &entry.metadata.deadend {
                let reason = if deadend.reason.is_empty() {
                    "generic"
                } else {
                    &deadend.reason
                };

                release
                    .metadata
                    .insert(metadata::DEADEND.to_string(), true.to_string());
                release
                    .metadata
                    .insert(metadata::DEADEND_REASON.to_string(), reason.to_string());
            }
        }
    }

    fn inject_throttling_params(updates: &metadata::UpdatesJSON, release: &mut CincinnatiPayload) {
        for entry in &updates.releases {
            if entry.version != release.version {
                continue;
            }

            if let Some(rollout) = &entry.metadata.rollout {
                release
                    .metadata
                    .insert(metadata::ROLLOUT.to_string(), true.to_string());
                if let Some(val) = rollout.start_epoch {
                    release
                        .metadata
                        .insert(metadata::START_EPOCH.to_string(), val.to_string());
                }
                if let Some(val) = rollout.start_percentage {
                    release
                        .metadata
                        .insert(metadata::START_VALUE.to_string(), val.to_string());
                }
                if let Some(minutes) = &rollout.duration_minutes {
                    release
                        .metadata
                        .insert(metadata::DURATION.to_string(), minutes.to_string());
                }
            }
        }
    }
}
