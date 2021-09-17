/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error, Result};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::{Blobstore, BlobstoreBytes, BlobstoreGetData};
use context::CoreContext;
use derived_data::{
    impl_bonsai_derived_mapping, BlobstoreExistsWithDataMapping, BonsaiDerivable, BonsaiDerived,
    BonsaiDerivedMappingContainer, DerivedDataTypesConfig,
};
use futures::stream::{self, StreamExt, TryStreamExt};
use metaconfig_types::BlameVersion;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::Arc;
use unodes::RootUnodeManifestId;

use crate::batch_v2::derive_blame_v2_in_batch;
use crate::derive_v2::derive_blame_v2;
use crate::{BlameDeriveOptions, DEFAULT_BLAME_FILESIZE_LIMIT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootBlameV2 {
    csid: ChangesetId,
    root_manifest: RootUnodeManifestId,
}

impl RootBlameV2 {
    pub fn root_manifest(&self) -> RootUnodeManifestId {
        self.root_manifest
    }
}

#[async_trait]
impl BonsaiDerivable for RootBlameV2 {
    const NAME: &'static str = "blame";

    type Options = BlameDeriveOptions;

    async fn derive_from_parents_impl(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
        options: &Self::Options,
    ) -> Result<Self, Error> {
        let csid = bonsai.get_changeset_id();
        // NOTE: This uses the default unode version for the repo, whatever
        // that may be.
        let root_manifest = RootUnodeManifestId::derive(&ctx, &repo, csid).await?;
        if options.blame_version != BlameVersion::V2 {
            return Err(anyhow!(
                "programming error: incorrect blame version (expected V2)"
            ));
        }
        derive_blame_v2(&ctx, &repo, bonsai, root_manifest, options).await?;
        Ok(RootBlameV2 {
            csid,
            root_manifest,
        })
    }

    async fn batch_derive_impl(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csids: Vec<ChangesetId>,
        mapping: &BonsaiDerivedMappingContainer<Self>,
        _gap_size: Option<usize>,
    ) -> Result<HashMap<ChangesetId, Self>, Error> {
        let derived = derive_blame_v2_in_batch(ctx, repo, mapping, csids.clone()).await?;

        stream::iter(derived.into_iter().map(|(csid, root_manifest)| async move {
            let derived = RootBlameV2 {
                csid,
                root_manifest,
            };
            mapping.put(ctx, csid, &derived).await?;
            Ok((csid, derived))
        }))
        .buffered(100)
        .try_collect::<HashMap<_, _>>()
        .await
    }
}

#[derive(Clone)]
pub struct RootBlameV2Mapping {
    blobstore: Arc<dyn Blobstore>,
    options: BlameDeriveOptions,
}

#[async_trait]
impl BlobstoreExistsWithDataMapping for RootBlameV2Mapping {
    type Value = RootBlameV2;

    fn new(blobstore: Arc<dyn Blobstore>, config: &DerivedDataTypesConfig) -> Result<Self> {
        let filesize_limit = config
            .blame_filesize_limit
            .unwrap_or(DEFAULT_BLAME_FILESIZE_LIMIT);
        let blame_version = config.blame_version;
        if blame_version != BlameVersion::V2 {
            return Err(anyhow!(
                "programming error: incorrect blame mapping version (expected V2)"
            ));
        }
        let options = BlameDeriveOptions {
            filesize_limit,
            blame_version,
        };
        Ok(Self { blobstore, options })
    }

    fn blobstore(&self) -> &dyn Blobstore {
        &self.blobstore
    }

    fn prefix(&self) -> &'static str {
        "derived_root_blame_v2."
    }

    fn options(&self) -> BlameDeriveOptions {
        self.options
    }

    fn serialize_value(&self, value: &Self::Value) -> Result<BlobstoreBytes> {
        Ok(value.root_manifest.into())
    }

    fn deserialize_value(&self, csid: ChangesetId, data: BlobstoreGetData) -> Result<Self::Value> {
        let root_manifest = data.try_into()?;
        Ok(RootBlameV2 {
            csid,
            root_manifest,
        })
    }
}

impl_bonsai_derived_mapping!(
    RootBlameV2Mapping,
    BlobstoreExistsWithDataMapping,
    RootBlameV2
);
