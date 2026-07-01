/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use derived_data_manager::DerivationStagePayload;
use derived_data_manager::PipelineDerivable;
use derived_data_manager::StageId;
use fbthrift::compact_protocol;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures::future;
use futures::stream;
use manifest::Entry;
use manifest::ManifestOps;
use manifest::PathTree;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::ManifestUnodeId;
use mononoke_types::NonRootMPath;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::path::MPath;
use mononoke_types::thrift::unodes as unodes_thrift;

use crate::CopyInfoSource;
use crate::RootUnodeManifestId;
use crate::UnodeRenameSources;
use crate::derive::derive_unode_entry;
use crate::mapping::format_key;
use crate::mapping::get_file_changes;

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "derived_unode_stage.{}.{}{}",
        stage_path.get_path_hash().to_hex(),
        key_prefix,
        cs_id,
    )
}

#[async_trait]
impl PipelineDerivable for RootUnodeManifestId {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant = PipelineDerivableVariant::Unodes;

    const HAS_FINALIZE: bool = false;

    type StageOutput = Option<Entry<ManifestUnodeId, FileUnodeId>>;

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

        let mut results = HashMap::new();

        for bonsai in bonsais {
            let cs_id = bonsai.get_changeset_id();
            let mut all_changes = get_file_changes(&bonsai);

            // Build parent entries for this changeset.
            let parent_entries: Vec<Entry<ManifestUnodeId, FileUnodeId>> = bonsai
                .parents()
                .map(|parent_csid| {
                    let output = results
                        .get(&parent_csid)
                        .or_else(|| parents.get(&parent_csid))
                        .ok_or_else(|| anyhow!("missing stage output for parent {parent_csid}"))?;
                    Ok(output.clone())
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();

            // Build known_entries from dependency stage outputs. The manager
            // keys dependency_outputs by absolute dep path, so we can copy
            // entries straight in.
            let known_entries: HashMap<MPath, Option<Entry<ManifestUnodeId, FileUnodeId>>> =
                dependency_outputs
                    .get(&cs_id)
                    .map(|deps| {
                        deps.iter()
                            .map(|(dep_path, dep_output)| (dep_path.clone(), dep_output.clone()))
                            .collect()
                    })
                    .unwrap_or_default();

            let (mut additional_changes, manifest_replacements) =
                crate::derive::get_unode_subtree_changes(
                    ctx,
                    derivation,
                    None,
                    bonsai.subtree_changes(),
                    &all_changes,
                )
                .await?;
            all_changes.append(&mut additional_changes);

            let entry = derive_unode_entry(
                ctx,
                derivation,
                cs_id,
                parent_entries,
                all_changes,
                manifest_replacements,
                known_entries,
                stage_path.clone(),
            )
            .await?;

            results.insert(cs_id, entry);
        }

        Ok(results)
    }

    async fn extract_stage_output_from_derived(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        derived: &RootUnodeManifestId,
        stage: &StageId,
    ) -> Result<Self::StageOutput> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        Ok(derived
            .manifest_unode_id()
            .find_entry(
                ctx.clone(),
                derivation.blobstore().clone(),
                stage_path.clone(),
            )
            .await?)
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
        let use_normal_mapping = stage_path.is_root()
            && justknobs::eval(
                "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
                None,
                Some("unodes"),
            );

        let key_prefix = derivation.mapping_key_prefix::<RootUnodeManifestId>();
        stream::iter(outputs.into_iter().map(|(cs_id, output)| async move {
            if use_normal_mapping {
                let Some(Entry::Tree(mf_unode_id)) = output else {
                    return Err(anyhow!(
                        "terminal stage output for {cs_id} should be Some(Entry::Tree), got {output:?}",
                    ));
                };
                let key = format_key(derivation, cs_id);
                derivation
                    .blobstore()
                    .put(ctx, key, RootUnodeManifestId(mf_unode_id).into())
                    .await
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let thrift_output = match output {
                    Some(Entry::Tree(mf_unode_id)) => {
                        unodes_thrift::UnodeStageOutput::manifest_unode_id(
                            mf_unode_id.into_thrift(),
                        )
                    }
                    Some(Entry::Leaf(file_unode_id)) => {
                        unodes_thrift::UnodeStageOutput::file_unode_id(file_unode_id.into_thrift())
                    }
                    None => unodes_thrift::UnodeStageOutput::empty(
                        unodes_thrift::UnodeStageOutputEmpty {
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
        let use_normal_mapping = stage_path.is_root()
            && justknobs::eval(
                "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
                None,
                Some("unodes"),
            );

        let key_prefix = derivation.mapping_key_prefix::<RootUnodeManifestId>();
        let results = stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            if use_normal_mapping {
                let key = format_key(derivation, cs_id);
                let Some(blob_data) = derivation.blobstore().get(ctx, &key).await? else {
                    return Ok::<_, Error>(None);
                };
                let root: RootUnodeManifestId = blob_data.try_into()?;
                Ok(Some((cs_id, Some(Entry::Tree(*root.manifest_unode_id())))))
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let maybe_bytes = derivation.blobstore().get(ctx, &key).await?;
                match maybe_bytes {
                    None => Ok::<_, Error>(None),
                    Some(blob_data) => {
                        let thrift_output: unodes_thrift::UnodeStageOutput =
                            compact_protocol::deserialize(blob_data.into_raw_bytes())?;
                        let output = match thrift_output {
                            unodes_thrift::UnodeStageOutput::manifest_unode_id(id) => {
                                Some(Entry::Tree(ManifestUnodeId::from_thrift(id)?))
                            }
                            unodes_thrift::UnodeStageOutput::file_unode_id(id) => {
                                Some(Entry::Leaf(FileUnodeId::from_thrift(id)?))
                            }
                            unodes_thrift::UnodeStageOutput::empty(_) => None,
                            unodes_thrift::UnodeStageOutput::UnknownField(x) => {
                                return Err(anyhow!(
                                    "unknown UnodeStageOutput variant {x} for {cs_id}"
                                ));
                            }
                        };
                        Ok(Some((cs_id, output)))
                    }
                }
            }
        }))
        .buffered(100)
        .try_filter_map(|opt| async move { Ok(opt) })
        .try_collect::<HashMap<_, _>>()
        .await?;
        Ok(results)
    }
}

/// Resolve each parent's stage-S unode stage output for a downstream pipeline
/// type (fastlog, blame) whose own `StageOutput` is `()`.
///
/// Returns the resolved stage output (Tree, Leaf, or None) for every parent.
/// Unlike the non-pipeline resolver, which always works from the repo-root tree,
/// a stage root may be a file (Leaf) or absent (None), so callers must handle
/// all three shapes rather than assuming a tree.
///
/// `unode_outputs` is the result of `RootUnodeManifestId::fetch_stage_outputs`.
/// For each parent: a stored stage output is used directly; an absent output
/// means the parent's unode stage was never derived, so it is bridged to the
/// canonical value and a missing canonical value errors loudly.
pub async fn resolve_parent_stage_outputs(
    ctx: &CoreContext,
    derivation: &DerivationContext,
    stage_path: &MPath,
    parent_csids: impl IntoIterator<Item = ChangesetId>,
    unode_outputs: &HashMap<ChangesetId, Option<Entry<ManifestUnodeId, FileUnodeId>>>,
) -> Result<HashMap<ChangesetId, Option<Entry<ManifestUnodeId, FileUnodeId>>>> {
    let mut subtrees = HashMap::new();
    for parent_csid in parent_csids {
        let output = match unode_outputs.get(&parent_csid) {
            Some(output) => output.clone(),
            None => {
                let derived = derivation
                    .fetch_dependency::<RootUnodeManifestId>(ctx, parent_csid)
                    .await
                    .with_context(|| {
                        format!(
                            "missing unode stage output for parent {parent_csid} at stage {stage_path}"
                        )
                    })?;
                RootUnodeManifestId::extract_stage_output_from_derived(
                    ctx,
                    derivation,
                    &derived,
                    &StageId::Manifest(stage_path.clone()),
                )
                .await?
            }
        };
        subtrees.insert(parent_csid, output);
    }
    Ok(subtrees)
}

/// Stage-scoped sibling of `find_unode_rename_sources`: resolves copy sources
/// against the parents' stage-S outputs instead of their full unode roots.
///
/// Only called for non-chokepoint commits (no subtree changes, no copy into S
/// from outside S), so the copy source is always inside S and `subtree_ops` is
/// empty. Batched like `find_unode_rename_sources`: dest paths are grouped by
/// source parent and resolved with one `find_entries` per parent tree, with
/// parents resolved concurrently.
pub async fn find_stage_unode_rename_sources(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    stage_path: &MPath,
    bonsai: &BonsaiChangeset,
    parent_stage_outputs: &HashMap<ChangesetId, Option<Entry<ManifestUnodeId, FileUnodeId>>>,
) -> Result<UnodeRenameSources, Error> {
    let mut copy_info: HashMap<NonRootMPath, CopyInfoSource> = HashMap::new();

    // Per source parent, the tree lookups still to resolve: the parent's stage-S
    // subtree plus a list of (relative source path, dest path, parent_index).
    let mut tree_lookups: HashMap<
        ChangesetId,
        (
            ManifestUnodeId,
            Vec<(MPath, NonRootMPath, NonRootMPath, usize)>,
        ),
    > = HashMap::new();

    for (to_path, file_change) in bonsai.file_changes() {
        let Some((from_path, csid)) = file_change.copy_from() else {
            continue;
        };
        let parent_index = bonsai.parents().position(|p| p == *csid).ok_or_else(|| {
            anyhow!(
                "bonsai changeset {} contains invalid copy from parent: {}",
                bonsai.get_changeset_id(),
                csid
            )
        })?;
        // Source outside stage S: its dest is also outside S, irrelevant to S's blame.
        if !stage_path.is_prefix_of(from_path) {
            continue;
        }
        let parent_output = parent_stage_outputs.get(csid).copied().flatten();
        let relative = MPath::from(from_path.clone()).remove_prefix_component(stage_path);
        if relative.is_root() {
            // Copy source is the stage root itself; a rename results only if the
            // parent's stage root is a file. No blobstore lookup needed.
            if let Some(Entry::Leaf(unode_id)) = parent_output {
                copy_info.insert(
                    to_path.clone(),
                    CopyInfoSource {
                        parent_index,
                        from_path: from_path.clone(),
                        unode_id,
                    },
                );
            }
        } else if let Some(Entry::Tree(subtree)) = parent_output {
            // Copy source is strictly under the stage root; batch the lookup
            // inside the parent's stage-S subtree.
            tree_lookups
                .entry(*csid)
                .or_insert_with(|| (subtree, Vec::new()))
                .1
                .push((relative, from_path.clone(), to_path.clone(), parent_index));
        }
    }

    let blobstore = derivation_ctx.blobstore();
    let resolved_futs = tree_lookups.into_iter().map(|(_csid, (subtree, lookups))| {
        cloned!(blobstore);
        async move {
            let relatives: Vec<MPath> = lookups.iter().map(|(rel, _, _, _)| rel.clone()).collect();
            let entries: HashMap<MPath, Entry<ManifestUnodeId, FileUnodeId>> = subtree
                .find_entries(ctx.clone(), blobstore, relatives)
                .try_collect::<HashMap<_, _>>()
                .await?;
            let mut sources = Vec::new();
            for (relative, from_path, to_path, parent_index) in lookups {
                if let Some(unode_id) = entries.get(&relative).copied().and_then(Entry::into_leaf) {
                    sources.push((
                        to_path,
                        CopyInfoSource {
                            parent_index,
                            from_path,
                            unode_id,
                        },
                    ));
                }
            }
            anyhow::Ok(sources)
        }
    });

    let resolved: Vec<(NonRootMPath, CopyInfoSource)> = future::try_join_all(resolved_futs)
        .map_ok(|sources| sources.into_iter().flatten().collect())
        .await?;
    copy_info.extend(resolved);

    // Non-chokepoint commits have no subtree changes, so there are no subtree ops.
    Ok(UnodeRenameSources {
        copy_info,
        subtree_ops: PathTree::default(),
    })
}
