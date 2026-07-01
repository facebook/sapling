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
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::ThriftConvert;
use mononoke_types::acl_manifest::AclManifestRestriction;
use mononoke_types::path::MPath;
use mononoke_types::thrift::acl_manifest as acl_manifest_thrift;
use mononoke_types::typed_hash::AclManifestId;

use crate::RootAclManifestId;
use crate::derive::derive_acl_manifest_entry;
use crate::derive::empty_root_acl_manifest_id;
use crate::derive::get_acl_file_changes;
use crate::derive::get_acl_manifest_subtree_changes;
use crate::derive::prepare_acl_file_changes;
use crate::mapping::format_key;

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "derived_acl_manifest_stage.{}.{}{}",
        stage_path.get_path_hash().to_hex(),
        key_prefix,
        cs_id,
    )
}

#[async_trait]
impl PipelineDerivable for RootAclManifestId {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant =
        PipelineDerivableVariant::AclManifests;

    type StageOutput = Option<Entry<AclManifestId, AclManifestRestriction>>;

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
        let acl_file_name = derivation
            .restricted_paths()
            .config()
            .acl_file_name()
            .to_string();

        let mut results = HashMap::new();

        for bonsai in bonsais {
            let cs_id = bonsai.get_changeset_id();

            // Build parent AclManifest entries for this changeset.
            let parent_entries: Vec<Entry<AclManifestId, AclManifestRestriction>> = bonsai
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
            let known_entries: HashMap<
                MPath,
                Option<Entry<AclManifestId, AclManifestRestriction>>,
            > = dependency_outputs
                .get(&cs_id)
                .map(|deps| {
                    deps.iter()
                        .map(|(dep_path, dep_output)| (dep_path.clone(), dep_output.clone()))
                        .collect()
                })
                .unwrap_or_default();

            let parent_ids: Vec<AclManifestId> = parent_entries
                .iter()
                .filter_map(|entry| entry.clone().into_tree())
                .collect();

            // Collect .slacl file changes (explicit + implicit deletes) and
            // resolve their content.
            let changes = get_acl_file_changes(
                ctx,
                blobstore,
                &bonsai,
                &parent_ids,
                &acl_file_name,
                stage_path,
            )
            .await?;
            let derive_changes =
                prepare_acl_file_changes(ctx, blobstore, changes, &acl_file_name).await?;

            let subtree_changes =
                get_acl_manifest_subtree_changes(ctx, derivation, &bonsai, None).await?;

            let entry = derive_acl_manifest_entry(
                ctx,
                blobstore,
                parent_entries,
                derive_changes,
                subtree_changes,
                known_entries,
                stage_path.clone(),
                &acl_file_name,
            )
            .await?;

            // The root stage must always materialize a manifest, matching
            // canonical derivation (which stores an empty root when no `.slacl`
            // files exist). A `None` entry at the root means the whole repo has
            // no restrictions; canonical extraction returns the empty root tree.
            let entry = match entry {
                Some(entry) => Some(entry),
                None if stage_path.is_root() => {
                    let empty = empty_root_acl_manifest_id(ctx, blobstore).await?;
                    Some(Entry::Tree(empty.into_inner_id()))
                }
                None => None,
            };

            results.insert(cs_id, entry);
        }

        Ok(results)
    }

    async fn extract_stage_output_from_derived(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        derived: &RootAclManifestId,
        stage: &StageId,
    ) -> Result<Self::StageOutput> {
        let StageId::Manifest(stage_path) = stage else {
            anyhow::bail!("{} has no finalize stage", Self::NAME);
        };
        Ok(derived
            .inner_id()
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
                Some("acl_manifests"),
            );

        let key_prefix = derivation.mapping_key_prefix::<RootAclManifestId>();
        stream::iter(outputs.into_iter().map(|(cs_id, output)| async move {
            if use_normal_mapping {
                let Some(Entry::Tree(acl_manifest_id)) = output else {
                    return Err(anyhow!(
                        "terminal stage output for {cs_id} should be Some(Entry::Tree), got {output:?}",
                    ));
                };
                let key = format_key(derivation, cs_id);
                derivation
                    .blobstore()
                    .put(ctx, key, RootAclManifestId(acl_manifest_id).into())
                    .await
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let thrift_output = match output {
                    Some(Entry::Tree(acl_manifest_id)) => {
                        acl_manifest_thrift::AclManifestStageOutput::acl_manifest_id(
                            acl_manifest_id.into_thrift(),
                        )
                    }
                    Some(Entry::Leaf(acl_file)) => {
                        acl_manifest_thrift::AclManifestStageOutput::acl_file(acl_file.into_thrift())
                    }
                    None => acl_manifest_thrift::AclManifestStageOutput::empty(
                        acl_manifest_thrift::AclManifestStageOutputEmpty {
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
                Some("acl_manifests"),
            );

        let key_prefix = derivation.mapping_key_prefix::<RootAclManifestId>();
        let results = stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            if use_normal_mapping {
                let key = format_key(derivation, cs_id);
                let Some(blob_data) = derivation.blobstore().get(ctx, &key).await? else {
                    return Ok::<_, Error>(None);
                };
                let root: RootAclManifestId = blob_data.try_into()?;
                Ok(Some((cs_id, Some(Entry::Tree(root.into_inner_id())))))
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let maybe_bytes = derivation.blobstore().get(ctx, &key).await?;
                match maybe_bytes {
                    None => Ok::<_, Error>(None),
                    Some(blob_data) => {
                        let thrift_output: acl_manifest_thrift::AclManifestStageOutput =
                            compact_protocol::deserialize(blob_data.into_raw_bytes())?;
                        let output = match thrift_output {
                            acl_manifest_thrift::AclManifestStageOutput::acl_manifest_id(id) => {
                                Some(Entry::Tree(AclManifestId::from_thrift(id)?))
                            }
                            acl_manifest_thrift::AclManifestStageOutput::acl_file(acl_file) => {
                                Some(Entry::Leaf(AclManifestRestriction::from_thrift(acl_file)?))
                            }
                            acl_manifest_thrift::AclManifestStageOutput::empty(_) => None,
                            acl_manifest_thrift::AclManifestStageOutput::UnknownField(x) => {
                                return Err(anyhow!(
                                    "unknown AclManifestStageOutput variant {x} for {cs_id}"
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
