/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error, Result};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use context::CoreContext;
use derived_data::{
    impl_bonsai_derived_mapping, BlobstoreExistsMapping, BonsaiDerivable, BonsaiDerived,
    DerivedDataTypesConfig,
};
use metaconfig_types::BlameVersion;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use std::sync::Arc;
use unodes::RootUnodeManifestId;

use crate::derive_v1::derive_blame_v1;
use crate::{BlameDeriveOptions, DEFAULT_BLAME_FILESIZE_LIMIT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlameRoot(ChangesetId);

impl From<ChangesetId> for BlameRoot {
    fn from(csid: ChangesetId) -> BlameRoot {
        BlameRoot(csid)
    }
}

#[async_trait]
impl BonsaiDerivable for BlameRoot {
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
        if options.blame_version != BlameVersion::V1 {
            return Err(anyhow!(
                "programming error: incorrect blame version (expected V1)"
            ));
        }
        derive_blame_v1(ctx, repo, bonsai, root_manifest, options).await?;
        Ok(BlameRoot(csid))
    }
}

#[derive(Clone)]
pub struct BlameRootMapping {
    blobstore: Arc<dyn Blobstore>,
    options: BlameDeriveOptions,
}

#[async_trait]
impl BlobstoreExistsMapping for BlameRootMapping {
    type Value = BlameRoot;

    fn new(blobstore: Arc<dyn Blobstore>, config: &DerivedDataTypesConfig) -> Result<Self> {
        let filesize_limit = config
            .blame_filesize_limit
            .unwrap_or(DEFAULT_BLAME_FILESIZE_LIMIT);
        let blame_version = config.blame_version;
        if blame_version != BlameVersion::V1 {
            return Err(anyhow!(
                "programming error: incorrect blame mapping version (expected V1)"
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
        "derived_rootblame.v1."
    }

    fn options(&self) -> BlameDeriveOptions {
        self.options
    }
}

impl_bonsai_derived_mapping!(BlameRootMapping, BlobstoreExistsMapping, BlameRoot);
