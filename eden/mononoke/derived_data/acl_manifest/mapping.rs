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
use basename_suffix_skeleton_manifest_v3::RootBssmV3DirectoryId;
use blobstore::BlobstoreGetData;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivableUntopologically;
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use derived_data_service_if as thrift;
use fsnodes::RootFsnodeId;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableUntopologicallyVariant;
use mononoke_types::ThriftConvert;
use mononoke_types::typed_hash::AclManifestId;

use crate::derive::derive_from_scratch;
use crate::derive::derive_single;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootAclManifestId(pub(crate) AclManifestId);

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_aclmf.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootAclManifestId>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

impl TryFrom<BlobstoreBytes> for RootAclManifestId {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        AclManifestId::from_bytes(blob_bytes.into_bytes()).map(RootAclManifestId)
    }
}

impl TryFrom<BlobstoreGetData> for RootAclManifestId {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootAclManifestId> for BlobstoreBytes {
    fn from(root_mf_id: RootAclManifestId) -> Self {
        BlobstoreBytes::from_bytes(root_mf_id.0.into_bytes())
    }
}

impl RootAclManifestId {
    pub fn into_inner_id(self) -> AclManifestId {
        self.0
    }
    pub fn inner_id(&self) -> &AclManifestId {
        &self.0
    }
}

#[async_trait]
impl BonsaiDerivable for RootAclManifestId {
    const VARIANT: DerivableType = DerivableType::AclManifests;

    type Dependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self> {
        derive_single(ctx, derivation_ctx, bonsai, parents, known).await
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx.blobstore().put(ctx, key, self.into()).await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx
            .blobstore()
            .get(ctx, &key)
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::acl_manifest(
            thrift::DerivedDataAclManifest::root_acl_manifest_id(id),
        ) = data
        {
            AclManifestId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME,
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::acl_manifest(
            thrift::DerivedDataAclManifest::root_acl_manifest_id(data.0.into_thrift()),
        ))
    }
}

#[async_trait]
impl DerivableUntopologically for RootAclManifestId {
    const DERIVABLE_UNTOPOLOGICALLY_VARIANT: DerivableUntopologicallyVariant =
        DerivableUntopologicallyVariant::AclManifests;

    /// From scratch derivation depends on BSSMV3 to efficiently find all
    /// the ACL files and on fsnodes to get the file content.
    type PredecessorDependencies = dependencies![RootBssmV3DirectoryId, RootFsnodeId];

    async fn unsafe_derive_untopologically(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
    ) -> Result<Self> {
        derive_from_scratch(ctx, derivation_ctx, bonsai).await
    }
}
