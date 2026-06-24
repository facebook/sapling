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
use derived_data_manager::DerivationContext;
use derived_data_manager::DerivationStagePayload;
use derived_data_manager::PipelineDerivable;
use fbthrift::compact_protocol;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentManifestId;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::ThriftConvert;
use mononoke_types::content_manifest::ContentManifestFile;
use mononoke_types::path::MPath;
use mononoke_types::thrift::content_manifest as content_manifest_thrift;

use crate::RootContentManifestId;
use crate::derive::derive_content_manifest_entry;
use crate::derive::empty_directory;
use crate::derive::get_changes;
use crate::derive::get_content_manifest_subtree_changes;
use crate::mapping::format_key;

fn stage_blobstore_key(stage_path: &MPath, key_prefix: &str, cs_id: ChangesetId) -> String {
    format!(
        "derived_content_manifest_stage.{}.{}{}",
        stage_path.get_path_hash().to_hex(),
        key_prefix,
        cs_id,
    )
}

#[async_trait]
impl PipelineDerivable for RootContentManifestId {
    const PIPELINE_DERIVABLE_VARIANT: PipelineDerivableVariant =
        PipelineDerivableVariant::ContentManifests;

    type StageOutput = Option<Entry<ContentManifestId, ContentManifestFile>>;

    async fn derive_stage_batch(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
        payload: &DerivationStagePayload,
        parents: HashMap<ChangesetId, Self::StageOutput>,
        dependency_outputs: HashMap<ChangesetId, HashMap<MPath, Self::StageOutput>>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let DerivationStagePayload::Manifest(payload) = payload;
        let stage_path = &payload.path;

        let blobstore = derivation.blobstore();

        let mut results = HashMap::new();

        for bonsai in bonsais {
            let cs_id = bonsai.get_changeset_id();
            let all_changes = get_changes(&bonsai);

            // Build parent ContentManifest entries for this changeset.
            let parent_entries: Vec<Entry<ContentManifestId, ContentManifestFile>> = bonsai
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
                Option<Entry<ContentManifestId, ContentManifestFile>>,
            > = dependency_outputs
                .get(&cs_id)
                .map(|deps| {
                    deps.iter()
                        .map(|(dep_path, dep_output)| (dep_path.clone(), dep_output.clone()))
                        .collect()
                })
                .unwrap_or_default();

            let subtree_changes =
                get_content_manifest_subtree_changes(ctx, derivation, None, &bonsai).await?;

            let entry = derive_content_manifest_entry(
                ctx,
                derivation,
                parent_entries,
                all_changes,
                subtree_changes,
                known_entries,
                stage_path.clone(),
            )
            .await?;

            // The root stage must always materialize a manifest, matching
            // canonical derivation (which stores an empty root when all files
            // are deleted). A `None` entry at the root means the whole repo is
            // empty; canonical extraction returns the empty root tree.
            let entry = match entry {
                Some(entry) => Some(entry),
                None if stage_path.is_root() => {
                    let empty = empty_directory(ctx, blobstore).await?;
                    Some(Entry::Tree(empty))
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
        derived: &RootContentManifestId,
        stage_path: &MPath,
    ) -> Result<Self::StageOutput> {
        Ok(derived
            .content_manifest_id()
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
        stage_path: &MPath,
        outputs: HashMap<ChangesetId, Self::StageOutput>,
    ) -> Result<()> {
        let use_normal_mapping = stage_path.is_root()
            && justknobs::eval(
                "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
                None,
                Some("content_manifests"),
            );

        let key_prefix = derivation.mapping_key_prefix::<RootContentManifestId>();
        stream::iter(outputs.into_iter().map(|(cs_id, output)| async move {
            if use_normal_mapping {
                let Some(Entry::Tree(content_manifest_id)) = output else {
                    return Err(anyhow!(
                        "terminal stage output for {cs_id} should be Some(Entry::Tree), got {output:?}",
                    ));
                };
                let key = format_key(derivation, cs_id);
                derivation
                    .blobstore()
                    .put(ctx, key, RootContentManifestId(content_manifest_id).into())
                    .await
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let thrift_output = match output {
                    Some(Entry::Tree(content_manifest_id)) => {
                        content_manifest_thrift::ContentManifestStageOutput::content_manifest_id(
                            content_manifest_id.into_thrift(),
                        )
                    }
                    Some(Entry::Leaf(content_manifest_file)) => {
                        content_manifest_thrift::ContentManifestStageOutput::content_manifest_file(
                            content_manifest_file.into_thrift(),
                        )
                    }
                    None => content_manifest_thrift::ContentManifestStageOutput::empty(
                        content_manifest_thrift::ContentManifestStageOutputEmpty {
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
        stage_path: &MPath,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::StageOutput>> {
        let use_normal_mapping = stage_path.is_root()
            && justknobs::eval(
                "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping",
                None,
                Some("content_manifests"),
            );

        let key_prefix = derivation.mapping_key_prefix::<RootContentManifestId>();
        let results = stream::iter(cs_ids.into_iter().map(|cs_id| async move {
            if use_normal_mapping {
                let key = format_key(derivation, cs_id);
                let Some(blob_data) = derivation.blobstore().get(ctx, &key).await? else {
                    return Ok::<_, Error>(None);
                };
                let root: RootContentManifestId = blob_data.try_into()?;
                Ok(Some((
                    cs_id,
                    Some(Entry::Tree(root.into_content_manifest_id())),
                )))
            } else {
                let key = stage_blobstore_key(stage_path, key_prefix, cs_id);
                let maybe_bytes = derivation.blobstore().get(ctx, &key).await?;
                match maybe_bytes {
                    None => Ok::<_, Error>(None),
                    Some(blob_data) => {
                        let thrift_output: content_manifest_thrift::ContentManifestStageOutput =
                            compact_protocol::deserialize(blob_data.into_raw_bytes())?;
                        let output = match thrift_output {
                            content_manifest_thrift::ContentManifestStageOutput::content_manifest_id(id) => {
                                Some(Entry::Tree(ContentManifestId::from_thrift(id)?))
                            }
                            content_manifest_thrift::ContentManifestStageOutput::content_manifest_file(file) => {
                                Some(Entry::Leaf(ContentManifestFile::from_thrift(file)?))
                            }
                            content_manifest_thrift::ContentManifestStageOutput::empty(_) => None,
                            content_manifest_thrift::ContentManifestStageOutput::UnknownField(x) => {
                                return Err(anyhow!(
                                    "unknown ContentManifestStageOutput variant {x} for {cs_id}"
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
