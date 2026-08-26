/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use acl_manifest::RootAclManifestId;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::KeyedBlobstore;
use blobstore::Loadable;
use context::CoreContext;
use derived_data::prefetch_content_metadata;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use derived_data_manager::DerivationStagePayload;
use derived_data_manager::PipelineDerivable;
use derived_data_manager::StageId;
use fbthrift::compact_protocol;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use manifest::Entry;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::sharded_augmented_manifest::HgAugmentedDirectoryNode;
use mercurial_types::sharded_augmented_manifest::HgAugmentedFileLeafNode;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::ThriftConvert;
use mononoke_types::acl_manifest::AclManifest;
use mononoke_types::acl_manifest::AclManifestRestriction;
use mononoke_types::path::MPath;
use mononoke_types::typed_hash::AclManifestId;

use crate::augmented_manifest_v2::RootHgAugmentedManifestV2Id;
use crate::derive_hg_augmented_manifest::build_augmented_subtree_replacements;
use crate::derive_hg_augmented_manifest::derive_augmented_manifest_entry_from_bonsai;
use crate::derive_hg_augmented_manifest::derive_from_hg_manifest_and_parents_staged;
use crate::derive_hg_augmented_manifest::finalize_augmented_manifest_root;
use crate::mapping::MappedHgChangesetId;
use crate::mapping::RootHgAugmentedManifestId;
use crate::mapping::format_key;

#[derive(Clone, Copy)]
enum AugmentedManifestPipelineKind {
    V1,
    V2,
}

impl AugmentedManifestPipelineKind {
    fn stage_blobstore_prefix(self) -> &'static str {
        match self {
            Self::V1 => "derived_hgaugmentedmanifest_stage",
            Self::V2 => "derived_hgaugmentedmanifest_v2_stage",
        }
    }

    fn derivation_name(self) -> &'static str {
        match self {
            Self::V1 => "hg_augmented_manifests",
            Self::V2 => "hg_augmented_manifests_v2",
        }
    }
}

fn stage_blobstore_key(
    pipeline_kind: AugmentedManifestPipelineKind,
    stage_path: &MPath,
    key_prefix: &str,
    cs_id: ChangesetId,
) -> String {
    format!(
        "{}.{}.{}{}",
        pipeline_kind.stage_blobstore_prefix(),
        stage_path.get_path_hash().to_hex(),
        key_prefix,
        cs_id,
    )
}

fn use_normal_mapping(pipeline_kind: AugmentedManifestPipelineKind, stage_path: &MPath) -> bool {
    stage_path.is_root()
        && justknobs::eval(
            "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
            None,
            Some(pipeline_kind.derivation_name()),
        )
}

async fn store_intermediate_stage_output(
    ctx: &CoreContext,
    blobstore: &dyn KeyedBlobstore,
    pipeline_kind: AugmentedManifestPipelineKind,
    stage_path: &MPath,
    key_prefix: &str,
    cs_id: ChangesetId,
    output: Option<HgAugmentedManifestEntry>,
) -> Result<()> {
    let key = stage_blobstore_key(pipeline_kind, stage_path, key_prefix, cs_id);
    let thrift_output = match output {
        Some(HgAugmentedManifestEntry::DirectoryNode(dir)) => {
            mercurial_thrift::HgAugmentedManifestStageOutput::directory(dir.into_thrift())
        }
        Some(HgAugmentedManifestEntry::FileNode(leaf)) => {
            mercurial_thrift::HgAugmentedManifestStageOutput::file(leaf.into_thrift())
        }
        None => mercurial_thrift::HgAugmentedManifestStageOutput::empty(
            mercurial_thrift::HgAugmentedManifestStageOutputEmpty {},
        ),
    };
    let bytes = compact_protocol::serialize(&thrift_output);
    blobstore
        .put(ctx, key, BlobstoreBytes::from_bytes(bytes))
        .await
}

async fn fetch_intermediate_stage_output(
    ctx: &CoreContext,
    blobstore: &dyn KeyedBlobstore,
    pipeline_kind: AugmentedManifestPipelineKind,
    stage_path: &MPath,
    key_prefix: &str,
    cs_id: ChangesetId,
) -> Result<Option<Option<HgAugmentedManifestEntry>>> {
    let key = stage_blobstore_key(pipeline_kind, stage_path, key_prefix, cs_id);
    let Some(blob_data) = blobstore.get(ctx, &key).await? else {
        return Ok(None);
    };
    let thrift_output: mercurial_thrift::HgAugmentedManifestStageOutput =
        compact_protocol::deserialize(blob_data.into_raw_bytes())?;
    let output = match thrift_output {
        mercurial_thrift::HgAugmentedManifestStageOutput::directory(dir) => Some(
            HgAugmentedManifestEntry::DirectoryNode(HgAugmentedDirectoryNode::from_thrift(dir)?),
        ),
        mercurial_thrift::HgAugmentedManifestStageOutput::file(leaf) => Some(
            HgAugmentedManifestEntry::FileNode(HgAugmentedFileLeafNode::from_thrift(leaf)?),
        ),
        mercurial_thrift::HgAugmentedManifestStageOutput::empty(_) => None,
        mercurial_thrift::HgAugmentedManifestStageOutput::UnknownField(x) => {
            return Err(anyhow!(
                "unknown HgAugmentedManifestStageOutput variant {x} for {cs_id}"
            ));
        }
    };
    Ok(Some(output))
}

/// Recover the augmented subtree id from a parent's stage output. A parent stage
/// output is a `DirectoryNode` for trees; a `FileNode` (stage root is a file)
/// has no subtree to descend into and contributes no parent overlay.
fn parent_aug_id(entry: &HgAugmentedManifestEntry) -> Option<HgAugmentedManifestId> {
    match entry {
        HgAugmentedManifestEntry::DirectoryNode(dir) => {
            Some(HgAugmentedManifestId::new(dir.treenode))
        }
        HgAugmentedManifestEntry::FileNode(_) => None,
    }
}

/// Normalize an ACL stage output into an optional overlay id, mirroring
/// `normalize_acl_root`: `None` if the pointer JK is off, the stage has no acl
/// entry, or the entry is the canonical empty acl manifest; `Some(id)` otherwise.
fn normalize_acl_stage(
    output: Option<&Option<Entry<AclManifestId, AclManifestRestriction>>>,
    cs_id: ChangesetId,
    stage_path: &MPath,
) -> Result<Option<AclManifestId>> {
    if !justknobs::eval("scm/mononoke:add_acl_manifest_pointer", None, None) {
        return Ok(None);
    }
    // With the JK on, a missing acl output is a broken invariant (an entry absent at this path is fine).
    let acl_output = output.ok_or_else(|| {
        anyhow!("missing AclManifests stage output for {cs_id} at stage {stage_path}")
    })?;
    let id = match acl_output.as_ref() {
        Some(Entry::Tree(id)) => *id,
        Some(Entry::Leaf(_)) | None => return Ok(None),
    };
    if id == AclManifest::empty_id() {
        Ok(None)
    } else {
        Ok(Some(id))
    }
}

/// Descend the derived root augmented manifest to `stage_path`, returning the
/// entry there. For ROOT, synthesize the root `DirectoryNode` from the root
/// envelope (the transitionary bridge for canonical-only parents).
async fn extract_stage_entry(
    ctx: &CoreContext,
    derivation: &DerivationContext,
    root_id: HgAugmentedManifestId,
    stage_path: &MPath,
) -> Result<Option<HgAugmentedManifestEntry>> {
    let blobstore = derivation.blobstore();
    let envelope = root_id.load(ctx, blobstore).await?;
    if stage_path.is_root() {
        return Ok(Some(HgAugmentedManifestEntry::DirectoryNode(
            HgAugmentedDirectoryNode {
                treenode: envelope.augmented_manifest.hg_node_id,
                augmented_manifest_id: envelope.augmented_manifest_id,
                augmented_manifest_size: envelope.augmented_manifest_size,
                acl_manifest_directory_id: envelope.augmented_manifest.acl_manifest_directory_id,
            },
        )));
    }

    let mut current = envelope;
    let mut components = stage_path.into_iter().peekable();
    while let Some(elem) = components.next() {
        let entry = current
            .augmented_manifest
            .subentries
            .lookup(ctx, blobstore, elem.as_ref())
            .await?;
        match entry {
            None => return Ok(None),
            Some(entry) => {
                if components.peek().is_none() {
                    return Ok(Some(entry));
                }
                // Need to descend further: the entry must be a directory.
                match entry {
                    HgAugmentedManifestEntry::DirectoryNode(dir) => {
                        current = HgAugmentedManifestId::new(dir.treenode)
                            .load(ctx, blobstore)
                            .await?;
                    }
                    HgAugmentedManifestEntry::FileNode(_) => return Ok(None),
                }
            }
        }
    }
    Ok(None)
}

#[async_trait]
impl PipelineDerivable for RootHgAugmentedManifestId {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant =
        PipelineDerivableVariant::HgAugmentedManifests;

    const HAS_FINALIZE: bool = false;

    type StageOutput = Option<HgAugmentedManifestEntry>;

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

        // Cross-type inputs at this stage (same-stage edges guarantee they are
        // derived): the own hg manifest subtree from HgChangesets@S and the own
        // acl subtree from AclManifests@S. Parent hg manifests are NOT fetched
        // here; they flow from the parent augmented subtree's hg_node_id, exactly
        // as the traversal recovers them.
        let csids: Vec<ChangesetId> = bonsais.iter().map(|b| b.get_changeset_id()).collect();
        let hg_stage = StageId::Manifest(stage_path.clone());
        let acl_stage = StageId::Manifest(stage_path.clone());
        let (hg_outputs, acl_outputs) = futures::future::try_join(
            MappedHgChangesetId::fetch_stage_outputs(ctx, derivation, &hg_stage, csids.clone()),
            RootAclManifestId::fetch_stage_outputs(ctx, derivation, &acl_stage, csids),
        )
        .await?;

        let mut results: HashMap<ChangesetId, Self::StageOutput> = HashMap::new();

        for bonsai in &bonsais {
            let cs_id = bonsai.get_changeset_id();

            // Missing output is a broken invariant; an absent entry (None below) is the legitimate "nothing here" case.
            let hg_output = hg_outputs.get(&cs_id).ok_or_else(|| {
                anyhow!("missing HgChangesets stage output for {cs_id} at stage {stage_path}")
            })?;
            let hg_entry = hg_output.entry.clone().map(crate::pipeline::untrace_entry);

            let out = match hg_entry {
                // Nothing at this stage path.
                None => None,
                // A tree to augment, or a file when the stage root resolves to a
                // file in this commit. The staged core handles both.
                Some(hg_entry) => {
                    // Parent augmented subtrees at S, bonsai-parent order,
                    // preferring in-batch results over external parents.
                    let parent_aug: Vec<Option<HgAugmentedManifestId>> = bonsai
                        .parents()
                        .map(|parent_csid| {
                            let output = results
                                .get(&parent_csid)
                                .or_else(|| parents.get(&parent_csid))
                                .ok_or_else(|| {
                                    anyhow!("missing stage output for parent {parent_csid}")
                                })?;
                            Ok(output.as_ref().and_then(parent_aug_id))
                        })
                        .collect::<Result<Vec<_>>>()?;

                    let known_entries: HashMap<MPath, Option<HgAugmentedManifestEntry>> =
                        dependency_outputs
                            .get(&cs_id)
                            .map(|deps| {
                                deps.iter()
                                    .map(|(dep_path, dep_output)| {
                                        (dep_path.clone(), dep_output.clone())
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();

                    let acl_overlay =
                        normalize_acl_stage(acl_outputs.get(&cs_id), cs_id, stage_path)?;

                    // Content metadata for files changed under this stage path.
                    let content_ids: HashSet<_> = bonsai
                        .file_changes()
                        .filter(|(path, _)| stage_path.is_prefix_of(*path))
                        .filter_map(|(_, change)| change.simplify().map(|c| c.content_id()))
                        .collect();
                    let content_metadata =
                        prefetch_content_metadata(ctx, derivation.blobstore(), content_ids).await?;

                    derive_from_hg_manifest_and_parents_staged(
                        ctx,
                        derivation.blobstore(),
                        stage_path.clone(),
                        hg_entry,
                        parent_aug,
                        known_entries,
                        &content_metadata,
                        &derivation.restricted_paths(),
                        acl_overlay,
                    )
                    .await?
                }
            };

            results.insert(cs_id, out);
        }

        Ok(results)
    }

    async fn extract_stage_output_from_derived(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        derived: &RootHgAugmentedManifestId,
        stage: &StageId,
    ) -> Result<Self::StageOutput> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        extract_stage_entry(
            ctx,
            derivation,
            derived.hg_augmented_manifest_id(),
            stage_path,
        )
        .await
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
        let pipeline_kind = AugmentedManifestPipelineKind::V1;
        let use_normal_mapping = use_normal_mapping(pipeline_kind, stage_path);
        let key_prefix = derivation.mapping_key_prefix::<RootHgAugmentedManifestId>();

        stream::iter(outputs.into_iter().map(|(cs_id, output)| async move {
            if use_normal_mapping {
                let Some(HgAugmentedManifestEntry::DirectoryNode(dir)) = output else {
                    return Err(anyhow!(
                        "terminal stage output for {cs_id} should be a directory, got {output:?}",
                    ));
                };
                let key = format_key(derivation, cs_id);
                derivation
                    .blobstore()
                    .put(
                        ctx,
                        key,
                        RootHgAugmentedManifestId::new(HgAugmentedManifestId::new(dir.treenode))
                            .into(),
                    )
                    .await
            } else {
                store_intermediate_stage_output(
                    ctx,
                    derivation.blobstore().as_ref(),
                    pipeline_kind,
                    stage_path,
                    key_prefix,
                    cs_id,
                    output,
                )
                .await
            }
        }))
        .buffer_unordered(100)
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
        let pipeline_kind = AugmentedManifestPipelineKind::V1;
        let use_normal_mapping = use_normal_mapping(pipeline_kind, stage_path);
        let key_prefix = derivation.mapping_key_prefix::<RootHgAugmentedManifestId>();

        let results = stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            if use_normal_mapping {
                let key = format_key(derivation, cs_id);
                let Some(blob_data) = derivation.blobstore().get(ctx, &key).await? else {
                    return Ok::<_, Error>(None);
                };
                let root: RootHgAugmentedManifestId = blob_data.try_into()?;
                let entry = extract_stage_entry(
                    ctx,
                    derivation,
                    root.hg_augmented_manifest_id(),
                    stage_path,
                )
                .await?;
                Ok(Some((cs_id, entry)))
            } else {
                let Some(output) = fetch_intermediate_stage_output(
                    ctx,
                    derivation.blobstore().as_ref(),
                    pipeline_kind,
                    stage_path,
                    key_prefix,
                    cs_id,
                )
                .await?
                else {
                    return Ok(None);
                };
                Ok(Some((cs_id, output)))
            }
        }))
        .buffer_unordered(100)
        .try_filter_map(|opt| async move { Ok(opt) })
        .try_collect::<HashMap<_, _>>()
        .await?;
        Ok(results)
    }
}

#[async_trait]
impl PipelineDerivable for RootHgAugmentedManifestV2Id {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant =
        PipelineDerivableVariant::HgAugmentedManifestsV2;

    const HAS_FINALIZE: bool = false;

    type StageOutput = Option<HgAugmentedManifestEntry>;

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

        let csids = bonsais
            .iter()
            .map(BonsaiChangeset::get_changeset_id)
            .collect();

        let acl_outputs = RootAclManifestId::fetch_stage_outputs(
            ctx,
            derivation,
            &StageId::Manifest(stage_path.clone()),
            csids,
        )
        .await?;

        let mut results: HashMap<ChangesetId, Self::StageOutput> = HashMap::new();
        for bonsai in &bonsais {
            let cs_id = bonsai.get_changeset_id();

            let parent_entries = bonsai
                .parents()
                .map(|parent_csid| {
                    results
                        .get(&parent_csid)
                        .or_else(|| parents.get(&parent_csid))
                        .cloned()
                        .ok_or_else(|| anyhow!("missing stage output for parent {parent_csid}"))
                })
                .collect::<Result<Vec<_>>>()?;
            let root_parent_entries = stage_path.is_root().then(|| parent_entries.clone());

            let known_entries = dependency_outputs.get(&cs_id).cloned().unwrap_or_default();
            let acl_overlay = normalize_acl_stage(acl_outputs.get(&cs_id), cs_id, stage_path)?;

            let file_changes = bonsai
                .file_changes()
                .filter(|(path, _)| stage_path.is_prefix_of(*path))
                .map(|(path, file_change)| {
                    Ok((
                        path.clone(),
                        match file_change {
                            FileChange::Change(change) => Some(change.clone()),
                            FileChange::Deletion => None,
                            FileChange::UntrackedChange(_) | FileChange::UntrackedDeletion => {
                                anyhow::bail!("Can't derive manifest for snapshot")
                            }
                        },
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let content_ids = file_changes
                .iter()
                .filter_map(|(_, change)| change.as_ref().map(|change| change.content_id()))
                .collect::<HashSet<_>>();

            let content_metadata_fut =
                prefetch_content_metadata(ctx, derivation.blobstore(), content_ids);
            let source_aug_roots = HashMap::new();
            let subtree_replacements_fut = build_augmented_subtree_replacements(
                ctx,
                derivation.blobstore(),
                bonsai,
                &source_aug_roots,
            );
            let (content_metadata, subtree_replacements) =
                futures::future::try_join(content_metadata_fut, subtree_replacements_fut).await?;
            let mut parent_csids = bonsai.parents();
            let parent_bonsai_csids = (parent_csids.next(), parent_csids.next());

            let output = derive_augmented_manifest_entry_from_bonsai(
                ctx,
                derivation.blobstore(),
                stage_path.clone(),
                parent_entries,
                known_entries,
                file_changes,
                subtree_replacements,
                parent_bonsai_csids,
                &content_metadata,
                &derivation.restricted_paths(),
                acl_overlay,
            )
            .await?;
            let output = match root_parent_entries {
                Some(root_parent_entries) => Some(HgAugmentedManifestEntry::DirectoryNode(
                    finalize_augmented_manifest_root(
                        ctx,
                        derivation.blobstore(),
                        output,
                        root_parent_entries,
                        &derivation.restricted_paths(),
                        acl_overlay,
                    )
                    .await?,
                )),
                None => output,
            };
            results.insert(cs_id, output);
        }

        Ok(results)
    }

    async fn extract_stage_output_from_derived(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        derived: &RootHgAugmentedManifestV2Id,
        stage: &StageId,
    ) -> Result<Self::StageOutput> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        extract_stage_entry(
            ctx,
            derivation,
            derived.hg_augmented_manifest_id(),
            stage_path,
        )
        .await
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
        let pipeline_kind = AugmentedManifestPipelineKind::V2;
        let use_normal_mapping = use_normal_mapping(pipeline_kind, stage_path);
        let key_prefix = derivation.mapping_key_prefix::<RootHgAugmentedManifestV2Id>();

        stream::iter(outputs.into_iter().map(|(cs_id, output)| async move {
            if use_normal_mapping {
                let Some(HgAugmentedManifestEntry::DirectoryNode(dir)) = output else {
                    return Err(anyhow!(
                        "terminal stage output for {cs_id} should be a directory, got {output:?}",
                    ));
                };
                derivation
                    .blobstore()
                    .put(
                        ctx,
                        format_key(derivation, cs_id),
                        RootHgAugmentedManifestId::new(HgAugmentedManifestId::new(dir.treenode))
                            .into(),
                    )
                    .await
            } else {
                store_intermediate_stage_output(
                    ctx,
                    derivation.blobstore().as_ref(),
                    pipeline_kind,
                    stage_path,
                    key_prefix,
                    cs_id,
                    output,
                )
                .await
            }
        }))
        .buffer_unordered(100)
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
        let pipeline_kind = AugmentedManifestPipelineKind::V2;
        let use_normal_mapping = use_normal_mapping(pipeline_kind, stage_path);
        let key_prefix = derivation.mapping_key_prefix::<RootHgAugmentedManifestV2Id>();

        stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            if use_normal_mapping {
                let Some(blob_data) = derivation
                    .blobstore()
                    .get(ctx, &format_key(derivation, cs_id))
                    .await?
                else {
                    return Ok::<_, Error>(None);
                };
                let root: RootHgAugmentedManifestId = blob_data.try_into()?;
                let entry = extract_stage_entry(
                    ctx,
                    derivation,
                    root.hg_augmented_manifest_id(),
                    stage_path,
                )
                .await?;
                Ok(Some((cs_id, entry)))
            } else {
                let Some(output) = fetch_intermediate_stage_output(
                    ctx,
                    derivation.blobstore().as_ref(),
                    pipeline_kind,
                    stage_path,
                    key_prefix,
                    cs_id,
                )
                .await?
                else {
                    return Ok(None);
                };
                Ok(Some((cs_id, output)))
            }
        }))
        .buffer_unordered(100)
        .try_filter_map(|output| async move { Ok(output) })
        .try_collect()
        .await
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Context;
    use fbinit::FacebookInit;
    use mercurial_types::HgNodeHash;
    use mononoke_macros::mononoke;
    use mononoke_types::FileType;
    use mononoke_types::hash::Blake2;
    use mononoke_types::hash::Blake3;
    use mononoke_types::hash::Sha1 as ContentSha1;
    use mononoke_types::sha1_hash::Sha1 as NodeSha1;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreRef;

    use super::*;

    #[facet::container]
    #[derive(Clone)]
    struct TestRepo(RepoBlobstore);

    fn directory_output(byte: u8) -> Option<HgAugmentedManifestEntry> {
        Some(HgAugmentedManifestEntry::DirectoryNode(
            HgAugmentedDirectoryNode {
                treenode: HgNodeHash::new(NodeSha1::from_byte_array([byte; 20])),
                augmented_manifest_id: Blake3::from_byte_array([byte; 32]),
                augmented_manifest_size: u64::from(byte),
                acl_manifest_directory_id: None,
            },
        ))
    }

    fn file_output(byte: u8) -> Option<HgAugmentedManifestEntry> {
        Some(HgAugmentedManifestEntry::FileNode(
            HgAugmentedFileLeafNode {
                file_type: FileType::Regular,
                filenode: HgNodeHash::new(NodeSha1::from_byte_array([byte; 20])),
                content_blake3: Blake3::from_byte_array([byte; 32]),
                content_sha1: ContentSha1::from_byte_array([byte; 20]),
                total_size: u64::from(byte),
                file_header_metadata: None,
            },
        ))
    }

    #[mononoke::fbinit_test]
    async fn test_augmented_manifest_stage_storage_separates_v1_and_v2(
        fb: FacebookInit,
    ) -> Result<()> {
        // Given: V1 and V2 produce directory, file, and absent outputs for the
        // same stage paths and changesets.
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
        let stage_path = MPath::new("src")?;
        let key_prefix = "test-prefix.";
        let cases = [
            (
                ChangesetId::new(Blake2::from_byte_array([1; 32])),
                directory_output(1),
                file_output(2),
            ),
            (
                ChangesetId::new(Blake2::from_byte_array([2; 32])),
                file_output(3),
                None,
            ),
            (
                ChangesetId::new(Blake2::from_byte_array([3; 32])),
                None,
                directory_output(4),
            ),
        ];

        // When: both versions store their outputs under the same path and
        // changeset identities.
        for (cs_id, v1_output, v2_output) in &cases {
            store_intermediate_stage_output(
                &ctx,
                repo.repo_blobstore(),
                AugmentedManifestPipelineKind::V1,
                &stage_path,
                key_prefix,
                *cs_id,
                v1_output.clone(),
            )
            .await?;
            store_intermediate_stage_output(
                &ctx,
                repo.repo_blobstore(),
                AugmentedManifestPipelineKind::V2,
                &stage_path,
                key_prefix,
                *cs_id,
                v2_output.clone(),
            )
            .await?;
        }

        // Then: each version reads only its own output, including stored
        // absence, and the V1 key remains byte-for-byte compatible.
        for (cs_id, expected_v1, expected_v2) in &cases {
            let actual_v1 = fetch_intermediate_stage_output(
                &ctx,
                repo.repo_blobstore(),
                AugmentedManifestPipelineKind::V1,
                &stage_path,
                key_prefix,
                *cs_id,
            )
            .await?
            .context("V1 stage output should be stored")?;
            let actual_v2 = fetch_intermediate_stage_output(
                &ctx,
                repo.repo_blobstore(),
                AugmentedManifestPipelineKind::V2,
                &stage_path,
                key_prefix,
                *cs_id,
            )
            .await?
            .context("V2 stage output should be stored")?;
            assert_eq!(&actual_v1, expected_v1);
            assert_eq!(&actual_v2, expected_v2);
        }
        assert_eq!(
            stage_blobstore_key(
                AugmentedManifestPipelineKind::V1,
                &stage_path,
                key_prefix,
                cases[0].0,
            ),
            format!(
                "derived_hgaugmentedmanifest_stage.{}.{}{}",
                stage_path.get_path_hash().to_hex(),
                key_prefix,
                cases[0].0,
            ),
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_v1_augmented_manifest_stage_output_does_not_satisfy_v2_fetch(
        fb: FacebookInit,
    ) -> Result<()> {
        // Given: only V1 has stored an output for this stage and changeset.
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
        let stage_path = MPath::new("src")?;
        let key_prefix = "test-prefix.";
        let cs_id = ChangesetId::new(Blake2::from_byte_array([5; 32]));
        store_intermediate_stage_output(
            &ctx,
            repo.repo_blobstore(),
            AugmentedManifestPipelineKind::V1,
            &stage_path,
            key_prefix,
            cs_id,
            directory_output(5),
        )
        .await?;

        // When: V2 looks for its checkpoint at the same stage and changeset.
        let v2_output = fetch_intermediate_stage_output(
            &ctx,
            repo.repo_blobstore(),
            AugmentedManifestPipelineKind::V2,
            &stage_path,
            key_prefix,
            cs_id,
        )
        .await?;

        // Then: the V1 checkpoint does not satisfy the V2 fetch.
        assert_eq!(v2_output, None);

        Ok(())
    }
}
