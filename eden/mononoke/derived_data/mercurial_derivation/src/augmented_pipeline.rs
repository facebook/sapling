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

use crate::augmented_derivation::fetch_augmented_roots_with_fallback;
use crate::derive_hg_augmented_manifest::derive_augmented_manifest_entry_from_bonsai;
use crate::derive_hg_augmented_manifest::finalize_augmented_manifest_root;
use crate::mapping::RootHgAugmentedManifestId;
use crate::mapping::format_key;

const STAGE_BLOBSTORE_PREFIX: &str = "derived_hgaugmentedmanifest_stage";

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "{}.{}.{}{}",
        STAGE_BLOBSTORE_PREFIX,
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
            Some(RootHgAugmentedManifestId::NAME),
        )
}

async fn fetch_source_augmented_roots(
    ctx: &CoreContext,
    derivation: &DerivationContext,
    source_csids: HashSet<ChangesetId>,
) -> Result<HashMap<ChangesetId, HgAugmentedManifestId>> {
    if source_csids.is_empty() {
        return Ok(HashMap::new());
    }

    let source_csids: Vec<_> = source_csids.into_iter().collect();
    let pipeline_outputs = RootHgAugmentedManifestId::fetch_stage_outputs(
        ctx,
        derivation,
        &StageId::Manifest(MPath::ROOT),
        source_csids.clone(),
    )
    .await?;
    let roots = pipeline_outputs
        .into_iter()
        .map(|(csid, output)| match output {
            Some(HgAugmentedManifestEntry::DirectoryNode(dir)) => {
                Ok((csid, HgAugmentedManifestId::new(dir.treenode)))
            }
            output => Err(anyhow!(
                "{} terminal stage output for copy source {csid} should be a directory, got {output:?}",
                RootHgAugmentedManifestId::NAME,
            )),
        })
        .collect::<Result<_>>()?;
    fetch_augmented_roots_with_fallback(ctx, derivation, source_csids, roots, "Copy source").await
}

async fn store_intermediate_stage_output(
    ctx: &CoreContext,
    blobstore: &dyn KeyedBlobstore,
    stage_path: &MPath,
    key_prefix: &str,
    cs_id: ChangesetId,
    output: Option<HgAugmentedManifestEntry>,
) -> Result<()> {
    let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
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
    stage_path: &MPath,
    key_prefix: &str,
    cs_id: ChangesetId,
) -> Result<Option<Option<HgAugmentedManifestEntry>>> {
    let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
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

/// Normalize an ACL stage output into an optional overlay id, mirroring
/// `normalize_acl_root`: `None` if the stage has no ACL entry or the entry is
/// the canonical empty ACL manifest; `Some(id)` otherwise.
fn normalize_acl_stage(
    output: Option<&Option<Entry<AclManifestId, AclManifestRestriction>>>,
    cs_id: ChangesetId,
    stage_path: &MPath,
) -> Result<Option<AclManifestId>> {
    // A missing ACL output is a broken invariant; an entry absent at this path is fine.
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

/// Derives augmented manifest stage entries from Bonsai changes, parent stage
/// entries, child stage entries, and the same-stage ACL output.
async fn derive_stage_batch_from_bonsai(
    ctx: &CoreContext,
    derivation: &DerivationContext,
    bonsais: Vec<BonsaiChangeset>,
    payload: &DerivationStagePayload,
    parents: HashMap<ChangesetId, Option<HgAugmentedManifestEntry>>,
    dependency_outputs: HashMap<ChangesetId, HashMap<MPath, Option<HgAugmentedManifestEntry>>>,
) -> Result<HashMap<ChangesetId, Option<HgAugmentedManifestEntry>>> {
    let DerivationStagePayload::Manifest(payload) = payload else {
        anyhow::bail!("{} has no finalize derive", RootHgAugmentedManifestId::NAME);
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

    let mut results: HashMap<ChangesetId, Option<HgAugmentedManifestEntry>> = HashMap::new();
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
            .filter(|(path, change)| {
                stage_path.is_prefix_of(*path)
                    || (change.is_changed() && path.is_prefix_of(stage_path))
            })
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
        let mut parent_csids = bonsai.parents();
        let parent_bonsai_csids = (parent_csids.next(), parent_csids.next());
        let is_hg_parent = |copy_csid: &ChangesetId| {
            parent_bonsai_csids.0.as_ref() == Some(copy_csid)
                || parent_bonsai_csids.1.as_ref() == Some(copy_csid)
        };
        let source_csids: HashSet<_> = file_changes
            .iter()
            .filter_map(|(_, change)| change.as_ref()?.copy_from())
            .filter(|(copy_path, copy_csid)| {
                !stage_path.is_prefix_of(copy_path) && is_hg_parent(copy_csid)
            })
            .map(|(_, copy_csid)| *copy_csid)
            .collect();
        let source_aug_roots = fetch_source_augmented_roots(ctx, derivation, source_csids).await?;
        let content_metadata = content_metadata_fut.await?;

        let output = derive_augmented_manifest_entry_from_bonsai(
            ctx,
            derivation.blobstore(),
            stage_path.clone(),
            parent_entries,
            known_entries,
            file_changes,
            parent_bonsai_csids,
            &source_aug_roots,
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
        derive_stage_batch_from_bonsai(
            ctx,
            derivation,
            bonsais,
            payload,
            parents,
            dependency_outputs,
        )
        .await
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
                store_intermediate_stage_output(
                    ctx,
                    derivation.blobstore().as_ref(),
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
                let Some(output) = fetch_intermediate_stage_output(
                    ctx,
                    derivation.blobstore().as_ref(),
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

#[cfg(test)]
mod tests {
    use anyhow::Context;
    use fbinit::FacebookInit;
    use futures::FutureExt;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::with_just_knobs_async;
    use mercurial_types::HgNodeHash;
    use mononoke_macros::mononoke;
    use mononoke_types::FileType;
    use mononoke_types::hash::Blake2;
    use mononoke_types::hash::Blake3;
    use mononoke_types::hash::Sha1 as ContentSha1;
    use mononoke_types::sha1_hash::Sha1 as NodeSha1;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreRef;
    use repo_derived_data::RepoDerivedData;
    use repo_derived_data::RepoDerivedDataRef;

    use super::*;

    #[facet::container]
    #[derive(Clone)]
    struct TestRepo(RepoBlobstore, RepoDerivedData);

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
    async fn test_augmented_manifest_stage_storage_round_trips(fb: FacebookInit) -> Result<()> {
        // Given: directory, file, and absent outputs for one stage path.
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
        let stage_path = MPath::new("src")?;
        let key_prefix = "test-prefix.";
        let cases = [
            (
                ChangesetId::new(Blake2::from_byte_array([1; 32])),
                directory_output(1),
            ),
            (
                ChangesetId::new(Blake2::from_byte_array([2; 32])),
                file_output(2),
            ),
            (ChangesetId::new(Blake2::from_byte_array([3; 32])), None),
        ];

        // When: each output is stored as an intermediate stage output.
        for (cs_id, output) in &cases {
            store_intermediate_stage_output(
                &ctx,
                repo.repo_blobstore(),
                &stage_path,
                key_prefix,
                *cs_id,
                output.clone(),
            )
            .await?;
        }

        // Then: each output reads back, including stored absence, under the
        // existing stage key.
        for (cs_id, expected) in &cases {
            let actual = fetch_intermediate_stage_output(
                &ctx,
                repo.repo_blobstore(),
                &stage_path,
                key_prefix,
                *cs_id,
            )
            .await?
            .context("stage output should be stored")?;
            assert_eq!(&actual, expected);
        }
        assert_eq!(
            stage_blobstore_key(&stage_path, key_prefix, cases[0].0),
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
    async fn test_terminal_publication_uses_only_shared_mapping(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;

        // Given: a terminal output and the terminal switch on.
        let cs_id = ChangesetId::new(Blake2::from_byte_array([5; 32]));
        let expected =
            HgAugmentedManifestId::new(HgNodeHash::new(NodeSha1::from_byte_array([5; 20])));

        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping".to_string(),
                KnobVal::Bool(true),
            )])),
            async move {
                let derivation = repo.repo_derived_data().manager().derivation_context(None);
                let key_prefix = derivation.mapping_key_prefix::<RootHgAugmentedManifestId>();

                // When: the pipeline stores the terminal root.
                RootHgAugmentedManifestId::store_stage_outputs(
                    &ctx,
                    &derivation,
                    &StageId::Manifest(MPath::ROOT),
                    HashMap::from([(cs_id, directory_output(5))]),
                )
                .await?;

                // Then: only the shared root mapping is recorded.
                assert_eq!(
                    fetch_intermediate_stage_output(
                        &ctx,
                        derivation.blobstore().as_ref(),
                        &MPath::ROOT,
                        key_prefix,
                        cs_id,
                    )
                    .await?,
                    None,
                );
                assert_eq!(
                    RootHgAugmentedManifestId::fetch(&ctx, &derivation, cs_id)
                        .await?
                        .map(|root| root.hg_augmented_manifest_id()),
                    Some(expected),
                );

                Ok(())
            }
            .boxed(),
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn test_terminal_switch_off_uses_only_checkpoint(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
        let checkpoint_cs_id = ChangesetId::new(Blake2::from_byte_array([6; 32]));
        let shared_cs_id = ChangesetId::new(Blake2::from_byte_array([7; 32]));
        let expected = directory_output(6);

        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([(
                "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping".to_string(),
                KnobVal::Bool(false),
            )])),
            async move {
                let derivation = repo.repo_derived_data().manager().derivation_context(None);

                // Given: one terminal checkpoint and one changeset with only a shared root.
                RootHgAugmentedManifestId::store_stage_outputs(
                    &ctx,
                    &derivation,
                    &StageId::Manifest(MPath::ROOT),
                    HashMap::from([(checkpoint_cs_id, expected.clone())]),
                )
                .await?;
                RootHgAugmentedManifestId::new(HgAugmentedManifestId::new(HgNodeHash::new(
                    NodeSha1::from_byte_array([7; 20]),
                )))
                .store_mapping(&ctx, &derivation, shared_cs_id)
                .await?;

                // When: the pipeline checks terminal completion with the switch off.
                let outputs = RootHgAugmentedManifestId::fetch_stage_outputs(
                    &ctx,
                    &derivation,
                    &StageId::Manifest(MPath::ROOT),
                    vec![checkpoint_cs_id, shared_cs_id],
                )
                .await?;

                // Then: only the checkpoint is completion and no root mapping was written.
                assert_eq!(outputs, HashMap::from([(checkpoint_cs_id, expected)]));
                assert_eq!(
                    RootHgAugmentedManifestId::fetch(&ctx, &derivation, checkpoint_cs_id).await?,
                    None,
                );

                Ok(())
            }
            .boxed(),
        )
        .await
    }
}
