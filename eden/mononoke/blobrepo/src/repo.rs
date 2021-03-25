/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobstore::Blobstore;
use bonsai_git_mapping::{ArcBonsaiGitMapping, BonsaiGitMapping};
use bonsai_globalrev_mapping::{
    ArcBonsaiGlobalrevMapping, BonsaiGlobalrevMapping, BonsaisOrGlobalrevs,
};
use bonsai_hg_mapping::{ArcBonsaiHgMapping, BonsaiHgMapping};
use bonsai_svnrev_mapping::RepoBonsaiSvnrevMapping;
use bookmarks::{
    self, ArcBookmarkUpdateLog, ArcBookmarks, Bookmark, BookmarkKind, BookmarkName,
    BookmarkPagination, BookmarkPrefix, BookmarkTransaction, BookmarkUpdateLog,
    BookmarkUpdateLogEntry, BookmarkUpdateReason, Bookmarks, Freshness,
};
use cacheblob::LeaseOps;
use changeset_fetcher::{ArcChangesetFetcher, ChangesetFetcher};
use changesets::{ArcChangesets, ChangesetInsert, Changesets};
use cloned::cloned;
use context::CoreContext;
use filenodes::{ArcFilenodes, Filenodes};
use filestore::FilestoreConfig;
use futures::{
    future::{try_join, BoxFuture},
    stream::FuturesUnordered,
    FutureExt, Stream, TryStreamExt,
};
use mercurial_mutation::{ArcHgMutationStore, HgMutationStore};
use metaconfig_types::DerivedDataConfig;
use mononoke_types::{
    BlobstoreValue, BonsaiChangeset, ChangesetId, Generation, Globalrev, MononokeId, RepositoryId,
};
use phases::{HeadsFetcher, Phases, SqlPhasesFactory};
use repo_blobstore::{RepoBlobstore, RepoBlobstoreArgs};
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use segmented_changelog_types::{ArcSegmentedChangelog, SegmentedChangelog};
use stats::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use topo_sort::sort_topological;

define_stats! {
    prefix = "mononoke.blobrepo";
    changeset_exists_by_bonsai: timeseries(Rate, Sum),
    get_bonsai_heads_maybe_stale: timeseries(Rate, Sum),
    get_bonsai_publishing_bookmarks_maybe_stale: timeseries(Rate, Sum),
    get_bookmark: timeseries(Rate, Sum),
    get_bookmarks_by_prefix_maybe_stale: timeseries(Rate, Sum),
    get_changeset_parents_by_bonsai: timeseries(Rate, Sum),
    get_generation_number: timeseries(Rate, Sum),
    update_bookmark_transaction: timeseries(Rate, Sum),
}

// NOTE: this structure and its fields are public to enable `DangerousOverride` functionality
#[facet::container]
#[derive(Clone)]
pub struct BlobRepoInner {
    #[facet]
    pub repo_identity: RepoIdentity,

    #[init(repo_identity.id())]
    pub repoid: RepositoryId,

    #[init(repo_identity.name().to_string())]
    pub reponame: String,

    #[facet]
    pub repo_blobstore: RepoBlobstore,

    #[facet]
    pub changesets: dyn Changesets,

    #[facet]
    pub changeset_fetcher: dyn ChangesetFetcher,

    #[facet]
    pub segmented_changelog: dyn SegmentedChangelog,

    #[facet]
    pub bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    pub bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    pub bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    pub repo_bonsai_svnrev_mapping: RepoBonsaiSvnrevMapping,

    #[facet]
    pub bookmarks: dyn Bookmarks,

    #[facet]
    pub bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    pub sql_phases_factory: SqlPhasesFactory,

    #[facet]
    pub filestore_config: FilestoreConfig,

    #[facet]
    pub filenodes: dyn Filenodes,

    #[facet]
    pub hg_mutation_store: dyn HgMutationStore,

    #[facet]
    pub repo_derived_data: RepoDerivedData,
}

#[facet::container]
#[derive(Clone)]
pub struct BlobRepo {
    #[delegate(
        RepoIdentity,
        RepoBlobstore,
        dyn Changesets,
        dyn ChangesetFetcher,
        dyn SegmentedChangelog,
        dyn BonsaiHgMapping,
        dyn BonsaiGitMapping,
        dyn BonsaiGlobalrevMapping,
        RepoBonsaiSvnrevMapping,
        dyn Bookmarks,
        dyn BookmarkUpdateLog,
        SqlPhasesFactory,
        FilestoreConfig,
        dyn Filenodes,
        dyn HgMutationStore,
        RepoDerivedData,
    )]
    inner: Arc<BlobRepoInner>,
}

impl BlobRepo {
    /// Create new `BlobRepo` object.
    ///
    /// Avoid using this constructor directly as it requires properly initialized `attributes`
    /// argument. Instead use `blobrepo_factory::*` functions.
    pub fn new_dangerous(
        blobstore_args: RepoBlobstoreArgs,
        changesets: ArcChangesets,
        bonsai_git_mapping: ArcBonsaiGitMapping,
        bonsai_globalrev_mapping: ArcBonsaiGlobalrevMapping,
        bonsai_svnrev_mapping: RepoBonsaiSvnrevMapping,
        derived_data_lease: Arc<dyn LeaseOps>,
        filestore_config: FilestoreConfig,
        phases_factory: SqlPhasesFactory,
        derived_data_config: DerivedDataConfig,
        reponame: String,
        bookmarks: ArcBookmarks,
        bookmark_update_log: ArcBookmarkUpdateLog,
        bonsai_hg_mapping: ArcBonsaiHgMapping,
        changeset_fetcher: ArcChangesetFetcher,
        filenodes: ArcFilenodes,
        hg_mutation_store: ArcHgMutationStore,
        segmented_changelog: ArcSegmentedChangelog,
    ) -> Self {
        let (blobstore, repoid) = blobstore_args.into_blobrepo_parts();

        let repo_blobstore = Arc::new(blobstore);
        let filestore_config = Arc::new(filestore_config);
        let sql_phases_factory = Arc::new(phases_factory);
        let repo_bonsai_svnrev_mapping = Arc::new(bonsai_svnrev_mapping);
        let repo_identity = Arc::new(RepoIdentity::new(repoid, reponame.clone()));
        let repo_derived_data = Arc::new(RepoDerivedData::new(
            derived_data_config,
            derived_data_lease,
        ));
        let inner = BlobRepoInner {
            repo_blobstore,
            changesets,
            bonsai_git_mapping,
            bonsai_globalrev_mapping,
            repo_bonsai_svnrev_mapping,
            repoid,
            filestore_config,
            sql_phases_factory,
            reponame,
            bookmarks,
            bookmark_update_log,
            bonsai_hg_mapping,
            changeset_fetcher,
            filenodes,
            hg_mutation_store,
            segmented_changelog,
            repo_identity,
            repo_derived_data,
        };
        BlobRepo {
            inner: Arc::new(inner),
        }
    }

    #[inline]
    pub fn bookmarks(&self) -> &ArcBookmarks {
        &self.inner.bookmarks
    }

    #[inline]
    pub fn bookmark_update_log(&self) -> &ArcBookmarkUpdateLog {
        &self.inner.bookmark_update_log
    }

    #[inline]
    pub fn bonsai_hg_mapping(&self) -> &ArcBonsaiHgMapping {
        &self.inner.bonsai_hg_mapping
    }

    #[inline]
    pub fn changeset_fetcher(&self) -> &ArcChangesetFetcher {
        &self.inner.changeset_fetcher
    }

    #[inline]
    pub fn filenodes(&self) -> &ArcFilenodes {
        &self.inner.filenodes
    }

    #[inline]
    pub fn hg_mutation_store(&self) -> &ArcHgMutationStore {
        &self.inner.hg_mutation_store
    }

    #[inline]
    pub fn segmented_changelog(&self) -> &ArcSegmentedChangelog {
        &self.inner.segmented_changelog
    }

    /// Get Bonsai changesets for Mercurial heads, which we approximate as Publishing Bonsai
    /// Bookmarks. Those will be served from cache, so they might be stale.
    pub fn get_bonsai_heads_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = Result<ChangesetId, Error>> {
        STATS::get_bonsai_heads_maybe_stale.add_value(1);
        self.bookmarks()
            .list(
                ctx,
                Freshness::MaybeStale,
                &BookmarkPrefix::empty(),
                BookmarkKind::ALL_PUBLISHING,
                &BookmarkPagination::FromStart,
                std::u64::MAX,
            )
            .map_ok(|(_, cs_id)| cs_id)
    }

    /// List all publishing Bonsai bookmarks.
    pub fn get_bonsai_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = Result<(Bookmark, ChangesetId), Error>> {
        STATS::get_bonsai_publishing_bookmarks_maybe_stale.add_value(1);
        self.bookmarks().list(
            ctx,
            Freshness::MaybeStale,
            &BookmarkPrefix::empty(),
            BookmarkKind::ALL_PUBLISHING,
            &BookmarkPagination::FromStart,
            std::u64::MAX,
        )
    }

    /// Get bookmarks by prefix, they will be read from replica, so they might be stale.
    pub fn get_bonsai_bookmarks_by_prefix_maybe_stale(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        max: u64,
    ) -> impl Stream<Item = Result<(Bookmark, ChangesetId), Error>> {
        STATS::get_bookmarks_by_prefix_maybe_stale.add_value(1);
        self.bookmarks().list(
            ctx,
            Freshness::MaybeStale,
            prefix,
            BookmarkKind::ALL,
            &BookmarkPagination::FromStart,
            max,
        )
    }

    pub async fn changeset_exists_by_bonsai(
        &self,
        ctx: CoreContext,
        changesetid: ChangesetId,
    ) -> Result<bool, Error> {
        STATS::changeset_exists_by_bonsai.add_value(1);
        let changeset = self
            .inner
            .changesets
            .get(ctx, self.get_repoid(), changesetid)
            .await?;
        Ok(changeset.is_some())
    }

    pub async fn get_changeset_parents_by_bonsai(
        &self,
        ctx: CoreContext,
        changesetid: ChangesetId,
    ) -> Result<Vec<ChangesetId>, Error> {
        STATS::get_changeset_parents_by_bonsai.add_value(1);
        let changeset = self
            .inner
            .changesets
            .get(ctx, self.get_repoid(), changesetid)
            .await?;
        let parents = changeset
            .ok_or_else(|| format_err!("Commit {} does not exist in the repo", changesetid))?
            .parents;
        Ok(parents)
    }

    pub fn get_bonsai_bookmark(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
    ) -> BoxFuture<'static, Result<Option<ChangesetId>, Error>> {
        STATS::get_bookmark.add_value(1);
        self.bookmarks().get(ctx, name)
    }

    pub fn bonsai_git_mapping(&self) -> &ArcBonsaiGitMapping {
        &self.inner.bonsai_git_mapping
    }

    pub fn bonsai_globalrev_mapping(&self) -> &ArcBonsaiGlobalrevMapping {
        &self.inner.bonsai_globalrev_mapping
    }

    pub fn bonsai_svnrev_mapping(&self) -> &RepoBonsaiSvnrevMapping {
        self.inner.repo_bonsai_svnrev_mapping.as_ref()
    }

    pub async fn get_bonsai_from_globalrev(
        &self,
        ctx: &CoreContext,
        globalrev: Globalrev,
    ) -> Result<Option<ChangesetId>, Error> {
        let maybe_changesetid = self
            .inner
            .bonsai_globalrev_mapping
            .get_bonsai_from_globalrev(ctx, self.get_repoid(), globalrev)
            .await?;
        Ok(maybe_changesetid)
    }

    pub async fn get_globalrev_from_bonsai(
        &self,
        ctx: &CoreContext,
        bcs: ChangesetId,
    ) -> Result<Option<Globalrev>, Error> {
        let maybe_globalrev = self
            .inner
            .bonsai_globalrev_mapping
            .get_globalrev_from_bonsai(ctx, self.get_repoid(), bcs)
            .await?;
        Ok(maybe_globalrev)
    }

    pub async fn get_bonsai_globalrev_mapping(
        &self,
        ctx: &CoreContext,
        bonsai_or_globalrev_ids: impl Into<BonsaisOrGlobalrevs>,
    ) -> Result<Vec<(ChangesetId, Globalrev)>, Error> {
        let entries = self
            .inner
            .bonsai_globalrev_mapping
            .get(ctx, self.get_repoid(), bonsai_or_globalrev_ids.into())
            .await?;
        Ok(entries
            .into_iter()
            .map(|entry| (entry.bcs_id, entry.globalrev))
            .collect())
    }

    pub fn read_next_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        limit: u64,
        freshness: Freshness,
    ) -> impl Stream<Item = Result<BookmarkUpdateLogEntry, Error>> {
        self.bookmark_update_log()
            .read_next_bookmark_log_entries(ctx, id, limit, freshness)
    }

    pub async fn count_further_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        exclude_reason: Option<BookmarkUpdateReason>,
    ) -> Result<u64, Error> {
        self.bookmark_update_log()
            .count_further_bookmark_log_entries(ctx, id, exclude_reason)
            .await
    }

    pub fn update_bookmark_transaction(&self, ctx: CoreContext) -> Box<dyn BookmarkTransaction> {
        STATS::update_bookmark_transaction.add_value(1);
        self.bookmarks().create_transaction(ctx)
    }

    // Returns the generation number of a changeset
    // note: it returns Option because changeset might not exist
    pub async fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs: ChangesetId,
    ) -> Result<Option<Generation>, Error> {
        STATS::get_generation_number.add_value(1);
        let result = self
            .inner
            .changesets
            .get(ctx, self.get_repoid(), cs)
            .await?;
        Ok(result.map(|res| Generation::new(res.gen)))
    }

    pub fn get_changeset_fetcher(&self) -> Arc<dyn ChangesetFetcher> {
        self.changeset_fetcher().clone()
    }

    pub fn blobstore(&self) -> &RepoBlobstore {
        &self.inner.repo_blobstore
    }

    pub fn get_blobstore(&self) -> RepoBlobstore {
        self.inner.repo_blobstore.as_ref().clone()
    }

    pub fn filestore_config(&self) -> FilestoreConfig {
        *self.inner.filestore_config
    }

    pub fn get_repoid(&self) -> RepositoryId {
        self.inner.repoid
    }

    pub fn get_phases(&self) -> Arc<dyn Phases> {
        self.inner.sql_phases_factory.get_phases(
            self.get_repoid(),
            self.get_changeset_fetcher(),
            self.get_heads_fetcher(),
        )
    }

    pub fn name(&self) -> &String {
        &self.inner.reponame
    }

    pub fn get_heads_fetcher(&self) -> HeadsFetcher {
        let this = self.clone();
        Arc::new(move |ctx: &CoreContext| {
            this.get_bonsai_heads_maybe_stale(ctx.clone())
                .try_collect()
                .boxed()
        })
    }

    pub fn get_phases_factory(&self) -> &SqlPhasesFactory {
        self.inner.sql_phases_factory.as_ref()
    }

    pub fn get_changesets_object(&self) -> Arc<dyn Changesets> {
        self.inner.changesets.clone()
    }

    pub fn get_derived_data_config(&self) -> &DerivedDataConfig {
        &self.inner.repo_derived_data.config()
    }

    pub fn get_derived_data_lease_ops(&self) -> Arc<dyn LeaseOps> {
        self.inner.repo_derived_data.lease().clone()
    }

    /// To be used by `DangerousOverride` only
    pub fn inner(&self) -> &Arc<BlobRepoInner> {
        &self.inner
    }

    /// To be used by `DagerouseOverride` only
    pub fn from_inner_dangerous(inner: BlobRepoInner) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }
}

/// This function uploads bonsai changests object to blobstore in parallel, and then does
/// sequential writes to changesets table. Parents of the changesets should already by saved
/// in the repository.
pub async fn save_bonsai_changesets(
    bonsai_changesets: Vec<BonsaiChangeset>,
    ctx: CoreContext,
    repo: BlobRepo,
) -> Result<(), Error> {
    let complete_changesets = repo.get_changesets_object();
    let blobstore = repo.get_blobstore();
    let repoid = repo.get_repoid();

    let mut parents_to_check: HashSet<ChangesetId> = HashSet::new();
    for bcs in &bonsai_changesets {
        parents_to_check.extend(bcs.parents());
    }
    // Remove commits that we are uploading in this batch
    for bcs in &bonsai_changesets {
        parents_to_check.remove(&bcs.get_changeset_id());
    }

    let parents_to_check = parents_to_check
        .into_iter()
        .map({
            |p| {
                cloned!(ctx, repo);
                async move {
                    let exists = repo.changeset_exists_by_bonsai(ctx, p).await?;
                    if exists {
                        Ok(())
                    } else {
                        Err(format_err!("Commit {} does not exist in the repo", p))
                    }
                }
            }
        })
        .collect::<FuturesUnordered<_>>()
        .try_collect::<Vec<_>>();

    let bonsai_changesets: HashMap<_, _> = bonsai_changesets
        .into_iter()
        .map(|bcs| (bcs.get_changeset_id(), bcs))
        .collect();

    // Order of inserting entries in changeset table matters though, so we first need to
    // topologically sort commits.
    let mut bcs_parents = HashMap::new();
    for bcs in bonsai_changesets.values() {
        let parents: Vec<_> = bcs.parents().collect();
        bcs_parents.insert(bcs.get_changeset_id(), parents);
    }

    let topo_sorted_commits = sort_topological(&bcs_parents).expect("loop in commit chain!");
    let mut bonsai_complete_futs = vec![];
    for bcs_id in topo_sorted_commits {
        if let Some(bcs) = bonsai_changesets.get(&bcs_id) {
            let bcs_id = bcs.get_changeset_id();
            let completion_record = ChangesetInsert {
                repo_id: repoid,
                cs_id: bcs_id,
                parents: bcs.parents().into_iter().collect(),
            };
            bonsai_complete_futs.push(complete_changesets.add(ctx.clone(), completion_record));
        }
    }

    // Order of inserting bonsai changesets objects doesn't matter, so we can join them
    let bonsai_objects = bonsai_changesets
        .into_iter()
        .map({
            |(_, bcs)| {
                cloned!(ctx, blobstore);
                async move {
                    let bonsai_blob = bcs.into_blob();
                    let bcs_id = bonsai_blob.id().clone();
                    let blobstore_key = bcs_id.blobstore_key();
                    blobstore
                        .put(&ctx, blobstore_key, bonsai_blob.into())
                        .await?;
                    Ok(())
                }
            }
        })
        .collect::<FuturesUnordered<_>>()
        .try_collect::<Vec<_>>();

    try_join(bonsai_objects, parents_to_check).await?;

    for bonsai_complete in bonsai_complete_futs {
        bonsai_complete.await?;
    }

    Ok(())
}
