/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt;
use std::sync::Arc;

use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_factory::{open_blobrepo, Caching, ReadOnlyStorage};
use bookmarks::{BookmarkName, BookmarkPrefix};
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::{Alias, FetchKey};
use fsnodes::{derive_fsnodes, RootFsnodeMapping};

use blame::derive_blame;
use futures::stream::{self, Stream};
use futures_ext::StreamExt;
use futures_preview::compat::Future01CompatExt;
use futures_preview::future::try_join_all;
use mercurial_types::Globalrev;
use metaconfig_types::{
    CommonConfig, MetadataDBConfig, RepoConfig, SourceControlServiceMonitoring,
};
use mononoke_types::hash::{Sha1, Sha256};
use skiplist::{fetch_skiplist_index, SkiplistIndex};
use slog::{debug, error, Logger};
use stats::service_data::{get_service_data_singleton, ServiceData};
use synced_commit_mapping::{SqlConstructors, SqlSyncedCommitMapping, SyncedCommitMapping};
use unodes::{derive_unodes, RootUnodeManifestMapping};
use warm_bookmarks_cache::{warm_hg_changeset, WarmBookmarksCache};

use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::file::{FileContext, FileId};
use crate::specifiers::{ChangesetId, ChangesetSpecifier, HgChangesetId};
use crate::tree::{TreeContext, TreeId};

const COMMON_COUNTER_PREFIX: &'static str = "mononoke.api";
const STALENESS_INFIX: &'static str = "staleness.secs";
const MISSING_FROM_CACHE_INFIX: &'static str = "missing_from_cache";
const MISSING_FROM_REPO_INFIX: &'static str = "missing_from_repo";

pub(crate) struct Repo {
    pub(crate) name: String,
    pub(crate) blob_repo: BlobRepo,
    pub(crate) skiplist_index: Arc<SkiplistIndex>,
    pub(crate) fsnodes_derived_mapping: Arc<RootFsnodeMapping>,
    pub(crate) unodes_derived_mapping: Arc<RootUnodeManifestMapping>,
    pub(crate) warm_bookmarks_cache: Arc<WarmBookmarksCache>,
    // This doesn't really belong here, but until we have production mappings, we can't do a better job
    pub(crate) synced_commit_mapping: Arc<dyn SyncedCommitMapping>,
    // Needed to report stats
    pub(crate) monitoring_config: Option<SourceControlServiceMonitoring>,
}

#[derive(Clone)]
pub struct RepoContext {
    ctx: CoreContext,
    repo: Arc<Repo>,
}

impl fmt::Debug for RepoContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RepoContext(repo={:?})", self.name())
    }
}

pub async fn open_synced_commit_mapping(
    config: RepoConfig,
    myrouter_port: Option<u16>,
    readonly_storage: ReadOnlyStorage,
) -> Result<SqlSyncedCommitMapping, Error> {
    let name = SqlSyncedCommitMapping::LABEL;
    match config.storage_config.dbconfig {
        MetadataDBConfig::LocalDB { path } => {
            SqlSyncedCommitMapping::with_sqlite_path(path.join(name), readonly_storage.0)
        }
        MetadataDBConfig::Mysql { db_address, .. } => {
            SqlSyncedCommitMapping::with_xdb(db_address, myrouter_port, readonly_storage.0)
                .compat()
                .await
        }
    }
}

impl Repo {
    pub(crate) async fn new(
        fb: FacebookInit,
        logger: Logger,
        name: String,
        config: RepoConfig,
        common_config: CommonConfig,
        myrouter_port: Option<u16>,
        with_cachelib: Caching,
        readonly_storage: ReadOnlyStorage,
    ) -> Result<Self, Error> {
        let skiplist_index_blobstore_key = config.skiplist_index_blobstore_key.clone();

        let repoid = config.repoid;

        let synced_commit_mapping = Arc::new(
            open_synced_commit_mapping(config.clone(), myrouter_port, readonly_storage).await?,
        );
        let monitoring_config = config.source_control_service_monitoring.clone();

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
            readonly_storage,
            logger.clone(),
        )
        .compat()
        .await?;

        let ctx = CoreContext::new_with_logger(fb, logger.clone());
        let skiplist_index = fetch_skiplist_index(
            ctx.clone(),
            skiplist_index_blobstore_key,
            blob_repo.get_blobstore().boxed(),
        )
        .compat()
        .await?;

        let unodes_derived_mapping =
            Arc::new(RootUnodeManifestMapping::new(blob_repo.get_blobstore()));
        let fsnodes_derived_mapping = Arc::new(RootFsnodeMapping::new(blob_repo.get_blobstore()));

        let warm_bookmarks_cache = Arc::new(
            WarmBookmarksCache::new(
                ctx.clone(),
                blob_repo.clone(),
                vec![
                    Box::new(&warm_hg_changeset),
                    Box::new(&derive_unodes),
                    Box::new(&derive_fsnodes),
                    Box::new(&derive_blame),
                ],
            )
            .compat()
            .await?,
        );

        Ok(Self {
            name,
            blob_repo,
            skiplist_index,
            unodes_derived_mapping,
            fsnodes_derived_mapping,
            warm_bookmarks_cache,
            synced_commit_mapping,
            monitoring_config,
        })
    }

    /// Temporary function to create directly from parts.
    pub(crate) fn new_from_parts(
        name: String,
        blob_repo: BlobRepo,
        skiplist_index: Arc<SkiplistIndex>,
        fsnodes_derived_mapping: Arc<RootFsnodeMapping>,
        unodes_derived_mapping: Arc<RootUnodeManifestMapping>,
        warm_bookmarks_cache: Arc<WarmBookmarksCache>,
        synced_commit_mapping: Arc<dyn SyncedCommitMapping>,
        monitoring_config: Option<SourceControlServiceMonitoring>,
    ) -> Self {
        Self {
            name,
            blob_repo,
            skiplist_index,
            fsnodes_derived_mapping,
            unodes_derived_mapping,
            warm_bookmarks_cache,
            synced_commit_mapping,
            monitoring_config,
        }
    }

    #[cfg(test)]
    /// Construct a Repo from a test BlobRepo
    pub(crate) async fn new_test(ctx: CoreContext, blob_repo: BlobRepo) -> Result<Self, Error> {
        let unodes_derived_mapping =
            Arc::new(RootUnodeManifestMapping::new(blob_repo.get_blobstore()));
        let fsnodes_derived_mapping = Arc::new(RootFsnodeMapping::new(blob_repo.get_blobstore()));
        let synced_commit_mapping =
            Arc::new(SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap());
        let warm_bookmarks_cache = Arc::new(
            WarmBookmarksCache::new(
                ctx.clone(),
                blob_repo.clone(),
                vec![
                    Box::new(&warm_hg_changeset),
                    Box::new(&derive_unodes),
                    Box::new(&derive_blame),
                ],
            )
            .compat()
            .await?,
        );
        Ok(Self {
            name: String::from("test"),
            blob_repo,
            skiplist_index: Arc::new(SkiplistIndex::new()),
            unodes_derived_mapping,
            fsnodes_derived_mapping,
            warm_bookmarks_cache,
            synced_commit_mapping,
            monitoring_config: None,
        })
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

    /// The name of the underlying repo.
    pub(crate) fn name(&self) -> &str {
        &self.repo.name
    }

    /// The underlying `BlobRepo`.
    pub(crate) fn blob_repo(&self) -> &BlobRepo {
        &self.repo.blob_repo
    }

    /// The skiplist index for the referenced repository.
    pub(crate) fn skiplist_index(&self) -> &SkiplistIndex {
        &self.repo.skiplist_index
    }

    /// The fsnodes mapping for the referenced repository.
    pub(crate) fn fsnodes_derived_mapping(&self) -> &Arc<RootFsnodeMapping> {
        &self.repo.fsnodes_derived_mapping
    }

    /// The unodes mapping for the referenced repository.
    pub(crate) fn unodes_derived_mapping(&self) -> &Arc<RootUnodeManifestMapping> {
        &self.repo.unodes_derived_mapping
    }

    /// The commit sync mapping for the referenced repository
    pub(crate) fn synced_commit_mapping(&self) -> &Arc<dyn SyncedCommitMapping> {
        &self.repo.synced_commit_mapping
    }

    /// The warm bookmarks cache for the references repository.
    pub(crate) fn warm_bookmarks_cache(&self) -> &Arc<WarmBookmarksCache> {
        &self.repo.warm_bookmarks_cache
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
            ChangesetSpecifier::Globalrev(rev) => {
                self.blob_repo()
                    .get_bonsai_from_globalrev(rev)
                    .compat()
                    .await?
            }
        };
        Ok(id)
    }

    /// Resolve a bookmark to a changeset.
    pub async fn resolve_bookmark(
        &self,
        bookmark: impl AsRef<str>,
    ) -> Result<Option<ChangesetContext>, MononokeError> {
        let bookmark = BookmarkName::new(bookmark.as_ref())?;
        self.resolve_bookmark_impl(&bookmark).await
    }

    async fn resolve_bookmark_impl(
        &self,
        bookmark: &BookmarkName,
    ) -> Result<Option<ChangesetContext>, MononokeError> {
        let mut cs_id = self.warm_bookmarks_cache().get(&bookmark);

        if cs_id.is_none() {
            // The bookmark wasn't in the warm bookmark cache.  Check
            // the blobrepo directly in case this is a bookmark that
            // has just been created.
            cs_id = self
                .blob_repo()
                .get_bonsai_bookmark(self.ctx.clone(), &bookmark)
                .compat()
                .await?;
        }

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

    /// Similar to changeset_hg_ids, but returning Globalrevs.
    pub async fn changeset_globalrev_ids(
        &self,
        changesets: Vec<ChangesetId>,
    ) -> Result<Vec<(ChangesetId, Globalrev)>, MononokeError> {
        let mapping = self
            .blob_repo()
            .get_bonsai_globalrev_mapping(changesets)
            .compat()
            .await?
            .into_iter()
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

    /// Get a Tree by id.  Returns `None` if the tree doesn't exist.
    pub async fn tree(&self, tree_id: TreeId) -> Result<Option<TreeContext>, MononokeError> {
        TreeContext::new_check_exists(self.clone(), tree_id).await
    }

    /// Get a File by id.  Returns `None` if the file doesn't exist.
    pub async fn file(&self, file_id: FileId) -> Result<Option<FileContext>, MononokeError> {
        FileContext::new_check_exists(self.clone(), FetchKey::Canonical(file_id)).await
    }

    /// Get a File by content sha-1.  Returns `None` if the file doesn't exist.
    pub async fn file_by_content_sha1(
        &self,
        hash: Sha1,
    ) -> Result<Option<FileContext>, MononokeError> {
        FileContext::new_check_exists(self.clone(), FetchKey::Aliased(Alias::Sha1(hash))).await
    }

    /// Get a File by content sha-256.  Returns `None` if the file doesn't exist.
    pub async fn file_by_content_sha256(
        &self,
        hash: Sha256,
    ) -> Result<Option<FileContext>, MononokeError> {
        FileContext::new_check_exists(self.clone(), FetchKey::Aliased(Alias::Sha256(hash))).await
    }

    /// Get the equivalent changeset from another repo, if synced
    pub async fn xrepo_commit_lookup(
        &self,
        other: &Self,
        specifier: ChangesetSpecifier,
    ) -> Result<Option<ChangesetContext>, MononokeError> {
        let changeset = self.resolve_specifier(specifier).await?;

        let remapped_changeset_id = match changeset {
            Some(changeset) => {
                self.synced_commit_mapping()
                    .get(
                        self.ctx().clone(),
                        self.blob_repo().get_repoid(),
                        changeset,
                        other.blob_repo().get_repoid(),
                    )
                    .compat()
                    .await?
            }
            None => None,
        };
        let changeset = changeset.map(|changeset| remapped_changeset_id.unwrap_or(changeset));
        match changeset {
            Some(changeset) => other.changeset(ChangesetSpecifier::Bonsai(changeset)).await,
            None => Ok(None),
        }
    }

    pub async fn report_monitoring_stats(&self) -> Result<(), MononokeError> {
        match self.repo.monitoring_config.as_ref() {
            None => Ok(()),
            Some(monitoring_config) => {
                let reporting_futs = monitoring_config
                    .bookmarks_to_report_age
                    .iter()
                    .map(move |bookmark| self.report_bookmark_age_difference(&bookmark));
                try_join_all(reporting_futs).await.map(|_| ())
            }
        }
    }

    fn set_counter(&self, name: &dyn AsRef<str>, value: i64) {
        get_service_data_singleton(self.ctx.fb).set_counter(name, value);
    }

    fn report_bookmark_missing_from_cache(&self, bookmark: &BookmarkName) {
        error!(
            self.ctx().logger(),
            "Monitored bookmark does not exist in the cache: {}", bookmark
        );

        let counter_name = format!(
            "{}.{}.{}.{}",
            COMMON_COUNTER_PREFIX,
            MISSING_FROM_CACHE_INFIX,
            self.blob_repo().get_repoid(),
            bookmark,
        );
        self.set_counter(&counter_name, 1);
    }

    fn report_bookmark_missing_from_repo(&self, bookmark: &BookmarkName) {
        error!(
            self.ctx().logger(),
            "Monitored bookmark does not exist in the repo: {}", bookmark
        );

        let counter_name = format!(
            "{}.{}.{}.{}",
            COMMON_COUNTER_PREFIX,
            MISSING_FROM_REPO_INFIX,
            self.blob_repo().get_repoid(),
            bookmark,
        );
        self.set_counter(&counter_name, 1);
    }

    fn report_bookmark_staleness(&self, bookmark: &BookmarkName, staleness: i64) {
        debug!(
            self.ctx().logger(),
            "Reporting staleness of {} to be {}s", bookmark, staleness
        );

        let counter_name = format!(
            "{}.{}.{}.{}",
            COMMON_COUNTER_PREFIX,
            STALENESS_INFIX,
            self.blob_repo().get_repoid(),
            bookmark,
        );
        self.set_counter(&counter_name, staleness);
    }

    async fn report_bookmark_age_difference(
        &self,
        bookmark: &BookmarkName,
    ) -> Result<(), MononokeError> {
        let repo = self.blob_repo();
        let ctx = self.ctx();

        let maybe_changeset_context_from_service = self.resolve_bookmark_impl(bookmark).await?;
        let maybe_bcs_id_from_blobrepo = repo
            .get_bonsai_bookmark(ctx.clone(), &bookmark)
            .compat()
            .await?;

        if maybe_bcs_id_from_blobrepo.is_none() {
            self.report_bookmark_missing_from_repo(bookmark);
        }

        if maybe_changeset_context_from_service.is_none() {
            self.report_bookmark_missing_from_cache(bookmark);
        }

        let (changeset_context_from_service, bcs_id) = match (
            maybe_changeset_context_from_service,
            maybe_bcs_id_from_blobrepo,
        ) {
            (Some(changeset_context_from_service), Some(bcs_id)) => {
                (changeset_context_from_service, bcs_id)
            }
            // At this point we've already reported missing monitored bookmark
            // either in the cache, or in the repo, or in both. Let's just silenly
            // return success, as we don't want the service to crash.
            _ => return Ok(()),
        };

        let bcs = repo
            .get_bonsai_changeset(ctx.clone(), bcs_id)
            .compat()
            .await?;

        let timestamp_as_per_cache = changeset_context_from_service
            .author_date()
            .await?
            .timestamp();

        let timestamp_as_per_blobrepo = bcs.author_date().timestamp_secs();

        let difference = timestamp_as_per_blobrepo - timestamp_as_per_cache;
        self.report_bookmark_staleness(bookmark, difference);

        Ok(())
    }
}
