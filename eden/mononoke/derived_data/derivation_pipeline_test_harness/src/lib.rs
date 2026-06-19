/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Shared utilities for derived-data tests: a reusable `TestRepo` facet
//! container and a harness that checks derivation-pipeline output matches
//! canonical derivation.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::num::NonZeroU64;

use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkKey;
use bookmarks::Bookmarks;
use bookmarks::BookmarksRef;
use bulk_derivation::BulkDerivation;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use derived_data_manager::DerivationStagePayload;
use derived_data_manager::DerivedDataManager;
use derived_data_manager::ManifestStagePayload;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use fixtures::TestRepoFixture;
use futures::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::with_just_knobs_async;
use metaconfig_types::DerivationPipelineConfig;
use metaconfig_types::DerivationPipelineStageConfig;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;

/// Facet container with the attributes derivation tests need. This is the
/// shared replacement for per-crate `TestRepo` declarations.
#[facet::container]
pub struct TestRepo(
    RepoIdentity,
    dyn BonsaiHgMapping,
    dyn Bookmarks,
    CommitGraph,
    dyn CommitGraphWriter,
    RepoDerivedData,
    RepoBlobstore,
    FilestoreConfig,
);

/// The pipeline-derivable types verified by the harness.
const PIPELINE_TYPES: [DerivableType; 9] = [
    DerivableType::Fsnodes,
    DerivableType::Unodes,
    DerivableType::Fastlog,
    DerivableType::BlameV2,
    DerivableType::SkeletonManifests,
    DerivableType::SkeletonManifestsV2,
    DerivableType::AclManifests,
    DerivableType::HgChangesets,
    DerivableType::HgAugmentedManifests,
];

const PIPELINE_BATCH_SIZE: u64 = 3;

/// A fixture that additionally describes a pipeline-stage layout.
pub trait PipelineTestFixture: TestRepoFixture {
    /// Stages as `(stage_absolute_path, [dependency_child_component, ...])`.
    /// ROOT is the empty string. Each dependency component names a child stage
    /// exactly one path element deeper than this stage.
    fn pipeline_stages() -> Vec<(&'static str, Vec<&'static str>)>;
}

/// Build and validate a `DerivationPipelineConfig` from the fixture's stages.
pub fn pipeline_config_from_stages(
    stages: Vec<(&'static str, Vec<&'static str>)>,
) -> Result<DerivationPipelineConfig> {
    let stage_paths: Vec<MPath> = stages
        .iter()
        .map(|(path, _)| parse_stage_path(path))
        .collect::<Result<_>>()?;

    let stages_map = stages
        .iter()
        .zip(stage_paths.iter())
        .map(|((_, deps), stage_path)| {
            let dep_paths = deps
                .iter()
                .map(|dep| {
                    let element = MPathElement::new(dep.as_bytes().to_vec())?;
                    Ok(stage_path.join(std::iter::once(&element)))
                })
                .collect::<Result<Vec<MPath>>>()?;
            Ok((
                stage_path.clone(),
                DerivationPipelineStageConfig {
                    dependencies: dep_paths,
                },
            ))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    let config = DerivationPipelineConfig {
        types: BTreeSet::from(PIPELINE_TYPES),
        bookmarks: vec![BookmarkKey::new("master")?],
        stages: stages_map,
        batch_size: NonZeroU64::new(PIPELINE_BATCH_SIZE)
            .ok_or_else(|| anyhow!("batch size must be non-zero"))?,
    };
    config.validate()?;
    Ok(config)
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

fn parse_stage_path(path: &str) -> Result<MPath> {
    if path.is_empty() {
        Ok(MPath::ROOT)
    } else {
        MPath::new(path)
    }
}

/// Verify that derivation-pipeline output matches canonical derivation for
/// every commit, type, and stage of the fixture.
pub async fn verify_pipeline_matches_canonical<F: PipelineTestFixture + Send>(
    fb: FacebookInit,
) -> Result<()> {
    // No pipeline boundary: the pipeline derives (and we verify) every commit.
    verify_pipeline_matches_canonical_impl::<F>(fb, vec![]).await
}

/// Like `verify_pipeline_matches_canonical`, but a prefix of the graph (the
/// ancestors of `boundary_label`, inclusive) is derived ONLY canonically and is
/// excluded from the pipeline run. The pipeline derives just the descendants of
/// the boundary, so the first pipeline batch's parents are canonical-only and
/// the manager must bridge them via `extract_stage_output_from_derived`. This
/// exercises the transitionary path that `verify_pipeline_matches_canonical`
/// never hits (there every ancestor has a stored pipeline stage output).
pub async fn verify_pipeline_matches_canonical_with_canonical_ancestors<
    F: PipelineTestFixture + Send,
>(
    fb: FacebookInit,
    boundary_label: &str,
) -> Result<()> {
    let (_repo, commits, _dag) = F::get_repo_and_dag::<TestRepo>(fb).await;
    let boundary = *commits.get(boundary_label).ok_or_else(|| {
        anyhow!(
            "fixture {} has no commit labelled {boundary_label}",
            F::REPO_NAME,
        )
    })?;
    verify_pipeline_matches_canonical_impl::<F>(fb, vec![boundary]).await
}

async fn verify_pipeline_matches_canonical_impl<F: PipelineTestFixture + Send>(
    fb: FacebookInit,
    pipeline_boundary: Vec<ChangesetId>,
) -> Result<()> {
    let ctx = &CoreContext::test_mock(fb);
    let (repo, _commits, _dag) = F::get_repo_and_dag::<TestRepo>(fb).await;
    let manager = repo.repo_derived_data().manager();

    let head = repo
        .bookmarks()
        .get(
            ctx.clone(),
            &BookmarkKey::new("master")?,
            bookmarks::Freshness::MostRecent,
        )
        .await?
        .ok_or_else(|| anyhow!("fixture {} has no master bookmark", F::REPO_NAME))?;

    let config = pipeline_config_from_stages(F::pipeline_stages())?;

    // All commits as ancestors of head, oldest first. Every commit is derived
    // canonically (so boundary commits have a canonical value for the manager
    // to extract a stage output from); only the pipeline run and verification
    // are scoped to the descendants of `pipeline_boundary`.
    let mut all_commits = repo
        .commit_graph()
        .ancestors_difference(ctx, vec![head], vec![])
        .await?;
    all_commits.reverse();

    // Subtree-change derivation reads the knobs that gate manifest-altering
    // subtree changes; enable them for both canonical derivation and the
    // pipeline run. Harmless for fixtures without subtree changes.
    let run = run_derivation_and_verification::<F>(
        ctx,
        &repo,
        manager,
        &config,
        &all_commits,
        head,
        pipeline_boundary,
    );
    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([
            (
                "scm/mononoke:enable_subtree_changes".to_string(),
                KnobVal::Bool(true),
            ),
            (
                "scm/mononoke:enable_manifest_altering_subtree_changes".to_string(),
                KnobVal::Bool(true),
            ),
        ])),
        run.boxed(),
    )
    .await
}

/// Like `verify_pipeline_matches_canonical`, but runs the full pipeline
/// derivation BEFORE any canonical derivation, then derives canonical and
/// verifies byte-equality. The canonical-first entry points populate every
/// canonical artifact (e.g. `bonsai_hg_mapping`) before the pipeline runs, so a
/// pipeline code path that reads a canonical artifact for an ancestor silently
/// succeeds. Running pipeline-first reproduces the production "pipeline ahead of
/// canonical" state where such a read would instead fail.
///
/// Scoped to the full graph (no canonical-only boundary): every parent is
/// pipeline-derived in an earlier batch, so the pipeline is self-sufficient.
///
/// `types` selects which types to derive and verify pipeline-first. Restrict it
/// to types whose pipeline derivation is self-sufficient (e.g. `HgChangesets`);
/// types with cross-type dependencies that read canonical data (e.g. `BlameV2`
/// reading `Unodes`) are not yet safe to run pipeline-first.
pub async fn verify_pipeline_first_then_canonical<F: PipelineTestFixture + Send>(
    fb: FacebookInit,
    types: &[DerivableType],
) -> Result<()> {
    let ctx = &CoreContext::test_mock(fb);
    let (repo, commits, _dag) = F::get_repo_and_dag::<TestRepo>(fb).await;
    let manager = repo.repo_derived_data().manager();

    let head = *commits
        .get("master")
        .or_else(|| commits.get("Q"))
        .ok_or_else(|| anyhow!("fixture {} has no master bookmark head", F::REPO_NAME))?;

    let config = pipeline_config_from_stages(F::pipeline_stages())?;

    let mut all_commits = repo
        .commit_graph()
        .ancestors_difference(ctx, vec![head], vec![])
        .await?;
    all_commits.reverse();

    let run = run_pipeline_first_then_canonical::<F>(
        ctx,
        &repo,
        manager,
        &config,
        types,
        &all_commits,
        head,
    );
    with_just_knobs_async(
        JustKnobsInMemory::new(HashMap::from([
            (
                "scm/mononoke:enable_subtree_changes".to_string(),
                KnobVal::Bool(true),
            ),
            (
                "scm/mononoke:enable_manifest_altering_subtree_changes".to_string(),
                KnobVal::Bool(true),
            ),
        ])),
        run.boxed(),
    )
    .await
}

/// The planned, payload-agnostic pipeline work: batches (split at chokepoints),
/// the stages in deepest-first (dependency) order, the types in dependency
/// order, and the set of commits the pipeline derives (and we verify).
struct PipelinePlan {
    batches: Vec<derivation_pipeline_utils::Batch>,
    sorted_stages: Vec<MPath>,
    sorted_types: Vec<DerivableType>,
    pipeline_commits: BTreeSet<ChangesetId>,
}

/// Plan batches: topological slices split at chokepoints. With a non-empty
/// `pipeline_boundary` (passed as `common`), only descendants of the boundary
/// are planned, so the boundary commits remain canonical-only parents.
async fn plan_pipeline(
    ctx: &CoreContext,
    repo: &TestRepo,
    manager: &DerivedDataManager,
    config: &DerivationPipelineConfig,
    types: &[DerivableType],
    head: ChangesetId,
    pipeline_boundary: Vec<ChangesetId>,
) -> Result<PipelinePlan> {
    let pipeline_commits: BTreeSet<ChangesetId> = repo
        .commit_graph()
        .ancestors_difference(ctx, vec![head], pipeline_boundary.clone())
        .await?
        .into_iter()
        .collect();
    let batches: Vec<Vec<_>> = repo
        .commit_graph()
        .ancestors_difference_segment_slices(
            ctx,
            vec![head],
            pipeline_boundary,
            PIPELINE_BATCH_SIZE,
        )
        .await?
        .try_collect()
        .await?;
    let blobstore = repo.repo_blobstore();
    let bonsais = stream::iter(batches.iter().flatten().copied())
        .map(|cs_id| async move { anyhow::Ok((cs_id, cs_id.load(ctx, blobstore).await?)) })
        .buffer_unordered(100)
        .try_collect::<HashMap<_, _>>()
        .await?;
    let batches =
        derivation_pipeline_utils::split_batches_at_chokepoints(batches, &bonsais, config);
    // Deepest stages first; config validation guarantees deps are exactly one
    // level deeper, so depth order is a valid topological order.
    let mut sorted_stages: Vec<MPath> = config.stages.keys().cloned().collect();
    sorted_stages.sort_by(|a, b| {
        b.num_components()
            .cmp(&a.num_components())
            .then_with(|| a.cmp(b))
    });
    // Each type's dependencies (within the managed set) must be derived before
    // it, mirroring the deepest-stage-first ordering above.
    let sorted_types = topo_sort_types(manager, types);
    Ok(PipelinePlan {
        batches,
        sorted_stages,
        sorted_types,
        pipeline_commits,
    })
}

/// Execute the pipeline synchronously in dependency-and-ancestor order so every
/// required input is already stored before it is read.
async fn run_pipeline(
    manager: &DerivedDataManager,
    ctx: &CoreContext,
    config: &DerivationPipelineConfig,
    plan: &PipelinePlan,
) -> Result<()> {
    for batch in &plan.batches {
        for stage_path in &plan.sorted_stages {
            let stage_config = &config.stages[stage_path];
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
            let payload = DerivationStagePayload::Manifest(ManifestStagePayload {
                path: stage_path.clone(),
                deps,
            });
            for &derivable_type in &plan.sorted_types {
                let variant = derivable_type.into_pipeline_derivable_variant()?;
                bulk_derivation::derive_stage_batch(
                    manager,
                    ctx,
                    batch.commits.clone(),
                    &payload,
                    variant,
                )
                .await?;
            }
        }
    }
    Ok(())
}

/// Assert pipeline output matches canonical for every pipeline-derived
/// commit/type/stage. Commits outside `plan.pipeline_commits` (a canonical-only
/// boundary and its ancestors) have no pipeline stage outputs and are skipped.
async fn verify_pipeline_output<F: PipelineTestFixture>(
    manager: &DerivedDataManager,
    ctx: &CoreContext,
    all_commits: &[ChangesetId],
    plan: &PipelinePlan,
) -> Result<()> {
    for &cs_id in all_commits {
        if !plan.pipeline_commits.contains(&cs_id) {
            continue;
        }
        for stage_path in &plan.sorted_stages {
            for &derivable_type in &plan.sorted_types {
                let matches = BulkDerivation::verify_stage_output(
                    manager,
                    ctx,
                    cs_id,
                    derivable_type,
                    stage_path,
                )
                .await?;
                if !matches {
                    bail!(
                        "pipeline output diverged from canonical: fixture={} cs_id={cs_id} type={derivable_type:?} stage={stage_path:?}",
                        F::REPO_NAME,
                    );
                }
            }
        }
    }
    Ok(())
}

async fn run_derivation_and_verification<F: PipelineTestFixture + Send>(
    ctx: &CoreContext,
    repo: &TestRepo,
    manager: &DerivedDataManager,
    config: &DerivationPipelineConfig,
    all_commits: &[ChangesetId],
    head: ChangesetId,
    pipeline_boundary: Vec<ChangesetId>,
) -> Result<()> {
    // Derive canonically for every type and commit.
    manager
        .derive_bulk_locally(ctx, all_commits, None, &PIPELINE_TYPES, None, None)
        .await
        .map_err(anyhow::Error::from)?;

    let plan = plan_pipeline(
        ctx,
        repo,
        manager,
        config,
        &PIPELINE_TYPES,
        head,
        pipeline_boundary,
    )
    .await?;
    run_pipeline(manager, ctx, config, &plan).await?;
    verify_pipeline_output::<F>(manager, ctx, all_commits, &plan).await
}

/// Run the full pipeline before any canonical derivation, then derive canonical
/// and verify. See `verify_pipeline_first_then_canonical`.
async fn run_pipeline_first_then_canonical<F: PipelineTestFixture + Send>(
    ctx: &CoreContext,
    repo: &TestRepo,
    manager: &DerivedDataManager,
    config: &DerivationPipelineConfig,
    types: &[DerivableType],
    all_commits: &[ChangesetId],
    head: ChangesetId,
) -> Result<()> {
    // Plan and run the pipeline BEFORE deriving canonical. Over the full graph
    // every parent is pipeline-derived in an earlier batch, so no canonical data
    // is required for the pipeline to complete.
    let plan = plan_pipeline(ctx, repo, manager, config, types, head, vec![]).await?;
    run_pipeline(manager, ctx, config, &plan).await?;

    // Now derive canonical for the selected types, then verify byte-equality.
    manager
        .derive_bulk_locally(ctx, all_commits, None, types, None, None)
        .await
        .map_err(anyhow::Error::from)?;
    verify_pipeline_output::<F>(manager, ctx, all_commits, &plan).await
}

#[cfg(test)]
mod tests {
    use fixtures::AclNestedDirectories;
    use fixtures::CrossStageDirectoryCopy;
    use fixtures::CrossStageFileCopy;
    use fixtures::NestedAncestorSubtreeCopy;
    use fixtures::NestedDirectories;
    use fixtures::NestedSubtreeCopy;
    use mononoke_macros::mononoke;

    use super::*;

    /// Types verified pipeline-first (pipeline derived before any canonical
    /// derivation; see `verify_pipeline_first_then_canonical`). A type belongs
    /// here only if its pipeline derivation never reads canonical data that the
    /// pipeline-ahead-of-canonical state would leave absent.
    ///
    /// `HgChangesets` qualifies: its only canonical read is the cross-stage
    /// copy-source parent's hg, now resolved from the pipeline terminal stage
    /// output (canonical fallback only for genuinely pre-pipeline parents).
    ///
    /// Excluded for now: `BlameV2` and `Fastlog` resolve their `Unodes`
    /// dependency from canonical derived data. That is safe in production because
    /// `Unodes` has been fully canonically derived (prod mapping) for a long
    /// time, so it is never absent there — but it makes them fail the artificial
    /// pipeline-first harness that withholds canonical. Add a type here once its
    /// pipeline derivation is self-sufficient for every dependency still in the
    /// transitionary state.
    const PIPELINE_FIRST_TYPES: [DerivableType; 1] = [DerivableType::HgChangesets];

    impl PipelineTestFixture for NestedDirectories {
        fn pipeline_stages() -> Vec<(&'static str, Vec<&'static str>)> {
            vec![
                ("", vec!["top1", "top2", "top3"]),
                ("top1", vec![]),
                ("top2", vec!["nested1", "nested2"]),
                ("top2/nested1", vec![]),
                ("top2/nested2", vec![]),
                ("top3", vec![]),
            ]
        }
    }

    impl PipelineTestFixture for AclNestedDirectories {
        fn pipeline_stages() -> Vec<(&'static str, Vec<&'static str>)> {
            vec![
                ("", vec!["top1", "top2"]),
                ("top1", vec!["sub"]),
                ("top1/sub", vec![]),
                ("top2", vec![]),
            ]
        }
    }

    impl PipelineTestFixture for NestedSubtreeCopy {
        fn pipeline_stages() -> Vec<(&'static str, Vec<&'static str>)> {
            vec![
                ("", vec!["top1", "top2"]),
                ("top1", vec![]),
                ("top2", vec![]),
            ]
        }
    }

    impl PipelineTestFixture for CrossStageDirectoryCopy {
        fn pipeline_stages() -> Vec<(&'static str, Vec<&'static str>)> {
            vec![
                ("", vec!["top1", "top2"]),
                ("top1", vec![]),
                ("top2", vec![]),
            ]
        }
    }

    impl PipelineTestFixture for CrossStageFileCopy {
        fn pipeline_stages() -> Vec<(&'static str, Vec<&'static str>)> {
            vec![
                ("", vec!["top1", "top2"]),
                ("top1", vec![]),
                ("top2", vec![]),
            ]
        }
    }

    impl PipelineTestFixture for NestedAncestorSubtreeCopy {
        fn pipeline_stages() -> Vec<(&'static str, Vec<&'static str>)> {
            vec![
                ("", vec!["top1", "top2"]),
                ("top1", vec!["sub"]),
                ("top1/sub", vec![]),
                ("top2", vec![]),
            ]
        }
    }

    #[mononoke::fbinit_test]
    async fn test_pipeline_matches_canonical(fb: FacebookInit) -> Result<()> {
        verify_pipeline_matches_canonical::<NestedDirectories>(fb).await
    }

    #[mononoke::fbinit_test]
    async fn test_pipeline_matches_canonical_acl(fb: FacebookInit) -> Result<()> {
        verify_pipeline_matches_canonical::<AclNestedDirectories>(fb).await
    }

    // Boundary commit `E` is derived canonically only; the pipeline derives just
    // its descendants. The first pipeline batch's parents include `E`, which has
    // no pipeline stage output, so the manager bridges it via
    // `extract_stage_output_from_derived`.
    #[mononoke::fbinit_test]
    async fn test_pipeline_matches_canonical_acl_with_canonical_ancestors(
        fb: FacebookInit,
    ) -> Result<()> {
        verify_pipeline_matches_canonical_with_canonical_ancestors::<AclNestedDirectories>(fb, "E")
            .await
    }

    #[mononoke::fbinit_test]
    async fn test_pipeline_matches_canonical_subtree_copy(fb: FacebookInit) -> Result<()> {
        verify_pipeline_matches_canonical::<NestedSubtreeCopy>(fb).await
    }

    #[mononoke::fbinit_test]
    async fn test_pipeline_matches_canonical_ancestor_subtree_copy(fb: FacebookInit) -> Result<()> {
        verify_pipeline_matches_canonical::<NestedAncestorSubtreeCopy>(fb).await
    }

    // Regression coverage for cross-stage copy-source resolution: the tip commit
    // carries copies whose `copy_from` sources are a directory and a missing path
    // outside the destination's stage. Both canonical and pipeline drop the bogus
    // copies in `resolve_cross_stage_copy_sources`, staying byte-identical.
    #[mononoke::fbinit_test]
    async fn test_pipeline_cross_stage_directory_copy(fb: FacebookInit) -> Result<()> {
        verify_pipeline_matches_canonical::<CrossStageDirectoryCopy>(fb).await
    }

    // Success path of cross-stage copy resolution: a real file copied across a
    // stage boundary, canonical-first.
    #[mononoke::fbinit_test]
    async fn test_pipeline_cross_stage_file_copy(fb: FacebookInit) -> Result<()> {
        verify_pipeline_matches_canonical::<CrossStageFileCopy>(fb).await
    }

    // Pipeline-first: derives the pipeline before any canonical derivation, so a
    // cross-stage copy whose source parent is not yet canonically derived must
    // still resolve. Regression guard for the `bonsai_hg_mapping` dependency in
    // `resolve_cross_stage_copy_sources`. Scoped to `PIPELINE_FIRST_TYPES`.
    #[mononoke::fbinit_test]
    async fn test_pipeline_first_cross_stage_file_copy(fb: FacebookInit) -> Result<()> {
        verify_pipeline_first_then_canonical::<CrossStageFileCopy>(fb, &PIPELINE_FIRST_TYPES).await
    }

    // Boundary commit `D` is derived canonically only; the pipeline derives just
    // its descendants (E, F, G, H, I, J, K, L, M, N, R, O, P, Q). The first pipeline batch's
    // parents include `D`, which has no pipeline stage output, so the manager must
    // bridge it via `extract_stage_output_from_derived`. The bridge is genuinely
    // used: `D` introduced `top2/nested1`, and the descendants leave that subtree
    // unchanged (they work in `top2/nested2`, `top1`, `top3`, and the root), so
    // the `top2`/`top2/nested1` stage outputs of the children are inherited from
    // `D`'s canonical value rather than recomputed from a stored pipeline stage.
    #[mononoke::fbinit_test]
    async fn test_pipeline_matches_canonical_with_canonical_ancestors(
        fb: FacebookInit,
    ) -> Result<()> {
        verify_pipeline_matches_canonical_with_canonical_ancestors::<NestedDirectories>(fb, "D")
            .await
    }
}
