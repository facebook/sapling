// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use blobrepo::BlobRepo;
use blobrepo_factory::{open_blobrepo, Caching};
use blobstore::Blobstore;
use context::CoreContext;
use derive_unode_manifest::derived_data_unodes::RootUnodeManifestMapping;
use failure::Error;
use futures_preview::compat::Future01CompatExt;
use metaconfig_types::{CommonConfig, RepoConfig};
use mononoke_types::RepositoryId;
use skiplist::{deserialize_skiplist_index, SkiplistIndex};
use slog::Logger;

use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::specifiers::{ChangesetId, ChangesetSpecifier};

pub(crate) struct Repo {
    pub(crate) blob_repo: BlobRepo,
    pub(crate) _skiplist_index: Arc<SkiplistIndex>,
    pub(crate) _unodes_derived_mapping: Arc<RootUnodeManifestMapping>,
}

#[derive(Clone)]
pub struct RepoContext {
    pub(crate) repo: Arc<Repo>,
    pub(crate) ctx: CoreContext,
}

impl Repo {
    pub(crate) async fn new(
        logger: Logger,
        config: RepoConfig,
        common_config: CommonConfig,
        myrouter_port: Option<u16>,
        with_cachelib: Caching,
    ) -> Result<Self, Error> {
        let skiplist_index_blobstore_key = config.skiplist_index_blobstore_key.clone();

        let repoid = RepositoryId::new(config.repoid);

        let blob_repo = open_blobrepo(
            config.storage_config.clone(),
            repoid,
            myrouter_port,
            with_cachelib,
            config.bookmarks_cache_ttl,
            config.redaction,
            common_config.scuba_censored_table,
            config.filestore,
            logger.clone(),
        )
        .compat()
        .await?;

        let skiplist_index = match skiplist_index_blobstore_key.clone() {
            Some(skiplist_index_blobstore_key) => {
                let ctx = CoreContext::new_with_logger(logger.clone());
                let bytes = blob_repo
                    .get_blobstore()
                    .get(ctx, skiplist_index_blobstore_key)
                    .compat()
                    .await;
                if let Ok(Some(bytes)) = bytes {
                    let bytes = bytes.into_bytes();
                    deserialize_skiplist_index(logger, bytes)?
                } else {
                    SkiplistIndex::new()
                }
            }
            None => SkiplistIndex::new(),
        };
        let unodes_derived_mapping =
            Arc::new(RootUnodeManifestMapping::new(blob_repo.get_blobstore()));

        Ok(Self {
            blob_repo,
            _skiplist_index: Arc::new(skiplist_index),
            _unodes_derived_mapping: unodes_derived_mapping,
        })
    }

    #[cfg(test)]
    /// Construct a Repo from a test BlobRepo
    pub(crate) fn new_test(blob_repo: BlobRepo) -> Self {
        let unodes_derived_mapping =
            Arc::new(RootUnodeManifestMapping::new(blob_repo.get_blobstore()));
        Self {
            blob_repo,
            _skiplist_index: Arc::new(SkiplistIndex::new()),
            _unodes_derived_mapping: unodes_derived_mapping,
        }
    }
}

impl RepoContext {
    /// Look up a changeset specifier to find the canonical bonsai changeset
    /// ID for a changeset.
    pub async fn resolve_specifier(
        &self,
        specifier: ChangesetSpecifier,
    ) -> Result<Option<ChangesetId>, MononokeError> {
        let id = match specifier {
            ChangesetSpecifier::Bonsai(cs_id) => {
                let exists = self
                    .repo
                    .blob_repo
                    .changeset_exists_by_bonsai(self.ctx.clone(), cs_id)
                    .compat()
                    .await?;
                match exists {
                    true => Some(cs_id),
                    false => None,
                }
            }
            ChangesetSpecifier::Hg(hg_cs_id) => {
                self.repo
                    .blob_repo
                    .get_bonsai_from_hg(self.ctx.clone(), hg_cs_id)
                    .compat()
                    .await?
            }
        };
        Ok(id)
    }

    /// Look up a changeset by specifier.
    pub async fn changeset(
        &self,
        specifier: ChangesetSpecifier,
    ) -> Result<Option<ChangesetContext>, MononokeError> {
        let changeset = self
            .resolve_specifier(specifier)
            .await?
            .map(|cs_id| ChangesetContext::new(self.clone(), cs_id));
        Ok(changeset)
    }
}
