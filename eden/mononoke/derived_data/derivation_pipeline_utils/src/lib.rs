/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Payload-agnostic planning logic for the derivation pipeline: batch
//! splitting at chokepoints.

use std::collections::HashMap;
use std::collections::HashSet;

use metaconfig_types::DerivationPipelineConfig;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::MPathHash;
use mononoke_types::NonRootMPath;

/// Why a single-changeset batch is a chokepoint, and how far its barrier reaches.
pub enum ChokepointKind {
    /// Subtree change: the source can span many stages, so every stage depends
    /// on the previous batch's terminal stage. Rare, so not scoped.
    Global,
    /// Cross-stage `copy_from`: only these dest stages depend on the previous
    /// batch's terminal stage; every other stage keeps its normal per-stage dep.
    Stages(HashSet<MPathHash>),
}

/// A batch of changesets, optionally a single-changeset chokepoint.
pub struct Batch {
    pub commits: Vec<ChangesetId>,
    pub chokepoint: Option<ChokepointKind>,
}

/// The stage that owns `path`: the longest stage path that is a prefix of it.
/// Stages partition the path space (`MPath::ROOT` is the catch-all), so this is
/// always `Some`.
pub fn owning_stage<'a>(
    pipeline_config: &'a DerivationPipelineConfig,
    path: &NonRootMPath,
) -> Option<&'a MPath> {
    pipeline_config
        .stages
        .keys()
        .filter(|stage| stage.is_prefix_of(path))
        .max_by_key(|stage| stage.num_components())
}

/// Classify a changeset as a chokepoint and how far its barrier reaches.
pub fn chokepoint_kind(
    bcs: &BonsaiChangeset,
    pipeline_config: &DerivationPipelineConfig,
) -> Option<ChokepointKind> {
    // Subtree changes graft from a source that can cross many stage
    // boundaries; keep the global barrier (rare, not worth scoping).
    if bcs.has_subtree_changes() {
        return Some(ChokepointKind::Global);
    }

    // Cross-stage `copy_from`: scope the barrier to each copy's dest stage. A
    // copy is cross-stage iff the dest's owning stage does not also cover src.
    let dest_stages: HashSet<MPathHash> = bcs
        .file_changes()
        .filter_map(|(dest, fc)| {
            let (src, _) = fc.copy_from()?;
            let dest_stage = owning_stage(pipeline_config, dest)?;
            (!dest_stage.is_prefix_of(src)).then(|| dest_stage.get_path_hash())
        })
        .collect();

    (!dest_stages.is_empty()).then_some(ChokepointKind::Stages(dest_stages))
}

/// Split topologically-ordered batches at chokepoint changesets. A chokepoint
/// changeset becomes its own single-changeset batch so its vertical dep can
/// target the previous batch's terminal stage.
pub fn split_batches_at_chokepoints(
    batches: Vec<Vec<ChangesetId>>,
    bonsais: &HashMap<ChangesetId, BonsaiChangeset>,
    config: &DerivationPipelineConfig,
) -> Vec<Batch> {
    let mut split_batches: Vec<Batch> = Vec::new();
    for batch in batches {
        let mut current_batch: Vec<ChangesetId> = Vec::new();
        for cs_id in batch {
            let chokepoint = bonsais
                .get(&cs_id)
                .and_then(|bcs| chokepoint_kind(bcs, config));
            if chokepoint.is_some() {
                if !current_batch.is_empty() {
                    split_batches.push(Batch {
                        commits: std::mem::take(&mut current_batch),
                        chokepoint: None,
                    });
                }
                split_batches.push(Batch {
                    commits: vec![cs_id],
                    chokepoint,
                });
            } else {
                current_batch.push(cs_id);
            }
        }
        if !current_batch.is_empty() {
            split_batches.push(Batch {
                commits: current_batch,
                chokepoint: None,
            });
        }
    }
    split_batches
}
