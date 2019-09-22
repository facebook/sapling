// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use blobrepo::BlobRepo;
use blobrepo_factory::{open_blobrepo, Caching};
use bookmarks::{BookmarkName, BookmarkPrefix};
use context::CoreContext;
use failure::Error;
use fbinit::FacebookInit;
use futures::stream::{self, Stream};
use futures_ext::StreamExt;
use futures_preview::compat::Future01CompatExt;
use metaconfig_types::{CommonConfig, RepoConfig};
use mononoke_types::RepositoryId;
use skiplist::{fetch_skiplist_index, SkiplistIndex};
use slog::Logger;
use unodes::RootUnodeManifestMapping;

use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::specifiers::{ChangesetId, ChangesetSpecifier, HgChangesetId};

pub(crate) struct Repo {
    pub(crate) blob_repo: BlobRepo,
    pub(crate) skiplist_index: Arc<SkiplistIndex>,
    pub(crate) _unodes_derived_mapping: Arc<RootUnodeManifestMapping>,
}

#[derive(Clone)]
pub struct RepoContext {
    ctx: CoreContext,
    repo: Arc<Repo>,
}

impl Repo {
    pub(crate) async fn new(
        fb: FacebookInit,
        logger: Logger,
        config: RepoConfig,
        common_config: CommonConfig,
        myrouter_port: Option<u16>,
        with_cachelib: Caching,
    ) -> Result<Self, Error> {
        let skiplist_index_blobstore_key = config.skiplist_index_blobstore_key.clone();

        let repoid = RepositoryId::new(config.repoid);

        let blob_repo = open_blobrepo(
            fb,
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

        let ctx = CoreContext::new_with_logger(fb, logger.clone());
        let skiplist_index = fetch_skiplist_index(
            ctx,
            skiplist_index_blobstore_key,
            blob_repo.get_blobstore().boxed(),
        )
        .compat()
        .await?;

        let unodes_derived_mapping =
            Arc::new(RootUnodeManifestMapping::new(blob_repo.get_blobstore()));

        Ok(Self {
            blob_repo,
            skiplist_index,
            _unodes_derived_mapping: unodes_derived_mapping,
        })
    }

    /// Temporary function to create directly from parts.
    pub(crate) fn new_from_parts(
        blob_repo: BlobRepo,
        skiplist_index: Arc<SkiplistIndex>,
        unodes_derived_mapping: Arc<RootUnodeManifestMapping>,
    ) -> Self {
        Self {
            blob_repo,
            skiplist_index,
            _unodes_derived_mapping: unodes_derived_mapping,
        }
    }

    #[cfg(test)]
    /// Construct a Repo from a test BlobRepo
    pub(crate) fn new_test(blob_repo: BlobRepo) -> Self {
        let unodes_derived_mapping =
            Arc::new(RootUnodeManifestMapping::new(blob_repo.get_blobstore()));
        Self {
            blob_repo,
            skiplist_index: Arc::new(SkiplistIndex::new()),
            _unodes_derived_mapping: unodes_derived_mapping,
        }
    }
}

/// A context object representing a query to a particular repo.
impl RepoContext {
    pub(crate) fn new(ctx: CoreContext, repo: Arc<Repo>) -> Self {
        Self { repo, ctx }
    }

    /// The context for this query.
    pub(crate) fn ctx(&self) -> &CoreContext {
        &self.ctx
    }

    /// The underlying `BlobRepo`.
    pub(crate) fn blob_repo(&self) -> &BlobRepo {
        &self.repo.blob_repo
    }

    /// The skiplist index for the referenced repository.
    pub(crate) fn skiplist_index(&self) -> &SkiplistIndex {
        &self.repo.skiplist_index
    }

    /// Look up a changeset specifier to find the canonical bonsai changeset
    /// ID for a changeset.
    pub async fn resolve_specifier(
        &self,
        specifier: ChangesetSpecifier,
    ) -> Result<Option<ChangesetId>, MononokeError> {
        let id = match specifier {
            ChangesetSpecifier::Bonsai(cs_id) => {
                let exists = self
                    .blob_repo()
                    .changeset_exists_by_bonsai(self.ctx.clone(), cs_id)
                    .compat()
                    .await?;
                match exists {
                    true => Some(cs_id),
                    false => None,
                }
            }
            ChangesetSpecifier::Hg(hg_cs_id) => {
                self.blob_repo()
                    .get_bonsai_from_hg(self.ctx.clone(), hg_cs_id)
                    .compat()
                    .await?
            }
        };
        Ok(id)
    }

    /// Resolve a bookmark to a changeset.
    pub async fn resolve_bookmark(
        &self,
        bookmark: impl ToString,
    ) -> Result<Option<ChangesetContext>, MononokeError> {
        let bookmark = BookmarkName::new(bookmark.to_string())?;
        let cs_id = self
            .blob_repo()
            .get_bonsai_bookmark(self.ctx.clone(), &bookmark)
            .compat()
            .await?;
        Ok(cs_id.map(|cs_id| ChangesetContext::new(self.clone(), cs_id)))
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

    /// Get Mercurial ID for multiple changesets
    ///
    /// This is a more efficient version of:
    /// ```ignore
    /// let ids: Vec<ChangesetId> = ...;
    /// ids.into_iter().map(|id| {
    ///     let hg_id = repo
    ///         .changeset(ChangesetSpecifier::Bonsai(id))
    ///         .await
    ///         .hg_id();
    ///     (id, hg_id)
    /// });
    /// ```
    pub async fn changeset_hg_ids(
        &self,
        changesets: Vec<ChangesetId>,
    ) -> Result<Vec<(ChangesetId, HgChangesetId)>, MononokeError> {
        let mapping = self
            .blob_repo()
            .get_hg_bonsai_mapping(self.ctx.clone(), changesets)
            .compat()
            .await?
            .into_iter()
            .map(|(hg_cs_id, cs_id)| (cs_id, hg_cs_id))
            .collect();
        Ok(mapping)
    }

    /// Get a list of bookmarks.
    pub fn list_bookmarks(
        &self,
        include_scratch: bool,
        prefix: Option<String>,
        limit: Option<u64>,
    ) -> impl Stream<Item = (String, ChangesetId), Error = MononokeError> {
        if include_scratch {
            let prefix = match prefix.map(BookmarkPrefix::new) {
                Some(Ok(prefix)) => prefix,
                Some(Err(e)) => {
                    return stream::once(Err(MononokeError::InvalidRequest(format!(
                        "invalid bookmark prefix: {}",
                        e
                    ))))
                    .boxify()
                }
                None => {
                    return stream::once(Err(MononokeError::InvalidRequest(
                        "prefix required to list scratch bookmarks".to_string(),
                    )))
                    .boxify()
                }
            };
            let limit = match limit {
                Some(limit) => limit,
                None => {
                    return stream::once(Err(MononokeError::InvalidRequest(
                        "limit required to list scratch bookmarks".to_string(),
                    )))
                    .boxify()
                }
            };
            self.blob_repo()
                .get_bonsai_bookmarks_by_prefix_maybe_stale(self.ctx.clone(), &prefix, limit)
                .map(|(bookmark, cs_id)| (bookmark.into_name().into_string(), cs_id))
                .map_err(MononokeError::from)
                .boxify()
        } else {
            // TODO(mbthomas): honour `limit` for publishing bookmarks
            let prefix = prefix.unwrap_or_else(|| "".to_string());
            self.blob_repo()
                .get_bonsai_publishing_bookmarks_maybe_stale(self.ctx.clone())
                .filter_map(move |(bookmark, cs_id)| {
                    let name = bookmark.into_name().into_string();
                    if name.starts_with(&prefix) {
                        Some((name, cs_id))
                    } else {
                        None
                    }
                })
                .map_err(MononokeError::from)
                .boxify()
        }
    }
}
