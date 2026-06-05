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

/// The four pipeline-derivable types verified by the harness.
const PIPELINE_TYPES: [DerivableType; 4] = [
    DerivableType::Fsnodes,
    DerivableType::Unodes,
    DerivableType::SkeletonManifests,
    DerivableType::SkeletonManifestsV2,
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
    let ctx = &CoreContext::test_mock(fb);
    let (repo, commits, _dag) = F::get_repo_and_dag::<TestRepo>(fb).await;
    let manager = repo.repo_derived_data().manager();

    let head = *commits
        .get("master")
        .or_else(|| commits.get("J"))
        .ok_or_else(|| anyhow!("fixture {} has no master bookmark head", F::REPO_NAME))?;

    let config = pipeline_config_from_stages(F::pipeline_stages())?;

    // All commits as ancestors of head, oldest first.
    let mut all_commits = repo
        .commit_graph()
        .ancestors_difference(ctx, vec![head], vec![])
        .await?;
    all_commits.reverse();

    // Subtree-change derivation reads the knobs that gate manifest-altering
    // subtree changes; enable them for both canonical derivation and the
    // pipeline run. Harmless for fixtures without subtree changes.
    let run = run_derivation_and_verification::<F>(ctx, &repo, manager, &config, &all_commits);
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

async fn run_derivation_and_verification<F: PipelineTestFixture + Send>(
    ctx: &CoreContext,
    repo: &TestRepo,
    manager: &DerivedDataManager,
    config: &DerivationPipelineConfig,
    all_commits: &[ChangesetId],
) -> Result<()> {
    let head = *all_commits
        .last()
        .ok_or_else(|| anyhow!("fixture {} has no commits", F::REPO_NAME))?;

    // Derive canonically for every type and commit.
    manager
        .derive_bulk_locally(ctx, all_commits, None, &PIPELINE_TYPES, None)
        .await
        .map_err(anyhow::Error::from)?;

    // Plan batches: topological slices split at chokepoints.
    let batches: Vec<Vec<_>> = repo
        .commit_graph()
        .ancestors_difference_segment_slices(ctx, vec![head], vec![], PIPELINE_BATCH_SIZE)
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

    // Execute the pipeline synchronously in dependency-and-ancestor order so
    // every required input is already stored before it is read.
    for batch in &batches {
        for stage_path in &sorted_stages {
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
            for derivable_type in PIPELINE_TYPES {
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

    // Assert pipeline output matches canonical for every commit/type/stage.
    for &cs_id in all_commits {
        for stage_path in &sorted_stages {
            for derivable_type in PIPELINE_TYPES {
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

#[cfg(test)]
mod tests {
    use fixtures::NestedAncestorSubtreeCopy;
    use fixtures::NestedDirectories;
    use fixtures::NestedSubtreeCopy;
    use mononoke_macros::mononoke;

    use super::*;

    impl PipelineTestFixture for NestedDirectories {
        fn pipeline_stages() -> Vec<(&'static str, Vec<&'static str>)> {
            vec![
                ("", vec!["top1", "top2"]),
                ("top1", vec![]),
                ("top2", vec!["nested1", "nested2"]),
                ("top2/nested1", vec![]),
                ("top2/nested2", vec![]),
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

    // Disjoint subtree-copy divergence: for a subtree copy whose dest is in a
    // different stage than its source, `derive_manifest_inner` grafts the
    // out-of-prefix subtree parent_replacement onto the source stage's root,
    // because `MPath::remove_prefix_component` collapses a non-matching path to
    // `MPath::ROOT` instead of dropping it. The tip commit also modifies a file
    // under the source stage, so the terminal "" merge consumes the corrupted
    // source-stage intermediate and the divergence reaches the terminal "" stage
    // (reader-visible at the canonical mapping), not just the source stage.
    // `#[ignore]`d because pipeline output diverges from canonical; the assertion
    // is deliberately left strict.
    #[ignore]
    #[mononoke::fbinit_test]
    async fn test_pipeline_matches_canonical_subtree_copy(fb: FacebookInit) -> Result<()> {
        verify_pipeline_matches_canonical::<NestedSubtreeCopy>(fb).await
    }

    // Strict-ancestor subtree-copy divergence: the tip commit copies the whole
    // `top2` subtree onto `top1`, whose dest path is a strict ancestor of the
    // deeper `top1/sub` stage, so that stage's content must become the `sub`
    // sub-slice of the replacement (`top2/sub`). `derive_manifest_inner` drops any
    // replacement whose path is not under the stage prefix, so the `top1/sub`
    // stage ignores the copy and derives from its stale parent, diverging from
    // canonical for all four manifest types. `#[ignore]`d because pipeline output
    // diverges from canonical; the assertion is deliberately left strict.
    #[ignore]
    #[mononoke::fbinit_test]
    async fn test_pipeline_matches_canonical_ancestor_subtree_copy(fb: FacebookInit) -> Result<()> {
        verify_pipeline_matches_canonical::<NestedAncestorSubtreeCopy>(fb).await
    }
}
