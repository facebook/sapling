/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use derived_data_manager::ManifestDerivable;
use derived_data_manager::ManifestDerivation;
use fbthrift::compact_protocol;
use manifest::Entry;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::PipelineDerivableVariant;
use mononoke_types::ThriftConvert;
use mononoke_types::acl_manifest::AclManifestRestriction;
use mononoke_types::thrift::acl_manifest as acl_manifest_thrift;
use mononoke_types::typed_hash::AclManifestId;

use crate::RootAclManifestId;
use crate::derive::derive_acl_manifest_entry;
use crate::derive::empty_root_acl_manifest_id;
use crate::derive::get_acl_file_changes;
use crate::derive::prepare_acl_file_changes;

#[async_trait]
impl ManifestDerivable for RootAclManifestId {
    type TreeId = AclManifestId;
    type Leaf = AclManifestRestriction;

    const PIPELINE_VARIANT: PipelineDerivableVariant = PipelineDerivableVariant::AclManifests;
    const STAGE_KEY_PREFIX: &'static str = "derived_acl_manifest_stage";

    async fn load_root_manifest(
        &self,
        _ctx: &CoreContext,
        _derivation: &DerivationContext,
    ) -> Result<AclManifestId> {
        Ok(self.0)
    }

    async fn from_root_manifest(
        _ctx: &CoreContext,
        _derivation: &DerivationContext,
        root: AclManifestId,
    ) -> Result<Self> {
        Ok(Self(root))
    }

    async fn derive_manifest(
        ctx: &CoreContext,
        derivation: &DerivationContext,
        bonsai: BonsaiChangeset,
        input: ManifestDerivation<AclManifestId, AclManifestRestriction>,
    ) -> Result<Option<Entry<AclManifestId, AclManifestRestriction>>> {
        let blobstore = derivation.blobstore();
        let acl_file_name = derivation
            .restricted_paths()
            .config()
            .acl_file_name()
            .to_string();
        let parent_ids = input
            .parents
            .iter()
            .filter_map(|entry| entry.clone().into_tree())
            .collect::<Vec<_>>();
        let changes = get_acl_file_changes(
            ctx,
            blobstore,
            &bonsai,
            &parent_ids,
            &acl_file_name,
            &input.path,
        )
        .await?;
        let changes = prepare_acl_file_changes(ctx, blobstore, changes, &acl_file_name).await?;
        let is_root = input.path.is_root();
        let entry =
            derive_acl_manifest_entry(ctx, blobstore, input, changes, &acl_file_name).await?;
        match entry {
            None if is_root => Ok(Some(Entry::Tree(
                empty_root_acl_manifest_id(ctx, blobstore)
                    .await?
                    .into_inner_id(),
            ))),
            entry => Ok(entry),
        }
    }

    fn encode_entry(entry: Option<Entry<AclManifestId, AclManifestRestriction>>) -> BlobstoreBytes {
        let entry = match entry {
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
        BlobstoreBytes::from_bytes(compact_protocol::serialize(&entry))
    }

    fn decode_entry(
        bytes: BlobstoreBytes,
    ) -> Result<Option<Entry<AclManifestId, AclManifestRestriction>>> {
        let thrift_output: acl_manifest_thrift::AclManifestStageOutput =
            compact_protocol::deserialize(bytes.into_bytes())?;
        Ok(match thrift_output {
            acl_manifest_thrift::AclManifestStageOutput::acl_manifest_id(id) => {
                Some(Entry::Tree(AclManifestId::from_thrift(id)?))
            }
            acl_manifest_thrift::AclManifestStageOutput::acl_file(acl_file) => {
                Some(Entry::Leaf(AclManifestRestriction::from_thrift(acl_file)?))
            }
            acl_manifest_thrift::AclManifestStageOutput::empty(_) => None,
            acl_manifest_thrift::AclManifestStageOutput::UnknownField(x) => {
                return Err(anyhow!("unknown AclManifestStageOutput variant {x}"));
            }
        })
    }
}
