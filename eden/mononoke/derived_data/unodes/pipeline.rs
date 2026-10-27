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
use derived_data_manager::DerivationContext;
use derived_data_manager::ManifestDerivable;
use derived_data_manager::ManifestDerivation;
use derived_data_manager::PipelineDerivable;
use derived_data_manager::StageId;
use fbthrift::compact_protocol;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures::future;
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
use crate::mapping::get_file_changes;

#[async_trait]
impl ManifestDerivable for RootUnodeManifestId {
    type TreeId = ManifestUnodeId;
    type Leaf = FileUnodeId;

    const PIPELINE_VARIANT: PipelineDerivableVariant = PipelineDerivableVariant::Unodes;
    const STAGE_KEY_PREFIX: &'static str = "derived_unode_stage";

    async fn load_root_manifest(
        &self,
        _ctx: &CoreContext,
        _derivation: &DerivationContext,
    ) -> Result<ManifestUnodeId> {
        Ok(self.0)
    }

    async fn from_root_manifest(
        _ctx: &CoreContext,
        _derivation: &DerivationContext,
        root: ManifestUnodeId,
    ) -> Result<Self> {
        Ok(Self(root))
    }

    async fn derive_manifest(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsai: BonsaiChangeset,
        input: ManifestDerivation<ManifestUnodeId, FileUnodeId>,
    ) -> Result<Option<Entry<ManifestUnodeId, FileUnodeId>>> {
        derive_unode_entry(
            ctx,
            derivation,
            bonsai.get_changeset_id(),
            input,
            get_file_changes(&bonsai),
        )
        .await
    }

    fn encode_entry(entry: Option<Entry<ManifestUnodeId, FileUnodeId>>) -> BlobstoreBytes {
        let entry = match entry {
            Some(Entry::Tree(mf_unode_id)) => {
                unodes_thrift::UnodeStageOutput::manifest_unode_id(mf_unode_id.into_thrift())
            }
            Some(Entry::Leaf(file_unode_id)) => {
                unodes_thrift::UnodeStageOutput::file_unode_id(file_unode_id.into_thrift())
            }
            None => unodes_thrift::UnodeStageOutput::empty(unodes_thrift::UnodeStageOutputEmpty {
                ..Default::default()
            }),
        };
        BlobstoreBytes::from_bytes(compact_protocol::serialize(&entry))
    }

    fn decode_entry(bytes: BlobstoreBytes) -> Result<Option<Entry<ManifestUnodeId, FileUnodeId>>> {
        let thrift_output: unodes_thrift::UnodeStageOutput =
            compact_protocol::deserialize(bytes.into_bytes())?;
        Ok(match thrift_output {
            unodes_thrift::UnodeStageOutput::manifest_unode_id(id) => {
                Some(Entry::Tree(ManifestUnodeId::from_thrift(id)?))
            }
            unodes_thrift::UnodeStageOutput::file_unode_id(id) => {
                Some(Entry::Leaf(FileUnodeId::from_thrift(id)?))
            }
            unodes_thrift::UnodeStageOutput::empty(_) => None,
            unodes_thrift::UnodeStageOutput::UnknownField(x) => {
                return Err(anyhow!("unknown UnodeStageOutput variant {x}"));
            }
        })
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
