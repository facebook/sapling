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
use mononoke_types::test_manifest::TestManifestDirectory;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ThriftConvert;

use crate::derive::derive_single;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootTestManifestDirectory(pub(crate) TestManifestDirectory);

impl RootTestManifestDirectory {
    pub fn into_inner(self) -> TestManifestDirectory {
        self.0
    }
}

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_testmanifest.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootTestManifestDirectory>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

impl TryFrom<BlobstoreBytes> for RootTestManifestDirectory {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        TestManifestDirectory::from_bytes(&blob_bytes.into_bytes()).map(RootTestManifestDirectory)
    }
}

impl TryFrom<BlobstoreGetData> for RootTestManifestDirectory {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootTestManifestDirectory> for BlobstoreBytes {
    fn from(root_mf_directory: RootTestManifestDirectory) -> Self {
        BlobstoreBytes::from_bytes(root_mf_directory.0.into_bytes())
    }
}

#[async_trait]
impl BonsaiDerivable for RootTestManifestDirectory {
    const VARIANT: DerivableType = DerivableType::TestManifest;

    type Dependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self> {
        derive_single(ctx, derivation_ctx, bonsai, parents).await
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
        if let thrift::DerivedData::test_manifest(
            thrift::DerivedDataTestManifest::root_test_manifest_directory(dir),
        ) = data
        {
            TestManifestDirectory::from_thrift(dir).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::test_manifest(
            thrift::DerivedDataTestManifest::root_test_manifest_directory(data.0.into_thrift()),
        ))
    }
}

impl_bonsai_derived_via_manager!(RootTestManifestDirectory);
