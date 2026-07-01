/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

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
use mononoke_types::NonRootMPath;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::blame_v2::load_blame_with_prefix;
use mononoke_types::path::MPath;
use unodes::RootUnodeManifestId;
use unodes::find_stage_unode_rename_sources;
use unodes::find_unode_rename_sources;
use unodes::resolve_parent_stage_outputs;

use crate::DEFAULT_BLAME_FILESIZE_LIMIT;
use crate::RootBlameV2;
use crate::derive_v2::create_blame_v2;
use crate::format_key;

/// Concurrency for per-file blame writes, matching the canonical
/// `derive_blame_v2` buffering.
const BLAME_BUFFER_SIZE: usize = 256;

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "derived_root_blame_v2_stage.{}.{}{}",
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
            Some("blame"),
        )
}

/// Prefix for pipeline blame blob writes/reads, gated on the same prod-mapping
/// JustKnob as the terminal mapping. After the flip the pipeline writes the
/// canonical content-addressed key (empty prefix, becoming source of truth);
/// before it, blame is namespaced. Applies at every stage (blame blobs are
/// keyed by file unode, so there is no cross-stage key collision).
fn pipeline_blame_key_prefix() -> &'static str {
    if justknobs::eval(
        "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
        None,
        Some("blame"),
    ) {
        ""
    } else {
        "pipeline."
    }
}

/// Whether stage S must resolve renames against the full root: subtree changes,
/// or a copy into S from outside S (both only occur on chokepoint commits).
pub(crate) fn is_chokepoint(bonsai: &BonsaiChangeset, stage_path: &MPath) -> bool {
    bonsai.has_subtree_changes()
        || bonsai.file_changes().any(|(dest, fc)| {
            matches!(
                fc.copy_from(),
                Some((from, _)) if stage_path.is_prefix_of(dest) && !stage_path.is_prefix_of(from)
            )
        })
}

#[async_trait]
impl PipelineDerivable for RootBlameV2 {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant = PipelineDerivableVariant::BlameV2;

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
        let dep_paths_relative: Vec<MPath> = payload
            .deps
            .iter()
            .map(|element| MPath::ROOT.join(std::iter::once(element)))
            .collect();

        let filesize_limit = derivation
            .config()
            .blame_filesize_limit
            .unwrap_or(DEFAULT_BLAME_FILESIZE_LIMIT);

        // Namespace pipeline blame blobs until the prod-mapping flip.
        let blame_key_prefix = pipeline_blame_key_prefix();

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

        for bonsai in &bonsais {
            let csid = bonsai.get_changeset_id();

            let own_output = *unode_outputs.get(&csid).ok_or_else(|| {
                anyhow!("missing unode stage output for {csid} at stage {stage_path}")
            })?;

            // Resolve each parent's stage-S output once; reused by the tree-arm
            // diff base and by the stage-scoped rename resolver below.
            let parent_stage_outputs = resolve_parent_stage_outputs(
                ctx,
                derivation,
                stage_path,
                bonsai.parents(),
                &unode_outputs,
            )
            .await?;

            // The rename map for this changeset. Chokepoint commits (subtree
            // changes or a copy into S from outside S) resolve against the full
            // unode root; everything else resolves against the parents' stage-S
            // outputs. Either way `create_blame_v2` looks up each file itself.
            let renames = Arc::new(if is_chokepoint(bonsai, stage_path) {
                find_unode_rename_sources(ctx, derivation, bonsai).await?
            } else {
                find_stage_unode_rename_sources(
                    ctx,
                    derivation,
                    stage_path,
                    bonsai,
                    &parent_stage_outputs,
                )
                .await?
            });

            let subtree = match own_output {
                Some(Entry::Tree(subtree)) => subtree,
                Some(Entry::Leaf(file_unode_id)) => {
                    // The stage root itself is a file (e.g. a directory replaced
                    // by a file). Canonical blame blames that leaf, but the
                    // parent stage prunes path S, so this child stage owns it.
                    // Skip only if an identical leaf already exists in a parent
                    // (mirrors `find_intersection_of_diffs`).
                    let entry = Entry::Leaf(file_unode_id);
                    let reused = bonsai.parents().any(|parent_csid| {
                        matches!(
                            unode_outputs.get(&parent_csid),
                            Some(Some(parent_entry)) if *parent_entry == entry
                        )
                    });
                    if !reused {
                        let abs_path: NonRootMPath = stage_path
                            .clone()
                            .into_optional_non_root_path()
                            .ok_or_else(|| {
                                anyhow!("stage-root leaf blame path resolved to root")
                            })?;
                        create_blame_v2(
                            ctx,
                            derivation,
                            derivation.blobstore(),
                            renames.clone(),
                            csid,
                            abs_path,
                            file_unode_id,
                            filesize_limit,
                            blame_key_prefix,
                        )
                        .await?;
                    }
                    results.insert(csid, ());
                    continue;
                }
                None => {
                    // No entry at this stage for this changeset: nothing to blame.
                    results.insert(csid, ());
                    continue;
                }
            };

            let parent_subtree_ids: Vec<ManifestUnodeId> = parent_stage_outputs
                .values()
                .filter_map(|e| (*e).and_then(Entry::into_tree))
                .collect();

            let pruner_dep_paths = dep_paths_relative.clone();
            let changed = find_intersection_of_diffs_pruned(
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
            // Tree-typed entries are intentionally dropped via `into_leaf()?`: a
            // path that became a directory is not blamed (matches canonical
            // `derive_blame_v2`).
            .map_ok(|(path, entry)| Some((Option::<NonRootMPath>::from(path)?, entry.into_leaf()?)))
            .try_filter_map(|x| async move { Ok(x) })
            .try_collect::<Vec<(NonRootMPath, FileUnodeId)>>()
            .await?
            .into_iter()
            // The recurse pruner only gates subtrees, so a dep path resolving to
            // a leaf still reaches the output and must be dropped here.
            .filter(|(relative_path, _)| {
                let path: MPath = relative_path.clone().into();
                !dep_paths_relative.iter().any(|dep| dep.is_prefix_of(&path))
            })
            .collect::<Vec<(NonRootMPath, FileUnodeId)>>();

            // Write the per-file blame concurrently, mirroring the canonical
            // `derive_blame_v2` (spawn each blame computation, buffered).
            // `create_blame_v2` looks up the rename for each file internally.
            stream::iter(changed)
                .map(|(relative_path, file_unode_id)| {
                    let ctx = ctx.clone();
                    let derivation = derivation.clone();
                    let blame_key_prefix = blame_key_prefix.to_owned();
                    let renames = renames.clone();
                    async move {
                        // Re-prefix the relative path with the stage path to get
                        // the absolute path used for blame storage and lookup.
                        let abs_path: NonRootMPath = stage_path
                            .join(relative_path.as_ref())
                            .into_optional_non_root_path()
                            .ok_or_else(|| anyhow!("re-prefixed blame path resolved to root"))?;
                        mononoke::spawn_task(async move {
                            create_blame_v2(
                                &ctx,
                                &derivation,
                                derivation.blobstore(),
                                renames,
                                csid,
                                abs_path,
                                file_unode_id,
                                filesize_limit,
                                &blame_key_prefix,
                            )
                            .await
                        })
                        .await?
                    }
                })
                .buffered(BLAME_BUFFER_SIZE)
                .try_for_each(|_| async { Ok::<_, Error>(()) })
                .await?;

            results.insert(csid, ());
        }

        Ok(results)
    }

    async fn extract_stage_output_from_derived(
        _ctx: &CoreContext,
        _derivation: &DerivationContext,
        _derived: &RootBlameV2,
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
        let key_prefix = derivation.mapping_key_prefix::<RootBlameV2>();

        stream::iter(outputs.into_keys().map(|cs_id| async move {
            if use_normal_mapping {
                // The terminal stage writes the canonical mapping: a pointer to
                // the full unode root, matching the non-pipeline derived value.
                // The unode root comes from the root stage output (guaranteed by
                // the same-stage cross-type edge), not the canonical mapping.
                let unode_outputs = RootUnodeManifestId::fetch_stage_outputs(
                    ctx,
                    derivation,
                    &StageId::Manifest(MPath::ROOT),
                    vec![cs_id],
                )
                .await?;
                let Some(Some(Entry::Tree(mf_unode_id))) = unode_outputs.get(&cs_id).copied()
                else {
                    return Err(anyhow!(
                        "terminal stage for {cs_id}: missing unode root stage output"
                    ));
                };
                let key = format_key(derivation, cs_id);
                derivation
                    .blobstore()
                    .put(ctx, key, RootUnodeManifestId(mf_unode_id).into())
                    .await
            } else {
                // Non-terminal stages write an empty presence marker; blame's
                // real output is the per-file blame blobs.
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                derivation
                    .blobstore()
                    .put(ctx, key, BlobstoreBytes::empty())
                    .await
            }
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
        let key_prefix = derivation.mapping_key_prefix::<RootBlameV2>();

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
        // Blame is verified once per commit at the root stage (which runs last,
        // so all stages' blame is present); non-root stages are no-ops.
        if !stage_path.is_root() {
            return Ok(true);
        }

        let bonsai = csid.load(ctx, derivation.blobstore()).await?;

        // The commit's changed files: canonical full unode root diffed against
        // the canonical parent unode roots (same set canonical blame derives).
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

        let changed: Vec<(NonRootMPath, FileUnodeId)> = find_intersection_of_diffs(
            ctx.clone(),
            derivation.blobstore().clone(),
            *unode_root.manifest_unode_id(),
            parent_roots,
        )
        .map_ok(|(path, entry)| Some((Option::<NonRootMPath>::from(path)?, entry.into_leaf()?)))
        .try_filter_map(|x| async move { Ok(x) })
        .try_collect::<Vec<_>>()
        .await?;

        // Data equality: per changed file, the pipeline-side blame blob must
        // match the canonical blame blob.
        let pipeline_prefix = pipeline_blame_key_prefix();
        for (_path, file_unode_id) in changed {
            let (pipeline_blame, canonical_blame) = futures::future::try_join(
                load_blame_with_prefix(ctx, derivation.blobstore(), file_unode_id, pipeline_prefix),
                load_blame_with_prefix(ctx, derivation.blobstore(), file_unode_id, ""),
            )
            .await?;
            match (pipeline_blame, canonical_blame) {
                (Some(p), Some(c)) if p == c => {}
                _ => return Ok(false),
            }
        }

        Ok(true)
    }
}
