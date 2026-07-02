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
use fbthrift::compact_protocol;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use manifest::BonsaiDiffFileChange;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathTree;
use manifest::bonsai_diff;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DeletedManifestV2Id;
use mononoke_types::FileUnodeId;
use mononoke_types::ManifestUnodeId;
use mononoke_types::NonRootMPath;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::deleted_manifest_common::DeletedManifestCommon;
use mononoke_types::deleted_manifest_v2::DeletedManifestV2;
use mononoke_types::path::MPath;
use mononoke_types::thrift::deleted_manifest as dm_thrift;
use mononoke_types::unode::UnodeEntry;
use multimap::MultiMap;
use unodes::RootUnodeManifestId;
use unodes::resolve_parent_stage_outputs;

use crate::RootDeletedManifestV2Id;
use crate::derive::DeletedManifestDeriver;
use crate::mapping::RootDeletedManifestIdCommon;
use crate::mapping_v2::format_key;

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "derived_deleted_manifest2_stage.{}.{}{}",
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
            Some("deleted_manifests"),
        )
}

/// Unode stage entry -> deleted manifest's `UnodeEntry`.
fn to_unode_entry(entry: &Entry<ManifestUnodeId, FileUnodeId>) -> UnodeEntry {
    match entry {
        Entry::Tree(id) => UnodeEntry::Directory(*id),
        Entry::Leaf(id) => UnodeEntry::File(*id),
    }
}

/// Change set under a stage for a subtree-altering (chokepoint) commit. Subtree
/// ops move paths that aren't listed in `file_changes`, so we recompute the same
/// way canonical `get_changes_bonsai` does — by diffing unode trees — but scoped
/// to the stage subtree. Returned paths are relative to the stage root. Stays
/// within the stage unode outputs (the declared dependency), never the root.
async fn subtree_op_change_set(
    ctx: &CoreContext,
    blobstore: &std::sync::Arc<dyn blobstore::KeyedBlobstore>,
    current_unode: Option<&UnodeEntry>,
    parent_unode_outputs: &HashMap<ChangesetId, Option<Entry<ManifestUnodeId, FileUnodeId>>>,
) -> Result<PathTree<()>> {
    let parent_trees: Vec<ManifestUnodeId> = parent_unode_outputs
        .values()
        .filter_map(|entry| (*entry).and_then(Entry::into_tree))
        .collect();
    match current_unode {
        // Diff the current stage tree against the parent stage trees; captures
        // paths moved into/within the stage by the subtree op.
        Some(UnodeEntry::Directory(cur_tree)) => {
            let changed: Vec<NonRootMPath> = bonsai_diff(
                ctx.clone(),
                blobstore.clone(),
                *cur_tree,
                parent_trees.into_iter().collect(),
            )
            .map_ok(|diff| match diff {
                BonsaiDiffFileChange::Changed(path, _)
                | BonsaiDiffFileChange::ChangedReusedId(path, _)
                | BonsaiDiffFileChange::Deleted(path) => path,
            })
            .try_collect()
            .await?;
            Ok(PathTree::from_iter(
                changed.into_iter().map(|path| (path, ())),
            ))
        }
        // The stage root became a file: `do_unfold` expands the
        // file-replacing-directory case itself from the parent trees.
        Some(UnodeEntry::File(_)) => Ok(PathTree::default()),
        // The stage subtree was removed entirely: every parent path is deleted.
        None => {
            let mut paths: Vec<NonRootMPath> = Vec::new();
            for parent_tree in parent_trees {
                let mut entries: Vec<NonRootMPath> = parent_tree
                    .list_all_entries(ctx.clone(), blobstore.clone())
                    .try_filter_map(
                        |(path, _)| async move { Ok(Option::<NonRootMPath>::from(path)) },
                    )
                    .try_collect()
                    .await?;
                paths.append(&mut entries);
            }
            Ok(PathTree::from_iter(
                paths.into_iter().map(|path| (path, ())),
            ))
        }
    }
}

#[async_trait]
impl PipelineDerivable for RootDeletedManifestV2Id {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant =
        PipelineDerivableVariant::DeletedManifests;

    const HAS_FINALIZE: bool = false;

    type StageOutput = Option<DeletedManifestV2Id>;

    async fn derive_stage_batch(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        payload: &DerivationStagePayload,
        parents: HashMap<ChangesetId, Self::StageOutput>,
        dependency_outputs: HashMap<ChangesetId, HashMap<MPath, Self::StageOutput>>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let DerivationStagePayload::Manifest(payload) = payload else {
            anyhow::bail!("{} has no finalize derive", Self::NAME);
        };
        let stage_path = &payload.path;
        let blobstore = derivation.blobstore();

        // Fetch unode stage-S subtrees for the batch and all their parents in a
        // single call (the same-stage cross-type edge guarantees they exist).
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

        let mut results: HashMap<ChangesetId, Option<DeletedManifestV2Id>> = HashMap::new();

        for bonsai in &bonsais {
            let cs_id = bonsai.get_changeset_id();

            // Current unode subtree at S.
            let current_unode = unode_outputs
                .get(&cs_id)
                .ok_or_else(|| {
                    anyhow!("missing unode stage output for {cs_id} at stage {stage_path}")
                })?
                .as_ref()
                .map(to_unode_entry);

            // Parent unode subtrees at S (bridges canonical-only parents).
            let parent_unode_outputs = resolve_parent_stage_outputs(
                ctx,
                derivation,
                stage_path,
                bonsai.parents(),
                &unode_outputs,
            )
            .await?;
            let parent_unodes: MultiMap<UnodeEntry, ChangesetId> = parent_unode_outputs
                .iter()
                .filter_map(|(cs, entry)| entry.as_ref().map(|entry| (to_unode_entry(entry), *cs)))
                .collect();

            // Parent deleted manifest subtrees at S, preferring in-batch results.
            // Every parent must have a stage output (a missing one is a broken
            // invariant, not an empty subtree); a parent whose output is None
            // simply has no deleted manifest here and contributes nothing.
            let mut parent_dms: MultiMap<DeletedManifestV2Id, ChangesetId> = MultiMap::new();
            for p in bonsai.parents() {
                let out = results
                    .get(&p)
                    .copied()
                    .or_else(|| parents.get(&p).copied())
                    .ok_or_else(|| {
                        anyhow!("missing deleted manifest stage output for parent {p} of {cs_id}")
                    })?;
                if let Some(id) = out {
                    parent_dms.insert(id, p);
                }
            }

            // Change set under S, relative to S. Subtree-altering (chokepoint)
            // commits move paths not listed in `file_changes`, so diff the stage
            // unode trees; everything else reads `file_changes` directly.
            let changes: PathTree<()> = if bonsai.has_manifest_altering_subtree_changes() {
                subtree_op_change_set(
                    ctx,
                    blobstore,
                    current_unode.as_ref(),
                    &parent_unode_outputs,
                )
                .await?
            } else {
                PathTree::from_iter(
                    bonsai
                        .file_changes()
                        .filter_map(|(path, _)| path.remove_prefix_component(stage_path))
                        .map(|rel| (rel, ())),
                )
            };

            // Child-stage boundaries, keyed relative to S.
            let known_entries: HashMap<MPath, Option<DeletedManifestV2Id>> = dependency_outputs
                .get(&cs_id)
                .map(|deps| {
                    deps.iter()
                        .map(|(dep_path, out)| (dep_path.remove_prefix_component(stage_path), *out))
                        .collect()
                })
                .unwrap_or_default();

            let out = DeletedManifestDeriver::<DeletedManifestV2>::derive_subtree(
                ctx,
                blobstore,
                cs_id,
                changes,
                parent_dms,
                current_unode,
                parent_unodes,
                known_entries,
                // The terminal stage materializes an empty root to match
                // canonical derivation; non-terminal stages leave it None.
                stage_path.is_root(),
            )
            .await?;

            results.insert(cs_id, out);
        }

        Ok(results)
    }

    async fn extract_stage_output_from_derived(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        derived: &RootDeletedManifestV2Id,
        stage: &StageId,
    ) -> Result<Self::StageOutput> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        // Walk the canonical deleted manifest down to `stage_path` and return the
        // node id there, whether or not it is itself deleted. `DeletedManifestOps::find_entry`
        // can't be used: it only yields nodes with a linknode set, so it would
        // return `None` for a live directory node that merely has deleted
        // descendants (and for the root), whereas the stage output is that node.
        let blobstore = derivation.blobstore();
        let mut current_id = *derived.id();
        for element in stage_path {
            let node = current_id.load(ctx, blobstore).await?;
            match node.lookup(ctx, blobstore, element).await? {
                Some(child_id) => current_id = child_id,
                None => return Ok(None),
            }
        }
        Ok(Some(current_id))
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
        let key_prefix = derivation.mapping_key_prefix::<RootDeletedManifestV2Id>();

        stream::iter(outputs.into_iter().map(|(cs_id, output)| async move {
            if use_normal_mapping {
                // Canonical deleted manifest always has a (possibly empty) root,
                // so the terminal stage output must be present.
                let Some(id) = output else {
                    return Err(anyhow!("terminal stage output for {cs_id} is None"));
                };
                let key = format_key(derivation, cs_id);
                derivation
                    .blobstore()
                    .put(ctx, key, RootDeletedManifestV2Id::new(id).into())
                    .await
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let thrift_output = match output {
                    Some(id) => {
                        dm_thrift::DeletedManifestStageOutput::deleted_manifest_id(id.into_thrift())
                    }
                    None => dm_thrift::DeletedManifestStageOutput::empty(
                        dm_thrift::DeletedManifestStageOutputEmpty {
                            ..Default::default()
                        },
                    ),
                };
                let bytes = compact_protocol::serialize(&thrift_output);
                derivation
                    .blobstore()
                    .put(ctx, key, BlobstoreBytes::from_bytes(bytes))
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
        let key_prefix = derivation.mapping_key_prefix::<RootDeletedManifestV2Id>();

        let results = stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            if use_normal_mapping {
                let key = format_key(derivation, cs_id);
                let Some(blob) = derivation.blobstore().get(ctx, &key).await? else {
                    return Ok::<_, Error>(None);
                };
                let root: RootDeletedManifestV2Id = blob.try_into()?;
                Ok(Some((cs_id, Some(*root.id()))))
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let Some(blob) = derivation.blobstore().get(ctx, &key).await? else {
                    return Ok::<_, Error>(None);
                };
                let thrift_output: dm_thrift::DeletedManifestStageOutput =
                    compact_protocol::deserialize(blob.into_raw_bytes())?;
                let output = match thrift_output {
                    dm_thrift::DeletedManifestStageOutput::deleted_manifest_id(id) => {
                        Some(DeletedManifestV2Id::from_thrift(id)?)
                    }
                    dm_thrift::DeletedManifestStageOutput::empty(_) => None,
                    dm_thrift::DeletedManifestStageOutput::UnknownField(x) => {
                        return Err(anyhow!(
                            "unknown DeletedManifestStageOutput variant {x} for {cs_id}"
                        ));
                    }
                };
                Ok(Some((cs_id, output)))
            }
        }))
        .buffered(100)
        .try_filter_map(|opt| async move { Ok(opt) })
        .try_collect::<HashMap<_, _>>()
        .await?;
        Ok(results)
    }
}
