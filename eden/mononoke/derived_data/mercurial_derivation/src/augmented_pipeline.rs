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
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::ThriftConvert;
use mononoke_types::acl_manifest::AclManifest;
use mononoke_types::acl_manifest::AclManifestRestriction;
use mononoke_types::path::MPath;
use mononoke_types::typed_hash::AclManifestId;

use crate::derive_hg_augmented_manifest::derive_from_hg_manifest_and_parents_staged;
use crate::mapping::MappedHgChangesetId;
use crate::mapping::RootHgAugmentedManifestId;
use crate::mapping::format_key;

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "derived_hgaugmentedmanifest_stage.{}.{}{}",
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
            Some("hg_augmented_manifests"),
        )
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
) -> Result<Option<AclManifestId>> {
    if !justknobs::eval("scm/mononoke:add_acl_manifest_pointer", None, None) {
        return Ok(None);
    }
    let id = match output.and_then(|o| o.as_ref()) {
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

            let hg_entry = hg_outputs
                .get(&cs_id)
                .and_then(|o| o.entry.clone())
                .map(crate::pipeline::untrace_entry);

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

                    let acl_overlay = normalize_acl_stage(acl_outputs.get(&cs_id))?;

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
        let use_normal_mapping = use_normal_mapping(stage_path);
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
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let thrift_output = match output {
                    Some(HgAugmentedManifestEntry::DirectoryNode(dir)) => {
                        mercurial_thrift::HgAugmentedManifestStageOutput::directory(
                            dir.into_thrift(),
                        )
                    }
                    Some(HgAugmentedManifestEntry::FileNode(leaf)) => {
                        mercurial_thrift::HgAugmentedManifestStageOutput::file(leaf.into_thrift())
                    }
                    None => mercurial_thrift::HgAugmentedManifestStageOutput::empty(
                        mercurial_thrift::HgAugmentedManifestStageOutputEmpty {},
                    ),
                };
                let bytes = compact_protocol::serialize(&thrift_output);
                derivation
                    .blobstore()
                    .put(ctx, key, BlobstoreBytes::from_bytes(bytes))
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
        let use_normal_mapping = use_normal_mapping(stage_path);
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
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let Some(blob_data) = derivation.blobstore().get(ctx, &key).await? else {
                    return Ok::<_, Error>(None);
                };
                let thrift_output: mercurial_thrift::HgAugmentedManifestStageOutput =
                    compact_protocol::deserialize(blob_data.into_raw_bytes())?;
                let output = match thrift_output {
                    mercurial_thrift::HgAugmentedManifestStageOutput::directory(dir) => {
                        Some(HgAugmentedManifestEntry::DirectoryNode(
                            HgAugmentedDirectoryNode::from_thrift(dir)?,
                        ))
                    }
                    mercurial_thrift::HgAugmentedManifestStageOutput::file(leaf) => {
                        Some(HgAugmentedManifestEntry::FileNode(
                            HgAugmentedFileLeafNode::from_thrift(leaf)?,
                        ))
                    }
                    mercurial_thrift::HgAugmentedManifestStageOutput::empty(_) => None,
                    mercurial_thrift::HgAugmentedManifestStageOutput::UnknownField(x) => {
                        return Err(anyhow!(
                            "unknown HgAugmentedManifestStageOutput variant {x} for {cs_id}"
                        ));
                    }
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
