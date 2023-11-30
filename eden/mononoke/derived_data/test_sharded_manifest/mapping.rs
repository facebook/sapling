/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::BlobstoreGetData;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_service_if::types as thrift;
use mononoke_types::test_sharded_manifest::TestShardedManifestDirectory;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ThriftConvert;
use test_manifest::RootTestManifestDirectory;

use crate::derive::derive_single;
use crate::derive_from_predecessor::derive_from_predecessor;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootTestShardedManifestDirectory(pub(crate) TestShardedManifestDirectory);

impl RootTestShardedManifestDirectory {
    pub fn into_inner(self) -> TestShardedManifestDirectory {
        self.0
    }
}

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_testshardedmanifest.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootTestShardedManifestDirectory>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

impl TryFrom<BlobstoreBytes> for RootTestShardedManifestDirectory {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        TestShardedManifestDirectory::from_bytes(&blob_bytes.into_bytes())
            .map(RootTestShardedManifestDirectory)
    }
}

impl TryFrom<BlobstoreGetData> for RootTestShardedManifestDirectory {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootTestShardedManifestDirectory> for BlobstoreBytes {
    fn from(root_mf_directory: RootTestShardedManifestDirectory) -> Self {
        BlobstoreBytes::from_bytes(root_mf_directory.0.into_bytes())
    }
}

#[async_trait]
impl BonsaiDerivable for RootTestShardedManifestDirectory {
    const VARIANT: DerivableType = DerivableType::TestShardedManifest;

    type Dependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self> {
        derive_single(ctx, derivation_ctx, bonsai, parents).await
    }

    async fn derive_from_predecessor(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
    ) -> Result<Self> {
        let csid = bonsai.get_changeset_id();
        let test_manifest = derivation_ctx
            .derive_dependency::<RootTestManifestDirectory>(ctx, csid)
            .await?;
        derive_from_predecessor(ctx, derivation_ctx, test_manifest).await
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
        if let thrift::DerivedData::test_sharded_manifest(
            thrift::DerivedDataTestShardedManifest::root_test_sharded_manifest_directory(dir),
        ) = data
        {
            TestShardedManifestDirectory::from_thrift(dir).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::test_sharded_manifest(
            thrift::DerivedDataTestShardedManifest::root_test_sharded_manifest_directory(
                data.0.into_thrift(),
            ),
        ))
    }
}

impl_bonsai_derived_via_manager!(RootTestShardedManifestDirectory);
