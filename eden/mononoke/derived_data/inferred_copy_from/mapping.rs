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
use blobstore::BlobstoreGetData;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use derived_data_service_if as thrift;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::InferredCopyFromId;
use mononoke_types::ThriftConvert;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootInferredCopyFromId(pub(crate) InferredCopyFromId);

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_icf.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootInferredCopyFromId>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

impl TryFrom<BlobstoreBytes> for RootInferredCopyFromId {
    type Error = Error;
    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        InferredCopyFromId::from_bytes(blob_bytes.into_bytes()).map(RootInferredCopyFromId)
    }
}

impl TryFrom<BlobstoreGetData> for RootInferredCopyFromId {
    type Error = Error;
    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootInferredCopyFromId> for BlobstoreBytes {
    fn from(root_id: RootInferredCopyFromId) -> Self {
        BlobstoreBytes::from_bytes(root_id.0.into_bytes())
    }
}

impl RootInferredCopyFromId {
    pub fn into_inner_id(self) -> InferredCopyFromId {
        self.0
    }
    pub fn inner_id(&self) -> &InferredCopyFromId {
        &self.0
    }
}

#[async_trait]
impl BonsaiDerivable for RootInferredCopyFromId {
    const VARIANT: DerivableType = DerivableType::InferredCopyFrom;

    type Dependencies = dependencies![];
    type PredecessorDependencies = dependencies![];

    async fn derive_single(
        _ctx: &CoreContext,
        _derivation_ctx: &DerivationContext,
        _bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
        _known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self> {
        unimplemented!("InferredCopyFrom derivation is not implemented")
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
        if let thrift::DerivedData::inferred_copy_from(
            thrift::DerivedDataInferredCopyFrom::root_inferred_copy_from_id(id),
        ) = data
        {
            InferredCopyFromId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::inferred_copy_from(
            thrift::DerivedDataInferredCopyFrom::root_inferred_copy_from_id(data.0.into_thrift()),
        ))
    }
}
