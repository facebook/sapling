/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::time::Duration;

use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use bytes::Bytes;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use test_repo_factory::TestRepoFactory;

use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;

use derived_data_service_if::types as thrift;

#[derive(Clone, Debug)]
pub struct DerivedGeneration {
    pub generation: u64,
}

impl From<DerivedGeneration> for BlobstoreBytes {
    fn from(derived: DerivedGeneration) -> BlobstoreBytes {
        let generation = derived.generation.to_string();
        let data = Bytes::copy_from_slice(generation.as_bytes());
        BlobstoreBytes::from_bytes(data)
    }
}

impl TryFrom<BlobstoreBytes> for DerivedGeneration {
    type Error = Error;

    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        let generation = std::str::from_utf8(blob_bytes.as_bytes())?.parse::<u64>()?;
        Ok(DerivedGeneration { generation })
    }
}

impl TryFrom<BlobstoreGetData> for DerivedGeneration {
    type Error = Error;

    fn try_from(data: BlobstoreGetData) -> Result<Self> {
        data.into_bytes().try_into()
    }
}

#[async_trait]
impl BonsaiDerivable for DerivedGeneration {
    const NAME: &'static str = "test_generation";

    type Dependencies = dependencies![];

    async fn derive_single(
        _ctx: &CoreContext,
        _derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self> {
        if let Some(delay_str) = bonsai
            .extra()
            .collect::<HashMap<_, _>>()
            .get("test-derive-delay")
        {
            let delay = std::str::from_utf8(delay_str)?.parse::<f64>()?;
            tokio::time::sleep(Duration::from_secs_f64(delay)).await;
        }
        let mut generation = 1;
        for parent in parents {
            if parent.generation >= generation {
                generation = parent.generation + 1;
            }
        }
        let derived = DerivedGeneration { generation };
        Ok(derived)
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        derivation_ctx
            .blobstore()
            .put(
                ctx,
                format!(
                    "repo{}.test_generation.{}",
                    derivation_ctx.repo_id(),
                    changeset_id,
                ),
                self.into(),
            )
            .await?;
        Ok(())
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        match derivation_ctx
            .blobstore()
            .get(
                ctx,
                &format!(
                    "repo{}.test_generation.{}",
                    derivation_ctx.repo_id(),
                    changeset_id
                ),
            )
            .await?
        {
            Some(blob) => Ok(Some(blob.try_into()?)),
            None => Ok(None),
        }
    }

    fn from_thrift(_: thrift::DerivedData) -> Result<Self> {
        bail!("Not implemented for {}", Self::NAME);
    }

    fn into_thrift(_: Self) -> Result<thrift::DerivedData> {
        bail!("Not implemented for {}", Self::NAME);
    }
}

pub fn make_test_repo_factory(fb: FacebookInit) -> TestRepoFactory {
    let mut factory = TestRepoFactory::new(fb).unwrap();
    factory.with_config_override(|repo_config| {
        repo_config
            .derived_data_config
            .get_active_config()
            .expect("No enabled derived data types config")
            .types
            .insert(DerivedGeneration::NAME.to_string());
    });
    factory
}
