/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobstore::Blobstore;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::{BonsaiGlobalrevMapping, BonsaisOrGlobalrevs};
use bookmarks::{
    self, Bookmark, BookmarkKind, BookmarkName, BookmarkPagination, BookmarkPrefix,
    BookmarkTransaction, BookmarkUpdateLog, BookmarkUpdateLogEntry, BookmarkUpdateReason,
    Bookmarks, Freshness,
};
use cacheblob::LeaseOps;
use changeset_fetcher::ChangesetFetcher;
use changesets::{ChangesetInsert, Changesets};
use cloned::cloned;
use context::CoreContext;
use filestore::FilestoreConfig;
use futures::{
    future::{try_join, BoxFuture},
    stream::FuturesUnordered,
    FutureExt, Stream, TryStreamExt,
};
use metaconfig_types::DerivedDataConfig;
use mononoke_types::{
    BlobstoreValue, BonsaiChangeset, ChangesetId, Generation, Globalrev, MononokeId, RepositoryId,
    Timestamp,
};
use phases::{HeadsFetcher, Phases, SqlPhasesFactory};
use repo_blobstore::{RepoBlobstore, RepoBlobstoreArgs};
use stats::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use topo_sort::sort_topological;
use type_map::TypeMap;

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
#[derive(Clone)]
pub struct BlobRepoInner {
    pub blobstore: RepoBlobstore,
    pub changesets: Arc<dyn Changesets>,
    pub bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
    pub bonsai_globalrev_mapping: Arc<dyn BonsaiGlobalrevMapping>,
    pub repoid: RepositoryId,
    pub derived_data_lease: Arc<dyn LeaseOps>,
    pub filestore_config: FilestoreConfig,
    pub phases_factory: SqlPhasesFactory,
    pub derived_data_config: DerivedDataConfig,
    pub reponame: String,
    pub attributes: Arc<TypeMap>,
}

#[derive(Clone)]
pub struct BlobRepo {
    inner: Arc<BlobRepoInner>,
}

impl BlobRepoInner {
    pub fn attribute<T: ?Sized + Send + Sync + 'static>(&self) -> Option<&Arc<T>> {
        self.attributes.get::<T>()
    }

    pub fn attribute_expected<T: ?Sized + Send + Sync + 'static>(&self) -> &Arc<T> {
        self.attributes.get::<T>().unwrap_or_else(|| {
            panic!(
                "BlobRepo initialized incorrectly and does not have {} attribute",
                std::any::type_name::<T>()
            )
        })
    }
}

impl BlobRepo {
    /// Create new `BlobRepo` object.
    ///
    /// Avoid using this constructor directly as it requires properly initialized `attributes`
    /// argument. Instead use `blobrepo_factory::*` functions.
    pub fn new_dangerous(
        blobstore_args: RepoBlobstoreArgs,
        changesets: Arc<dyn Changesets>,
        bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
        bonsai_globalrev_mapping: Arc<dyn BonsaiGlobalrevMapping>,
        derived_data_lease: Arc<dyn LeaseOps>,
        filestore_config: FilestoreConfig,
        phases_factory: SqlPhasesFactory,
        derived_data_config: DerivedDataConfig,
        reponame: String,
        attributes: Arc<TypeMap>,
    ) -> Self {
        let (blobstore, repoid) = blobstore_args.into_blobrepo_parts();

        let inner = BlobRepoInner {
            blobstore,
            changesets,
            bonsai_git_mapping,
            bonsai_globalrev_mapping,
            repoid,
            derived_data_lease,
            filestore_config,
            phases_factory,
            derived_data_config,
            reponame,
            attributes,
        };
        BlobRepo {
            inner: Arc::new(inner),
        }
    }

    #[inline]
    pub fn attribute<T: ?Sized + Send + Sync + 'static>(&self) -> Option<&Arc<T>> {
        self.inner.attribute::<T>()
    }

    #[inline]
    pub fn attribute_expected<T: ?Sized + Send + Sync + 'static>(&self) -> &Arc<T> {
        self.inner.attribute_expected::<T>()
    }

    /// Get Bonsai changesets for Mercurial heads, which we approximate as Publishing Bonsai
    /// Bookmarks. Those will be served from cache, so they might be stale.
    pub fn get_bonsai_heads_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = Result<ChangesetId, Error>> {
        STATS::get_bonsai_heads_maybe_stale.add_value(1);
        self.attribute_expected::<dyn Bookmarks>()
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
        self.attribute_expected::<dyn Bookmarks>().list(
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
        self.attribute_expected::<dyn Bookmarks>().list(
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
        self.attribute_expected::<dyn Bookmarks>().get(ctx, name)
    }

    pub fn bonsai_git_mapping(&self) -> &Arc<dyn BonsaiGitMapping> {
        &self.inner.bonsai_git_mapping
    }

    pub fn bonsai_globalrev_mapping(&self) -> &Arc<dyn BonsaiGlobalrevMapping> {
        &self.inner.bonsai_globalrev_mapping
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

    pub fn list_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        name: BookmarkName,
        max_rec: u32,
        offset: Option<u32>,
        freshness: Freshness,
    ) -> impl Stream<Item = Result<(u64, Option<ChangesetId>, BookmarkUpdateReason, Timestamp), Error>>
    {
        self.attribute_expected::<dyn BookmarkUpdateLog>()
            .list_bookmark_log_entries(ctx.clone(), name, max_rec, offset, freshness)
    }

    pub fn read_next_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        limit: u64,
        freshness: Freshness,
    ) -> impl Stream<Item = Result<BookmarkUpdateLogEntry, Error>> {
        self.attribute_expected::<dyn BookmarkUpdateLog>()
            .read_next_bookmark_log_entries(ctx, id, limit, freshness)
    }

    pub async fn count_further_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        exclude_reason: Option<BookmarkUpdateReason>,
    ) -> Result<u64, Error> {
        self.attribute_expected::<dyn BookmarkUpdateLog>()
            .count_further_bookmark_log_entries(ctx, id, exclude_reason)
            .await
    }

    pub fn update_bookmark_transaction(&self, ctx: CoreContext) -> Box<dyn BookmarkTransaction> {
        STATS::update_bookmark_transaction.add_value(1);
        self.attribute_expected::<dyn Bookmarks>()
            .create_transaction(ctx)
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
        self.attribute_expected::<dyn ChangesetFetcher>().clone()
    }

    pub fn blobstore(&self) -> &RepoBlobstore {
        &self.inner.blobstore
    }

    pub fn get_blobstore(&self) -> RepoBlobstore {
        self.inner.blobstore.clone()
    }

    pub fn filestore_config(&self) -> FilestoreConfig {
        self.inner.filestore_config
    }

    pub fn get_repoid(&self) -> RepositoryId {
        self.inner.repoid
    }

    pub fn get_phases(&self) -> Arc<dyn Phases> {
        self.inner.phases_factory.get_phases(
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

    pub fn bookmarks(&self) -> Arc<dyn Bookmarks> {
        self.attribute_expected::<dyn Bookmarks>().clone()
    }

    pub fn get_phases_factory(&self) -> &SqlPhasesFactory {
        &self.inner.phases_factory
    }

    pub fn get_changesets_object(&self) -> Arc<dyn Changesets> {
        self.inner.changesets.clone()
    }

    pub fn get_derived_data_config(&self) -> &DerivedDataConfig {
        &self.inner.derived_data_config
    }

    pub fn get_derived_data_lease_ops(&self) -> Arc<dyn LeaseOps> {
        self.inner.derived_data_lease.clone()
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
