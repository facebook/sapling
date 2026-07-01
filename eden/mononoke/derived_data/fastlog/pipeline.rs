/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::Loadable;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use derived_data_manager::DerivationStagePayload;
use derived_data_manager::PipelineDerivable;
use derived_data_manager::StageId;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use manifest::Diff;
use manifest::Entry;
use manifest::find_intersection_of_diffs;
use manifest::find_intersection_of_diffs_pruned;
use mononoke_macros::mononoke;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::ManifestUnodeId;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::path::MPath;
use unodes::RootUnodeManifestId;
use unodes::resolve_parent_stage_outputs;

use crate::RootFastlog;
use crate::fastlog_impl::create_new_batch_with_prefix;
use crate::fastlog_impl::fetch_fastlog_batch_by_unode_id_with_prefix;
use crate::fastlog_impl::fetch_unode_parents;
use crate::fastlog_impl::save_fastlog_batch_by_unode_id_with_prefix;
use crate::mapping::format_key;

/// Concurrency for per-unode fastlog batch writes within a single commit,
/// matching the canonical `derive_single` buffering.
const FASTLOG_BUFFER_SIZE: usize = 100;

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "derived_root_fastlog_stage.{}.{}{}",
        stage_path.get_path_hash().to_hex(),
        key_prefix,
        cs_id,
    )
}

fn use_normal_mapping(stage_path: &MPath) -> bool {
    stage_path.is_root()
        && justknobs::eval(
            "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
            None,
            Some("fastlog"),
        )
}

/// Prefix for pipeline fastlog batch blob writes/reads, gated on the same
/// prod-mapping JustKnob as the terminal mapping. After the flip the pipeline
/// writes the canonical content-addressed key (empty prefix, becoming source of
/// truth); before it, batches are namespaced. Applies at every stage (fastlog
/// batches are keyed by unode hash, so there is no cross-stage key collision).
fn pipeline_fastlog_key_prefix() -> &'static str {
    if justknobs::eval(
        "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
        None,
        Some("fastlog"),
    ) {
        ""
    } else {
        "pipeline."
    }
}

#[async_trait]
impl PipelineDerivable for RootFastlog {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant = PipelineDerivableVariant::Fastlog;

    type StageOutput = ();

    async fn derive_stage_batch(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        payload: &DerivationStagePayload,
        _parents: HashMap<ChangesetId, Self::StageOutput>,
        _dependency_outputs: HashMap<ChangesetId, HashMap<MPath, Self::StageOutput>>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let DerivationStagePayload::Manifest(payload) = payload else {
            anyhow::bail!("{} has no finalize derive", Self::NAME);
        };
        let stage_path = &payload.path;

        // Dep paths are immediate children of S (the payload stores only the
        // last element), so each relative path is a single component; the diff
        // over the S subtree produces paths relative to S, so we prune those.
        // Child stages own the unodes under their subtrees.
        let dep_paths_relative: Vec<MPath> = payload
            .deps
            .iter()
            .map(|element| MPath::ROOT.join(std::iter::once(element)))
            .collect();

        // Namespace pipeline fastlog batches until the prod-mapping flip.
        let fastlog_key_prefix = pipeline_fastlog_key_prefix();

        // Fetch unode stage-S subtrees for the batch and all their parents in a
        // single call.
        let mut all_csids: Vec<ChangesetId> =
            bonsais.iter().map(|b| b.get_changeset_id()).collect();
        for bonsai in &bonsais {
            all_csids.extend(bonsai.parents());
        }
        all_csids.sort();
        all_csids.dedup();

        let unode_outputs = RootUnodeManifestId::fetch_stage_outputs(
            ctx,
            derivation,
            &StageId::Manifest(stage_path.clone()),
            all_csids,
        )
        .await?;

        let mut results = HashMap::new();

        // Process commits in the given topological order: per-commit batches are
        // written before moving on, so an intra-batch ancestor's batch is
        // available when chaining its descendant within stage S.
        for bonsai in &bonsais {
            let csid = bonsai.get_changeset_id();

            let own_output = *unode_outputs.get(&csid).ok_or_else(|| {
                anyhow!("missing unode stage output for {csid} at stage {stage_path}")
            })?;
            let subtree = match own_output {
                Some(Entry::Tree(subtree)) => subtree,
                Some(Entry::Leaf(file_unode_id)) => {
                    // The stage root itself is a file (e.g. a directory replaced
                    // by a file). Canonical fastlog records a batch for that leaf
                    // unode, but the parent stage prunes path S, so this child
                    // stage owns it. Mirror `find_intersection_of_diffs`: skip
                    // only if an identical leaf already exists in a parent.
                    let entry = Entry::Leaf(file_unode_id);
                    let reused = bonsai.parents().any(|parent_csid| {
                        matches!(
                            unode_outputs.get(&parent_csid),
                            Some(Some(parent_entry)) if *parent_entry == entry
                        )
                    });
                    if !reused {
                        let blobstore = derivation.blobstore();
                        let parents = fetch_unode_parents(ctx, blobstore, entry).await?;
                        let batch = create_new_batch_with_prefix(
                            ctx,
                            blobstore,
                            parents,
                            csid,
                            fastlog_key_prefix,
                        )
                        .await?;
                        save_fastlog_batch_by_unode_id_with_prefix(
                            ctx,
                            blobstore,
                            entry,
                            batch,
                            fastlog_key_prefix,
                        )
                        .await?;
                    }
                    results.insert(csid, ());
                    continue;
                }
                None => {
                    // No entry at this stage for this changeset: nothing to derive.
                    results.insert(csid, ());
                    continue;
                }
            };

            let parent_subtrees = resolve_parent_stage_outputs(
                ctx,
                derivation,
                stage_path,
                bonsai.parents(),
                &unode_outputs,
            )
            .await?;
            let parent_subtree_ids: Vec<ManifestUnodeId> = parent_subtrees
                .values()
                .filter_map(|e| (*e).and_then(Entry::into_tree))
                .collect();

            let pruner_dep_paths = dep_paths_relative.clone();
            // Keep BOTH trees and leaves: fastlog derives directory unodes too.
            let changed: Vec<Entry<ManifestUnodeId, FileUnodeId>> =
                find_intersection_of_diffs_pruned(
                    ctx.clone(),
                    derivation.blobstore().clone(),
                    subtree,
                    parent_subtree_ids,
                    move |diff: &Diff<ManifestUnodeId>| {
                        let path = match diff {
                            Diff::Added(path, _)
                            | Diff::Removed(path, _)
                            | Diff::Changed(path, _, _) => path,
                        };
                        !pruner_dep_paths.iter().any(|dep| dep.is_prefix_of(path))
                    },
                )
                // The recurse pruner only gates subtrees, so a dep path resolving to
                // a leaf still reaches the output and must be dropped here.
                .try_filter_map(|(path, entry)| {
                    let keep = !dep_paths_relative.iter().any(|dep| dep.is_prefix_of(&path));
                    async move { Ok(keep.then_some(entry)) }
                })
                .try_collect()
                .await?;

            // Write the per-unode batches concurrently, chaining onto parent
            // unode batches (pipeline prefix first, canonical fallback).
            stream::iter(changed)
                .map(|entry| {
                    let ctx = ctx.clone();
                    let derivation = derivation.clone();
                    let fastlog_key_prefix = fastlog_key_prefix.to_owned();
                    async move {
                        mononoke::spawn_task(async move {
                            let blobstore = derivation.blobstore();
                            let parents = fetch_unode_parents(&ctx, blobstore, entry).await?;
                            let batch = create_new_batch_with_prefix(
                                &ctx,
                                blobstore,
                                parents,
                                csid,
                                &fastlog_key_prefix,
                            )
                            .await?;
                            save_fastlog_batch_by_unode_id_with_prefix(
                                &ctx,
                                blobstore,
                                entry,
                                batch,
                                &fastlog_key_prefix,
                            )
                            .await
                        })
                        .await?
                    }
                })
                .buffered(FASTLOG_BUFFER_SIZE)
                .try_for_each(|_| async { Ok::<_, Error>(()) })
                .await?;

            results.insert(csid, ());
        }

        Ok(results)
    }

    async fn extract_stage_output_from_derived(
        _ctx: &CoreContext,
        _derivation: &DerivationContext,
        _derived: &RootFastlog,
        _stage: &StageId,
    ) -> Result<Self::StageOutput> {
        Ok(())
    }

    async fn store_stage_outputs(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        stage: &StageId,
        outputs: HashMap<ChangesetId, Self::StageOutput>,
    ) -> Result<()> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        let use_normal_mapping = use_normal_mapping(stage_path);
        let key_prefix = derivation.mapping_key_prefix::<RootFastlog>();

        stream::iter(outputs.into_keys().map(|cs_id| async move {
            let key = if use_normal_mapping {
                // The terminal stage writes the canonical mapping: an empty
                // presence marker, matching `RootFastlog::store_mapping`.
                format_key(derivation, cs_id)
            } else {
                // Non-terminal stages write an empty presence marker; fastlog's
                // real output is the per-unode batch blobs.
                stage_blobstore_key(stage_path, key_prefix, cs_id)
            };
            derivation
                .blobstore()
                .put(ctx, key, BlobstoreBytes::empty())
                .await
        }))
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;
        Ok(())
    }

    async fn fetch_stage_outputs(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        stage: &StageId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        let use_normal_mapping = use_normal_mapping(stage_path);
        let key_prefix = derivation.mapping_key_prefix::<RootFastlog>();

        let results = stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            let key = if use_normal_mapping {
                format_key(derivation, cs_id)
            } else {
                stage_blobstore_key(stage_path, key_prefix, cs_id)
            };
            match derivation.blobstore().get(ctx, &key).await? {
                Some(_) => Ok::<_, Error>(Some((cs_id, ()))),
                None => Ok(None),
            }
        }))
        .buffered(100)
        .try_filter_map(|opt| async move { Ok(opt) })
        .try_collect::<HashMap<_, _>>()
        .await?;
        Ok(results)
    }

    async fn verify_stage(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        csid: ChangesetId,
        stage: &StageId,
    ) -> Result<bool> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        // Fastlog is verified once per commit at the root stage (which runs
        // last, so all stages' batches are present); non-root stages are no-ops.
        if !stage_path.is_root() {
            return Ok(true);
        }

        let bonsai = csid.load(ctx, derivation.blobstore()).await?;

        // The commit's changed unodes: canonical full unode root diffed against
        // the canonical parent unode roots (same set canonical fastlog derives).
        // Keep BOTH trees and leaves.
        let unode_root = derivation
            .fetch_dependency::<RootUnodeManifestId>(ctx, csid)
            .await?;
        let parent_roots: Vec<ManifestUnodeId> =
            stream::iter(bonsai.parents().map(|parent_csid| async move {
                let root = derivation
                    .fetch_dependency::<RootUnodeManifestId>(ctx, parent_csid)
                    .await?;
                Ok::<_, Error>(*root.manifest_unode_id())
            }))
            .buffered(100)
            .try_collect()
            .await?;

        let changed: Vec<(MPath, Entry<ManifestUnodeId, FileUnodeId>)> =
            find_intersection_of_diffs(
                ctx.clone(),
                derivation.blobstore().clone(),
                *unode_root.manifest_unode_id(),
                parent_roots,
            )
            .try_collect()
            .await?;

        // Data equality: per changed unode, the pipeline-side batch blob must
        // match the canonical batch blob.
        let pipeline_prefix = pipeline_fastlog_key_prefix();
        let blobstore = derivation.blobstore();
        for (_path, entry) in changed {
            let (pipeline_batch, canonical_batch) = futures::future::try_join(
                fetch_fastlog_batch_by_unode_id_with_prefix(
                    ctx,
                    blobstore,
                    &entry,
                    pipeline_prefix,
                ),
                fetch_fastlog_batch_by_unode_id_with_prefix(ctx, blobstore, &entry, ""),
            )
            .await?;
            match (pipeline_batch, canonical_batch) {
                (Some(p), Some(c)) if p == c => {}
                _ => return Ok(false),
            }
        }

        Ok(true)
    }
}
