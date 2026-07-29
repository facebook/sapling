/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Inline execution of the derivation pipeline over a set of changesets.
//!
//! The pipeline splits derivation of a single derived-data type into
//! path-scoped stages derived in dependency order (deepest subtree first),
//! optionally followed by a finalize stage. This crate turns a
//! [`DerivationPipelineConfig`] plus a topologically-ordered list of changesets
//! into batches (split at chokepoints) and drives each stage/type via
//! [`bulk_derivation::derive_stage_batch`].
//!
//! It sits above `bulk_derivation` (which it calls) and is shared by the
//! derivation-pipeline test harness and the async-requests backfill worker so
//! there is a single, tested orchestration path.

use std::collections::BTreeSet;
use std::collections::HashMap;

use anyhow::Result;
use anyhow::anyhow;
use blobstore::Loadable;
use bulk_derivation::BulkDerivation;
use context::CoreContext;
use derivation_pipeline_utils::Batch;
use derived_data_manager::DerivationError;
use derived_data_manager::DerivationStagePayload;
use derived_data_manager::DerivedDataManager;
use derived_data_manager::ManifestStagePayload;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use metaconfig_types::DerivationPipelineConfig;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use repo_blobstore::RepoBlobstore;

/// Bounded concurrency for loading bonsais during chokepoint classification.
const LOAD_BONSAIS_CONCURRENCY: usize = 100;

/// The order in which a pipeline run visits stages and types.
///
/// Stages are deepest-first: config validation guarantees each dependency is
/// exactly one path element deeper, so path-depth-descending is a valid
/// topological order over the stage DAG. Types are in dependency order so that,
/// within a stage, a type is derived only after the types it depends on.
pub struct PipelineOrder {
    pub sorted_stages: Vec<MPath>,
    pub sorted_types: Vec<DerivableType>,
}

/// Compute the deepest-first stage order and the dependency-topological type
/// order for `types`. Pure; performs no I/O.
pub fn plan_stages_and_types(
    manager: &DerivedDataManager,
    config: &DerivationPipelineConfig,
    types: &[DerivableType],
) -> PipelineOrder {
    let mut sorted_stages: Vec<MPath> = config.stages.keys().cloned().collect();
    sorted_stages.sort_by(|a, b| {
        b.num_components()
            .cmp(&a.num_components())
            .then_with(|| a.cmp(b))
    });
    PipelineOrder {
        sorted_stages,
        sorted_types: topo_sort_types(manager, types),
    }
}

/// Topologically sort `types` so every type's dependencies (restricted to the
/// given set) come before it.
fn topo_sort_types(manager: &DerivedDataManager, types: &[DerivableType]) -> Vec<DerivableType> {
    let managed: BTreeSet<DerivableType> = types.iter().copied().collect();
    let mut sorted: Vec<DerivableType> = Vec::with_capacity(types.len());
    let mut placed: BTreeSet<DerivableType> = BTreeSet::new();
    while sorted.len() < types.len() {
        for &derivable_type in types {
            if placed.contains(&derivable_type) {
                continue;
            }
            let deps_ready = manager
                .dependency_types(derivable_type)
                .into_iter()
                .filter(|dep| managed.contains(dep))
                .all(|dep| placed.contains(&dep));
            if deps_ready {
                sorted.push(derivable_type);
                placed.insert(derivable_type);
            }
        }
    }
    sorted
}

/// Split an already-forward-topological (parents-first) changeset list into
/// chokepoint-aware batches of at most `config.batch_size`.
///
/// IMPORTANT: `cs_ids` MUST be in forward topological order — every parent
/// before its children. The pipeline derives each batch's stages assuming all
/// out-of-batch parents are already derived; a reverse-ordered input would
/// derive children before their parents.
pub async fn plan_batches(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    config: &DerivationPipelineConfig,
    cs_ids: Vec<ChangesetId>,
) -> Result<Vec<Batch>> {
    let batch_size = config.batch_size.get() as usize;
    let raw_batches: Vec<Vec<ChangesetId>> = cs_ids.chunks(batch_size).map(<[_]>::to_vec).collect();
    let bonsais = stream::iter(cs_ids)
        .map(|cs_id| async move { anyhow::Ok((cs_id, cs_id.load(ctx, blobstore).await?)) })
        .buffer_unordered(LOAD_BONSAIS_CONCURRENCY)
        .try_collect::<HashMap<_, _>>()
        .await?;
    Ok(derivation_pipeline_utils::split_batches_at_chokepoints(
        raw_batches,
        &bonsais,
        config,
    ))
}

/// Bounded concurrency for deriving independent same-depth stages within one
/// phase. Stages of equal depth never depend on each other (config validation
/// guarantees a dependency is exactly one level deeper), so they can be derived
/// concurrently. This horizontal (per-subtree) parallelism is the only
/// parallelism available for serial (non-predecessor-derivable) types, whose
/// slices cannot be fanned out vertically.
const STAGE_PHASE_CONCURRENCY: usize = 16;

/// Build the manifest-stage payload for `stage_path` from its configured
/// dependencies (each dependency contributes its last path element).
fn manifest_payload(
    config: &DerivationPipelineConfig,
    stage_path: &MPath,
) -> Result<DerivationStagePayload> {
    let stage_config = config
        .stages
        .get(stage_path)
        .ok_or_else(|| anyhow!("stage {stage_path:?} missing from pipeline config"))?;
    let deps: Vec<MPathElement> = stage_config
        .dependencies
        .iter()
        .map(|dep_path| {
            dep_path
                .iter()
                .last()
                .cloned()
                .ok_or_else(|| anyhow!("dependency path {dep_path:?} is empty"))
        })
        .collect::<Result<_>>()?;
    Ok(DerivationStagePayload::Manifest(ManifestStagePayload {
        path: stage_path.clone(),
        deps,
    }))
}

/// Derive a single stage for every type. Types are derived in dependency order
/// so that a type depending on another at the same stage (e.g. blame on unodes)
/// sees its dependency's stage output already stored. Takes owned inputs so the
/// returned future is `Send + 'static` and can run concurrently within a phase
/// (a borrowed `&CoreContext` is not `Send`-general-enough for the worker's
/// `tokio::spawn`).
async fn derive_stage(
    manager: DerivedDataManager,
    ctx: CoreContext,
    payload: DerivationStagePayload,
    sorted_types: Vec<DerivableType>,
    commits: Vec<ChangesetId>,
) -> Result<(), DerivationError> {
    for derivable_type in sorted_types {
        let variant = derivable_type.into_pipeline_derivable_variant()?;
        bulk_derivation::derive_stage_batch(&manager, &ctx, commits.clone(), &payload, variant)
            .await?;
    }
    Ok(())
}

/// Execute a planned pipeline.
///
/// Batches run sequentially — that is the cross-batch (vertical) dependency
/// edge, so a batch's stage outputs are stored before the next batch begins.
/// Within a batch, stages are derived phase by phase in deepest-first order (a
/// stage's child stages must be stored before it), and the mutually-independent
/// same-depth stages of each phase are derived **concurrently** — the pipeline's
/// horizontal parallelism. Finally the finalize stage runs for the types that
/// have one.
///
/// `batches` must be forward-topological (see [`plan_batches`]) and
/// `sorted_stages` deepest-first (see [`plan_stages_and_types`]).
pub async fn run_pipeline_batches(
    manager: &DerivedDataManager,
    ctx: &CoreContext,
    config: &DerivationPipelineConfig,
    sorted_stages: &[MPath],
    sorted_types: &[DerivableType],
    batches: &[Batch],
) -> Result<(), DerivationError> {
    for batch in batches {
        // `sorted_stages` is deepest-first, so equal-depth stages are contiguous;
        // each chunk is one phase of mutually-independent stages. Build each
        // stage's payload synchronously (borrowing `config`), then hand
        // fully-owned inputs to the concurrent futures so the buffered stream is
        // `Send + 'static`.
        for phase in sorted_stages.chunk_by(|a, b| a.num_components() == b.num_components()) {
            let stage_futures = phase
                .iter()
                .map(|stage_path| {
                    let payload = manifest_payload(config, stage_path)?;
                    Ok::<_, DerivationError>(derive_stage(
                        manager.clone(),
                        ctx.clone(),
                        payload,
                        sorted_types.to_vec(),
                        batch.commits.clone(),
                    ))
                })
                .collect::<Result<Vec<_>, DerivationError>>()?;
            stream::iter(stage_futures)
                .buffer_unordered(STAGE_PHASE_CONCURRENCY)
                .try_collect::<Vec<_>>()
                .await?;
        }

        // Finalize stage (a step distinct from the terminal manifest stage) for
        // the types that have one, after every manifest stage of the batch is
        // stored.
        for &derivable_type in sorted_types {
            let variant = derivable_type.into_pipeline_derivable_variant()?;
            if !bulk_derivation::pipeline_has_finalize(variant) {
                continue;
            }
            bulk_derivation::derive_stage_batch(
                manager,
                ctx,
                batch.commits.clone(),
                &DerivationStagePayload::Finalize,
                variant,
            )
            .await?;
        }
    }
    Ok(())
}
