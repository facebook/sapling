/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Error, Result};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use context::CoreContext;
use derived_data::{
    impl_bonsai_derived_mapping, BlobstoreExistsMapping, BonsaiDerivable, BonsaiDerived,
    DerivedDataTypesConfig,
};
use mononoke_types::{BonsaiChangeset, ChangesetId};
use std::sync::Arc;
use unodes::RootUnodeManifestId;

use crate::derive_v1::derive_blame_v1;
use crate::{BlameDeriveOptions, DEFAULT_BLAME_FILESIZE_LIMIT};

#[derive(Debug, Clone, Copy)]
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
        derive_blame_v1(ctx, repo, bonsai, root_manifest, options).await?;
        Ok(BlameRoot(csid))
    }
}

#[derive(Clone)]
pub struct BlameRootMapping {
    blobstore: Arc<dyn Blobstore>,
    options: BlameDeriveOptions,
    repo: BlobRepo,
}

#[async_trait]
impl BlobstoreExistsMapping for BlameRootMapping {
    type Value = BlameRoot;

    fn new(repo: &BlobRepo, config: &DerivedDataTypesConfig) -> Result<Self> {
        let filesize_limit = config
            .blame_filesize_limit
            .unwrap_or(DEFAULT_BLAME_FILESIZE_LIMIT);
        let options = BlameDeriveOptions { filesize_limit };
        Ok(Self {
            blobstore: repo.get_blobstore().boxed(),
            options,
            repo: repo.clone(),
        })
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

    fn repo_name(&self) -> &str {
        self.repo.name()
    }

    fn derived_data_scuba_table(&self) -> &Option<String> {
        &self.repo.get_derived_data_config().scuba_table
    }
}

impl_bonsai_derived_mapping!(BlameRootMapping, BlobstoreExistsMapping, BlameRoot);
