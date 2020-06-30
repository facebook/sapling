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
    self, Bookmark, BookmarkName, BookmarkPrefix, BookmarkUpdateLogEntry, BookmarkUpdateReason,
    Bookmarks, Freshness,
};
use cacheblob::LeaseOps;
use changeset_fetcher::{ChangesetFetcher, SimpleChangesetFetcher};
use changesets::{ChangesetInsert, Changesets};
use cloned::cloned;
use context::CoreContext;
use filestore::FilestoreConfig;
use futures::compat::Future01CompatExt;
use futures::future::{FutureExt as NewFutureExt, TryFutureExt};
use futures::stream::TryStreamExt;
use futures_ext::{BoxFuture, FutureExt};
use futures_old::future::{loop_fn, ok, Future, Loop};
use futures_old::stream::{self, FuturesUnordered, Stream};
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
    pub bookmarks: Arc<dyn Bookmarks>,
    pub changesets: Arc<dyn Changesets>,
    pub bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
    pub bonsai_globalrev_mapping: Arc<dyn BonsaiGlobalrevMapping>,
    pub repoid: RepositoryId,
    // Returns new ChangesetFetcher that can be used by operation that work with commit graph
    // (for example, revsets).
    pub changeset_fetcher_factory:
        Arc<dyn Fn() -> Arc<dyn ChangesetFetcher + Send + Sync> + Send + Sync>,
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

impl BlobRepo {
    /// Create new `BlobRepo` object.
    ///
    /// Avoid using this constructor directly as it requires properly initialized `attributes`
    /// argument. Instead use `blobrepo_factory::*` functions.
    pub fn new_dangerous(
        bookmarks: Arc<dyn Bookmarks>,
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

        let changeset_fetcher_factory = {
            cloned!(changesets, repoid);
            move || {
                let res: Arc<dyn ChangesetFetcher + Send + Sync> = Arc::new(
                    SimpleChangesetFetcher::new(changesets.clone(), repoid.clone()),
                );
                res
            }
        };

        let inner = BlobRepoInner {
            bookmarks,
            blobstore,
            changesets,
            bonsai_git_mapping,
            bonsai_globalrev_mapping,
            repoid,
            changeset_fetcher_factory: Arc::new(changeset_fetcher_factory),
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

    pub fn get_attribute<T: ?Sized + Send + Sync + 'static>(&self) -> Option<&Arc<T>> {
        self.inner.attributes.get::<T>()
    }

    /// Get Bonsai changesets for Mercurial heads, which we approximate as Publishing Bonsai
    /// Bookmarks. Those will be served from cache, so they might be stale.
    pub fn get_bonsai_heads_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = ChangesetId, Error = Error> {
        STATS::get_bonsai_heads_maybe_stale.add_value(1);
        self.inner
            .bookmarks
            .list_publishing_by_prefix(
                ctx,
                &BookmarkPrefix::empty(),
                self.get_repoid(),
                Freshness::MaybeStale,
            )
            .map_ok(|(_, cs_id)| cs_id)
            .compat()
    }

    /// List all publishing Bonsai bookmarks.
    pub fn get_bonsai_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = (Bookmark, ChangesetId), Error = Error> {
        STATS::get_bonsai_publishing_bookmarks_maybe_stale.add_value(1);
        self.inner
            .bookmarks
            .list_publishing_by_prefix(
                ctx,
                &BookmarkPrefix::empty(),
                self.get_repoid(),
                Freshness::MaybeStale,
            )
            .compat()
    }

    /// Get bookmarks by prefix, they will be read from replica, so they might be stale.
    pub fn get_bonsai_bookmarks_by_prefix_maybe_stale(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        max: u64,
    ) -> impl Stream<Item = (Bookmark, ChangesetId), Error = Error> {
        STATS::get_bookmarks_by_prefix_maybe_stale.add_value(1);
        self.inner
            .bookmarks
            .list_all_by_prefix(
                ctx.clone(),
                prefix,
                self.get_repoid(),
                Freshness::MaybeStale,
                max,
            )
            .compat()
    }

    pub fn changeset_exists_by_bonsai(
        &self,
        ctx: CoreContext,
        changesetid: ChangesetId,
    ) -> BoxFuture<bool, Error> {
        STATS::changeset_exists_by_bonsai.add_value(1);
        self.inner
            .changesets
            .get(ctx, self.get_repoid(), changesetid)
            .map(|res| res.is_some())
            .boxify()
    }

    pub fn get_changeset_parents_by_bonsai(
        &self,
        ctx: CoreContext,
        changesetid: ChangesetId,
    ) -> impl Future<Item = Vec<ChangesetId>, Error = Error> {
        STATS::get_changeset_parents_by_bonsai.add_value(1);
        self.inner
            .changesets
            .get(ctx, self.get_repoid(), changesetid)
            .and_then(move |maybe_bonsai| {
                maybe_bonsai
                    .ok_or_else(|| format_err!("Commit {} does not exist in the repo", changesetid))
            })
            .map(|bonsai| bonsai.parents)
    }

    pub fn get_bonsai_bookmark(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        STATS::get_bookmark.add_value(1);
        self.inner
            .bookmarks
            .get(ctx, name, self.get_repoid())
            .compat()
            .boxify()
    }

    pub fn bonsai_git_mapping(&self) -> &Arc<dyn BonsaiGitMapping> {
        &self.inner.bonsai_git_mapping
    }

    pub fn bonsai_globalrev_mapping(&self) -> &Arc<dyn BonsaiGlobalrevMapping> {
        &self.inner.bonsai_globalrev_mapping
    }

    pub fn get_bonsai_from_globalrev(
        &self,
        globalrev: Globalrev,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        self.inner
            .bonsai_globalrev_mapping
            .get_bonsai_from_globalrev(self.get_repoid(), globalrev)
    }

    pub fn get_globalrev_from_bonsai(
        &self,
        bcs: ChangesetId,
    ) -> BoxFuture<Option<Globalrev>, Error> {
        self.inner
            .bonsai_globalrev_mapping
            .get_globalrev_from_bonsai(self.get_repoid(), bcs)
    }

    pub fn get_bonsai_globalrev_mapping(
        &self,
        bonsai_or_globalrev_ids: impl Into<BonsaisOrGlobalrevs>,
    ) -> BoxFuture<Vec<(ChangesetId, Globalrev)>, Error> {
        self.inner
            .bonsai_globalrev_mapping
            .get(self.get_repoid(), bonsai_or_globalrev_ids.into())
            .map(|result| {
                result
                    .into_iter()
                    .map(|entry| (entry.bcs_id, entry.globalrev))
                    .collect()
            })
            .boxify()
    }

    pub fn list_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        name: BookmarkName,
        max_rec: u32,
        offset: Option<u32>,
        freshness: Freshness,
    ) -> impl Stream<Item = (Option<ChangesetId>, BookmarkUpdateReason, Timestamp), Error = Error>
    {
        self.inner
            .bookmarks
            .list_bookmark_log_entries(
                ctx.clone(),
                name,
                self.get_repoid(),
                max_rec,
                offset,
                freshness,
            )
            .compat()
    }

    pub fn read_next_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        limit: u64,
        freshness: Freshness,
    ) -> impl Stream<Item = BookmarkUpdateLogEntry, Error = Error> {
        self.inner
            .bookmarks
            .read_next_bookmark_log_entries(ctx, id, self.get_repoid(), limit, freshness)
            .compat()
    }

    pub fn count_further_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        id: u64,
        exclude_reason: Option<BookmarkUpdateReason>,
    ) -> impl Future<Item = u64, Error = Error> {
        self.inner
            .bookmarks
            .count_further_bookmark_log_entries(ctx, id, self.get_repoid(), exclude_reason)
            .compat()
    }

    pub fn update_bookmark_transaction(&self, ctx: CoreContext) -> Box<dyn bookmarks::Transaction> {
        STATS::update_bookmark_transaction.add_value(1);
        self.inner
            .bookmarks
            .create_transaction(ctx, self.get_repoid())
    }

    // Returns the generation number of a changeset
    // note: it returns Option because changeset might not exist
    pub fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs: ChangesetId,
    ) -> impl Future<Item = Option<Generation>, Error = Error> {
        STATS::get_generation_number.add_value(1);
        self.inner
            .changesets
            .get(ctx, self.get_repoid(), cs)
            .map(|res| res.map(|res| Generation::new(res.gen)))
    }

    pub fn get_changeset_fetcher(&self) -> Arc<dyn ChangesetFetcher> {
        (self.inner.changeset_fetcher_factory)()
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
                .collect()
                .compat()
                .boxed()
        })
    }

    pub fn get_bookmarks_object(&self) -> Arc<dyn Bookmarks> {
        self.inner.bookmarks.clone()
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
pub fn save_bonsai_changesets(
    bonsai_changesets: Vec<BonsaiChangeset>,
    ctx: CoreContext,
    repo: BlobRepo,
) -> impl Future<Item = (), Error = Error> {
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

    let parents_to_check = stream::futures_unordered(parents_to_check.into_iter().map({
        cloned!(ctx, repo);
        move |p| {
            repo.changeset_exists_by_bonsai(ctx.clone(), p)
                .and_then(move |exists| {
                    if exists {
                        Ok(())
                    } else {
                        Err(format_err!("Commit {} does not exist in the repo", p))
                    }
                })
        }
    }))
    .collect();

    let bonsai_changesets: HashMap<_, _> = bonsai_changesets
        .into_iter()
        .map(|bcs| (bcs.get_changeset_id(), bcs))
        .collect();

    // Order of inserting bonsai changesets objects doesn't matter, so we can join them
    let mut bonsai_object_futs = FuturesUnordered::new();
    for bcs in bonsai_changesets.values() {
        bonsai_object_futs.push(save_bonsai_changeset_object(
            ctx.clone(),
            blobstore.clone(),
            bcs.clone(),
        ));
    }
    let bonsai_objects = bonsai_object_futs.collect();
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

    bonsai_objects
        .join(parents_to_check)
        .and_then(move |_| {
            loop_fn(
                bonsai_complete_futs.into_iter(),
                move |mut futs| match futs.next() {
                    Some(fut) => fut
                        .and_then(move |_| ok(Loop::Continue(futs)))
                        .left_future(),
                    None => ok(Loop::Break(())).right_future(),
                },
            )
        })
        .and_then(|_| ok(()))
}

pub fn save_bonsai_changeset_object(
    ctx: CoreContext,
    blobstore: RepoBlobstore,
    bonsai_cs: BonsaiChangeset,
) -> impl Future<Item = (), Error = Error> {
    let bonsai_blob = bonsai_cs.into_blob();
    let bcs_id = bonsai_blob.id().clone();
    let blobstore_key = bcs_id.blobstore_key();

    blobstore
        .put(ctx, blobstore_key, bonsai_blob.into())
        .compat()
        .map(|_| ())
}
