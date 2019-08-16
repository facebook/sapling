// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::utils::{IncompleteFilenodeInfo, IncompleteFilenodes, UnittestOverride};
use crate::bonsai_generation::{create_bonsai_changeset_object, save_bonsai_changeset_object};
use crate::derive_hg_manifest::derive_hg_manifest;
use crate::envelope::HgBlobEnvelope;
use crate::errors::*;
use crate::file::{
    fetch_file_content_from_blobstore, fetch_file_content_id_from_blobstore,
    fetch_file_content_sha256_from_blobstore, fetch_file_contents, fetch_file_envelope,
    fetch_file_metadata_from_blobstore, fetch_file_parents_from_blobstore,
    fetch_file_size_from_blobstore, HgBlobEntry,
};
use crate::filenode_lookup::{lookup_filenode_id, store_filenode_id, FileNodeIdPointer};
use crate::repo_commit::*;
use crate::{BlobManifest, HgBlobChangeset};
use blob_changeset::{ChangesetMetadata, HgChangesetContent};
use blobstore::{Blobstore, Loadable, Storable};
use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiHgMappingEntry, BonsaiOrHgChangesetIds};
use bookmarks::{
    self, Bookmark, BookmarkName, BookmarkPrefix, BookmarkUpdateReason, Bookmarks, Freshness,
};
use bytes::Bytes;
use cacheblob::{LeaseOps, MemWritesBlobstore};
use changeset_fetcher::{ChangesetFetcher, SimpleChangesetFetcher};
use changesets::{ChangesetEntry, ChangesetInsert, Changesets};
use cloned::cloned;
use context::CoreContext;
use failure_ext::{bail_err, prelude::*, Error, FutureFailureErrorExt, FutureFailureExt, Result};
use filenodes::{FilenodeInfo, Filenodes};
use filestore::{self, Alias, FetchKey, FilestoreConfig, StoreRequest};
use futures::future::{self, loop_fn, ok, Either, Future, Loop};
use futures::stream::{self, once, FuturesUnordered, Stream};
use futures::sync::oneshot;
use futures::IntoFuture;
use futures_ext::{
    bounded_traversal::bounded_traversal, spawn_future, try_boxfuture, BoxFuture, BoxStream,
    FutureExt, StreamExt,
};
use futures_stats::{FutureStats, Timed};
use lock_ext::LockExt;
use maplit::hashmap;
use mercurial_revlog::file::{File, META_SZ};
use mercurial_types::manifest::Content;
use mercurial_types::{
    calculate_hg_node_id_stream, Changeset, Entry, FileBytes, HgBlobNode, HgChangesetId, HgEntryId,
    HgFileEnvelope, HgFileEnvelopeMut, HgFileNodeId, HgManifestEnvelopeMut, HgManifestId,
    HgNodeHash, HgParents, Manifest, RepoPath, Type,
};
use mononoke_types::{
    hash::Sha256, Blob, BlobstoreBytes, BlobstoreValue, BonsaiChangeset, ChangesetId, ContentId,
    ContentMetadata, FileChange, FileType, Generation, MPath, MPathElement, MononokeId,
    RepositoryId, Timestamp,
};
use repo_blobstore::{RepoBlobstore, RepoBlobstoreArgs};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::{trace, Logger};
use stats::{define_stats, Histogram, Timeseries};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    convert::From,
    mem,
    sync::{Arc, Mutex},
};
use time_ext::DurationExt;
use topo_sort::sort_topological;
use tracing::{trace_args, EventId, Traced};
use uuid::Uuid;

define_stats! {
    prefix = "mononoke.blobrepo";
    get_bonsai_changeset: timeseries(RATE, SUM),
    get_bonsai_heads_maybe_stale: timeseries(RATE, SUM),
    get_bonsai_publishing_bookmarks_maybe_stale: timeseries(RATE, SUM),
    get_file_content: timeseries(RATE, SUM),
    get_raw_hg_content: timeseries(RATE, SUM),
    get_changesets: timeseries(RATE, SUM),
    get_heads_maybe_stale: timeseries(RATE, SUM),
    changeset_exists: timeseries(RATE, SUM),
    changeset_exists_by_bonsai: timeseries(RATE, SUM),
    many_changesets_exists: timeseries(RATE, SUM),
    get_changeset_parents: timeseries(RATE, SUM),
    get_changeset_parents_by_bonsai: timeseries(RATE, SUM),
    get_changeset_by_changesetid: timeseries(RATE, SUM),
    get_hg_file_copy_from_blobstore: timeseries(RATE, SUM),
    get_hg_from_bonsai_changeset: timeseries(RATE, SUM),
    generate_hg_from_bonsai_changeset: timeseries(RATE, SUM),
    generate_hg_from_bonsai_total_latency_ms: histogram(100, 0, 10_000, AVG; P 50; P 75; P 90; P 95; P 99),
    generate_hg_from_bonsai_single_latency_ms: histogram(100, 0, 10_000, AVG; P 50; P 75; P 90; P 95; P 99),
    generate_hg_from_bonsai_generated_commit_num: histogram(1, 0, 20, AVG; P 50; P 75; P 90; P 95; P 99),
    get_manifest_by_nodeid: timeseries(RATE, SUM),
    get_root_entry: timeseries(RATE, SUM),
    get_bookmark: timeseries(RATE, SUM),
    get_bookmarks_by_prefix_maybe_stale: timeseries(RATE, SUM),
    get_publishing_bookmarks_maybe_stale: timeseries(RATE, SUM),
    get_pull_default_bookmarks_maybe_stale: timeseries(RATE, SUM),
    get_bonsai_from_hg: timeseries(RATE, SUM),
    get_hg_bonsai_mapping: timeseries(RATE, SUM),
    update_bookmark_transaction: timeseries(RATE, SUM),
    get_linknode: timeseries(RATE, SUM),
    get_linknode_opt: timeseries(RATE, SUM),
    get_all_filenodes: timeseries(RATE, SUM),
    get_generation_number: timeseries(RATE, SUM),
    get_generation_number_by_bonsai: timeseries(RATE, SUM),
    upload_blob: timeseries(RATE, SUM),
    upload_hg_file_entry: timeseries(RATE, SUM),
    upload_hg_tree_entry: timeseries(RATE, SUM),
    create_changeset: timeseries(RATE, SUM),
    create_changeset_compute_cf: timeseries("create_changeset.compute_changed_files"; RATE, SUM),
    create_changeset_expected_cf: timeseries("create_changeset.expected_changed_files"; RATE, SUM),
    create_changeset_cf_count: timeseries("create_changeset.changed_files_count"; AVG, SUM),
}

pub struct BlobRepo {
    blobstore: RepoBlobstore,
    bookmarks: Arc<dyn Bookmarks>,
    filenodes: Arc<dyn Filenodes>,
    changesets: Arc<dyn Changesets>,
    bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    repoid: RepositoryId,
    // Returns new ChangesetFetcher that can be used by operation that work with commit graph
    // (for example, revsets).
    changeset_fetcher_factory:
        Arc<dyn Fn() -> Arc<dyn ChangesetFetcher + Send + Sync> + Send + Sync>,
    derived_data_lease: Arc<dyn LeaseOps>,
    filestore_config: FilestoreConfig,
}

impl BlobRepo {
    pub fn new(
        bookmarks: Arc<dyn Bookmarks>,
        blobstore_args: RepoBlobstoreArgs,
        filenodes: Arc<dyn Filenodes>,
        changesets: Arc<dyn Changesets>,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
        derived_data_lease: Arc<dyn LeaseOps>,
        filestore_config: FilestoreConfig,
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

        BlobRepo {
            bookmarks,
            blobstore,
            filenodes,
            changesets,
            bonsai_hg_mapping,
            repoid,
            changeset_fetcher_factory: Arc::new(changeset_fetcher_factory),
            derived_data_lease,
            filestore_config,
        }
    }

    pub fn new_with_changeset_fetcher_factory(
        bookmarks: Arc<dyn Bookmarks>,
        blobstore_args: RepoBlobstoreArgs,
        filenodes: Arc<dyn Filenodes>,
        changesets: Arc<dyn Changesets>,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
        changeset_fetcher_factory: Arc<
            dyn Fn() -> Arc<dyn ChangesetFetcher + Send + Sync> + Send + Sync,
        >,
        derived_data_lease: Arc<dyn LeaseOps>,
        filestore_config: FilestoreConfig,
    ) -> Self {
        let (blobstore, repoid) = blobstore_args.into_blobrepo_parts();
        BlobRepo {
            bookmarks,
            blobstore,
            filenodes,
            changesets,
            bonsai_hg_mapping,
            repoid,
            changeset_fetcher_factory,
            derived_data_lease,
            filestore_config,
        }
    }

    /// Convert this BlobRepo instance into one that only does writes in memory.
    ///
    /// ------------
    /// IMPORTANT!!!
    /// ------------
    /// Currently this applies to the blobstore *ONLY*. A future improvement would be to also
    /// do database writes in-memory.
    /// This function produces a blobrepo which DOES NOT HAVE ANY CENSORSHIP ENABLED
    #[allow(non_snake_case)]
    pub fn in_memory_writes_READ_DOC_COMMENT(self) -> BlobRepo {
        let BlobRepo {
            bookmarks,
            blobstore,
            filenodes,
            changesets,
            bonsai_hg_mapping,
            repoid,
            derived_data_lease,
            filestore_config,
            ..
        } = self;

        let repo_blobstore_args =
            RepoBlobstoreArgs::new_with_wrapped_inner_blobstore(blobstore, repoid, |blobstore| {
                Arc::new(MemWritesBlobstore::new(blobstore))
            });

        BlobRepo::new(
            bookmarks,
            repo_blobstore_args,
            filenodes,
            changesets,
            bonsai_hg_mapping,
            derived_data_lease,
            filestore_config,
        )
    }

    fn fetch<Id>(
        &self,
        ctx: CoreContext,
        id: Id,
    ) -> impl Future<Item = Id::Value, Error = Error> + Send
    where
        Id: MononokeId,
    {
        id.load(ctx, &self.blobstore).and_then(move |ret| {
            ret.ok_or(ErrorKind::MissingTypedKeyEntry(id.blobstore_key()).into())
        })
    }

    // this is supposed to be used only from unittest
    pub fn unittest_fetch<Id>(
        &self,
        ctx: CoreContext,
        id: Id,
    ) -> impl Future<Item = Id::Value, Error = Error> + Send
    where
        Id: MononokeId,
    {
        self.fetch(ctx, id)
    }

    fn store<K, V>(&self, ctx: CoreContext, value: V) -> impl Future<Item = K, Error = Error> + Send
    where
        V: BlobstoreValue<Key = K>,
        K: MononokeId<Value = V>,
    {
        value.into_blob().store(ctx, &self.blobstore)
    }

    // this is supposed to be used only from unittest
    pub fn unittest_store<K, V>(
        &self,
        ctx: CoreContext,
        value: V,
    ) -> impl Future<Item = K, Error = Error> + Send
    where
        V: BlobstoreValue<Key = K>,
        K: MononokeId<Value = V>,
    {
        self.store(ctx, value)
    }

    pub fn get_file_content(
        &self,
        ctx: CoreContext,
        key: HgFileNodeId,
    ) -> BoxStream<FileBytes, Error> {
        STATS::get_file_content.add_value(1);
        fetch_file_content_from_blobstore(ctx, &self.blobstore, key).boxify()
    }

    pub fn rechunk_file_by_content_id(
        &self,
        ctx: CoreContext,
        id: ContentId,
    ) -> impl Future<Item = ContentMetadata, Error = Error> {
        filestore::rechunk(
            self.blobstore.clone(),
            self.filestore_config.clone(),
            ctx,
            id,
        )
    }

    pub fn get_file_content_by_content_id(
        &self,
        ctx: CoreContext,
        id: ContentId,
    ) -> BoxStream<FileBytes, Error> {
        STATS::get_file_content.add_value(1);
        fetch_file_contents(ctx, &self.blobstore, id).boxify()
    }

    pub fn get_file_size(&self, ctx: CoreContext, key: HgFileNodeId) -> BoxFuture<u64, Error> {
        fetch_file_size_from_blobstore(ctx, &self.blobstore, key).boxify()
    }

    pub fn get_file_content_id(
        &self,
        ctx: CoreContext,
        key: HgFileNodeId,
    ) -> BoxFuture<ContentId, Error> {
        fetch_file_content_id_from_blobstore(ctx, &self.blobstore, key).boxify()
    }

    pub fn get_file_content_metadata(
        &self,
        ctx: CoreContext,
        key: ContentId,
    ) -> BoxFuture<ContentMetadata, Error> {
        fetch_file_metadata_from_blobstore(ctx, &self.blobstore, key).boxify()
    }

    pub fn get_file_content_id_by_sha256(
        &self,
        ctx: CoreContext,
        key: Sha256,
    ) -> BoxFuture<ContentId, Error> {
        FetchKey::Aliased(Alias::Sha256(key))
            .load(ctx, &self.blobstore)
            .and_then(move |content_id| {
                content_id.ok_or(ErrorKind::ContentBlobByAliasMissing(key).into())
            })
            .boxify()
    }

    pub fn get_file_parents(
        &self,
        ctx: CoreContext,
        key: HgFileNodeId,
    ) -> impl Future<Item = HgParents, Error = Error> {
        fetch_file_parents_from_blobstore(ctx, &self.blobstore, key)
    }

    pub fn get_file_sha256(
        &self,
        ctx: CoreContext,
        content_id: ContentId,
    ) -> BoxFuture<Sha256, Error> {
        fetch_file_content_sha256_from_blobstore(ctx, &self.blobstore, content_id).boxify()
    }

    pub fn get_file_content_by_alias(
        &self,
        ctx: CoreContext,
        sha256: Sha256,
    ) -> BoxStream<FileBytes, Error> {
        filestore::fetch(
            &self.blobstore,
            ctx,
            &FetchKey::Aliased(Alias::Sha256(sha256)),
        )
        .and_then(move |stream| stream.ok_or(ErrorKind::ContentBlobByAliasMissing(sha256).into()))
        .flatten_stream()
        .map(FileBytes)
        .boxify()
    }

    /// Get Mercurial heads, which we approximate as publishing Bonsai Bookmarks.
    pub fn get_heads_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = HgChangesetId, Error = Error> {
        STATS::get_heads_maybe_stale.add_value(1);
        self.get_bonsai_heads_maybe_stale(ctx.clone()).and_then({
            let repo = self.clone();
            move |cs| repo.get_hg_from_bonsai_changeset(ctx.clone(), cs)
        })
    }

    /// Get Bonsai changesets for Mercurial heads, which we approximate as Publishing Bonsai
    /// Bookmarks. Those will be served from cache, so they might be stale.
    pub fn get_bonsai_heads_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = ChangesetId, Error = Error> {
        STATS::get_bonsai_heads_maybe_stale.add_value(1);
        self.bookmarks
            .list_publishing_by_prefix(
                ctx,
                &BookmarkPrefix::empty(),
                self.repoid,
                Freshness::MaybeStale,
            )
            .map(|(_, cs_id)| cs_id)
    }

    /// List all publishing Bonsai bookmarks.
    pub fn get_bonsai_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = (Bookmark, ChangesetId), Error = Error> {
        STATS::get_bonsai_publishing_bookmarks_maybe_stale.add_value(1);
        self.bookmarks.list_publishing_by_prefix(
            ctx,
            &BookmarkPrefix::empty(),
            self.repoid,
            Freshness::MaybeStale,
        )
    }

    // TODO(stash): make it accept ChangesetId
    pub fn changeset_exists(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<bool, Error> {
        STATS::changeset_exists.add_value(1);
        let changesetid = changesetid.clone();
        let repo = self.clone();
        let repoid = self.repoid.clone();

        self.get_bonsai_from_hg(ctx.clone(), changesetid)
            .and_then(move |maybebonsai| match maybebonsai {
                Some(bonsai) => repo
                    .changesets
                    .get(ctx, repoid, bonsai)
                    .map(|res| res.is_some())
                    .left_future(),
                None => Ok(false).into_future().right_future(),
            })
            .boxify()
    }

    pub fn changeset_exists_by_bonsai(
        &self,
        ctx: CoreContext,
        changesetid: ChangesetId,
    ) -> BoxFuture<bool, Error> {
        STATS::changeset_exists_by_bonsai.add_value(1);
        let changesetid = changesetid.clone();
        let repo = self.clone();
        let repoid = self.repoid.clone();

        repo.changesets
            .get(ctx, repoid, changesetid)
            .map(|res| res.is_some())
            .boxify()
    }

    // TODO(stash): make it accept ChangesetId
    pub fn get_changeset_parents(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<Vec<HgChangesetId>, Error> {
        STATS::get_changeset_parents.add_value(1);
        let repo = self.clone();

        self.get_bonsai_cs_entry_or_fail(ctx.clone(), changesetid)
            .map(|bonsai| bonsai.parents)
            .and_then({
                cloned!(repo);
                move |bonsai_parents| {
                    future::join_all(bonsai_parents.into_iter().map(move |bonsai_parent| {
                        repo.get_hg_from_bonsai_changeset(ctx.clone(), bonsai_parent)
                    }))
                }
            })
            .boxify()
    }

    pub fn get_changeset_parents_by_bonsai(
        &self,
        ctx: CoreContext,
        changesetid: ChangesetId,
    ) -> impl Future<Item = Vec<ChangesetId>, Error = Error> {
        STATS::get_changeset_parents_by_bonsai.add_value(1);
        let repo = self.clone();
        let repoid = self.repoid.clone();

        repo.changesets
            .get(ctx, repoid, changesetid)
            .and_then(move |maybe_bonsai| {
                maybe_bonsai.ok_or(ErrorKind::BonsaiNotFound(changesetid).into())
            })
            .map(|bonsai| bonsai.parents)
    }

    fn get_bonsai_cs_entry_or_fail(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> impl Future<Item = ChangesetEntry, Error = Error> {
        let repoid = self.repoid.clone();
        let changesets = self.changesets.clone();

        self.get_bonsai_from_hg(ctx.clone(), changesetid)
            .and_then(move |maybebonsai| {
                maybebonsai.ok_or(ErrorKind::BonsaiMappingNotFound(changesetid).into())
            })
            .and_then(move |bonsai| {
                changesets
                    .get(ctx, repoid, bonsai)
                    .and_then(move |maybe_bonsai| {
                        maybe_bonsai.ok_or(ErrorKind::BonsaiNotFound(bonsai).into())
                    })
            })
    }

    pub fn get_changeset_by_changesetid(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<HgBlobChangeset, Error> {
        STATS::get_changeset_by_changesetid.add_value(1);
        HgBlobChangeset::load(ctx, &self.blobstore, changesetid)
            .and_then(move |cs| cs.ok_or(ErrorKind::ChangesetMissing(changesetid).into()))
            .boxify()
    }

    pub fn get_manifest_by_nodeid(
        &self,
        ctx: CoreContext,
        manifestid: HgManifestId,
    ) -> BoxFuture<Box<dyn Manifest + Sync>, Error> {
        STATS::get_manifest_by_nodeid.add_value(1);
        BlobManifest::load(ctx, &self.blobstore, manifestid)
            .and_then(move |mf| mf.ok_or(ErrorKind::ManifestMissing(manifestid).into()))
            .map(|m| m.boxed())
            .boxify()
    }

    pub fn get_content_by_entryid(
        &self,
        ctx: CoreContext,
        entry_id: HgEntryId,
    ) -> impl Future<Item = Content, Error = Error> {
        match entry_id {
            HgEntryId::File(file_type, filenode_id) => {
                let stream = self.get_file_content(ctx, filenode_id).boxify();
                let content = Content::new_file(file_type, stream);
                Ok(content).into_future().left_future()
            }
            HgEntryId::Manifest(manifest_id) => self
                .get_manifest_by_nodeid(ctx, manifest_id)
                .map(Content::Tree)
                .right_future(),
        }
    }

    pub fn get_root_entry(&self, manifestid: HgManifestId) -> HgBlobEntry {
        STATS::get_root_entry.add_value(1);
        HgBlobEntry::new_root(self.blobstore.clone(), manifestid)
    }

    pub fn get_bookmark(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
    ) -> BoxFuture<Option<HgChangesetId>, Error> {
        STATS::get_bookmark.add_value(1);
        self.bookmarks
            .get(ctx.clone(), name, self.repoid)
            .and_then({
                let repo = self.clone();
                move |cs_opt| match cs_opt {
                    None => future::ok(None).left_future(),
                    Some(cs) => repo
                        .get_hg_from_bonsai_changeset(ctx, cs)
                        .map(|cs| Some(cs))
                        .right_future(),
                }
            })
            .boxify()
    }

    pub fn get_bonsai_bookmark(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        STATS::get_bookmark.add_value(1);
        self.bookmarks.get(ctx, name, self.repoid)
    }

    pub fn list_bookmark_log_entries(
        &self,
        ctx: CoreContext,
        name: BookmarkName,
        max_rec: u32,
    ) -> impl Stream<Item = (Option<ChangesetId>, BookmarkUpdateReason, Timestamp), Error = Error>
    {
        self.bookmarks
            .list_bookmark_log_entries(ctx.clone(), name, self.repoid, max_rec)
    }

    /// Get Pull-Default (Pull-Default is a Mercurial concept) bookmarks by prefix, they will be
    /// read from cache or a replica, so they might be stale.
    pub fn get_pull_default_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = (Bookmark, HgChangesetId), Error = Error> {
        STATS::get_pull_default_bookmarks_maybe_stale.add_value(1);
        let stream = self.bookmarks.list_pull_default_by_prefix(
            ctx.clone(),
            &BookmarkPrefix::empty(),
            self.repoid,
            Freshness::MaybeStale,
        );
        to_hg_bookmark_stream(&self, &ctx, stream)
    }

    /// Get Publishing (Publishing is a Mercurial concept) bookmarks by prefix, they will be read
    /// from cache or a replica, so they might be stale.
    pub fn get_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> impl Stream<Item = (Bookmark, HgChangesetId), Error = Error> {
        STATS::get_publishing_bookmarks_maybe_stale.add_value(1);
        let stream = self.bookmarks.list_publishing_by_prefix(
            ctx.clone(),
            &BookmarkPrefix::empty(),
            self.repoid,
            Freshness::MaybeStale,
        );
        to_hg_bookmark_stream(&self, &ctx, stream)
    }

    /// Get bookmarks by prefix, they will be read from replica, so they might be stale.
    pub fn get_bookmarks_by_prefix_maybe_stale(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        max: u64,
    ) -> impl Stream<Item = (Bookmark, HgChangesetId), Error = Error> {
        STATS::get_bookmarks_by_prefix_maybe_stale.add_value(1);
        let stream = self.bookmarks.list_all_by_prefix(
            ctx.clone(),
            prefix,
            self.repoid,
            Freshness::MaybeStale,
            max,
        );
        to_hg_bookmark_stream(&self, &ctx, stream)
    }

    pub fn update_bookmark_transaction(&self, ctx: CoreContext) -> Box<dyn bookmarks::Transaction> {
        STATS::update_bookmark_transaction.add_value(1);
        self.bookmarks.create_transaction(ctx, self.repoid)
    }

    pub fn get_linknode_opt(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> impl Future<Item = Option<HgChangesetId>, Error = Error> {
        STATS::get_linknode_opt.add_value(1);
        self.get_filenode_opt(ctx, path, node)
            .map(|filenode_opt| filenode_opt.map(|filenode| filenode.linknode))
    }

    pub fn get_linknode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> impl Future<Item = HgChangesetId, Error = Error> {
        STATS::get_linknode.add_value(1);
        self.get_filenode(ctx, path, node)
            .map(|filenode| filenode.linknode)
    }

    pub fn get_filenode_opt(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> impl Future<Item = Option<FilenodeInfo>, Error = Error> {
        let path = path.clone();
        self.filenodes.get_filenode(ctx, &path, node, self.repoid)
    }

    pub fn get_filenode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> impl Future<Item = FilenodeInfo, Error = Error> {
        self.get_filenode_opt(ctx, path, node).and_then({
            cloned!(path);
            move |filenode| filenode.ok_or(ErrorKind::MissingFilenode(path, node).into())
        })
    }

    pub fn get_file_envelope(
        &self,
        ctx: CoreContext,
        node: HgFileNodeId,
    ) -> impl Future<Item = HgFileEnvelope, Error = Error> {
        let store = self.get_blobstore();
        fetch_file_envelope(ctx, &store, node)
    }

    pub fn get_filenode_from_envelope(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
        linknode: HgChangesetId,
    ) -> impl Future<Item = FilenodeInfo, Error = Error> {
        let store = self.get_blobstore();
        fetch_file_envelope(ctx, &store, node)
            .with_context({
                cloned!(path);
                move |_| format!("While fetching filenode for {} {}", path, node)
            })
            .from_err()
            .and_then({
                cloned!(path, linknode);
                move |envelope| {
                    let (p1, p2) = envelope.parents();
                    let copyfrom = envelope
                        .get_copy_info()
                        .with_context({
                            cloned!(path);
                            move |_| format!("While parsing copy information for {} {}", path, node)
                        })?
                        .map(|(path, node)| (RepoPath::FilePath(path), node));
                    Ok(FilenodeInfo {
                        path,
                        filenode: node,
                        p1,
                        p2,
                        copyfrom,
                        linknode,
                    })
                }
            })
    }

    pub fn get_all_filenodes_maybe_stale(
        &self,
        ctx: CoreContext,
        path: RepoPath,
    ) -> BoxFuture<Vec<FilenodeInfo>, Error> {
        STATS::get_all_filenodes.add_value(1);
        self.filenodes
            .get_all_filenodes_maybe_stale(ctx, &path, self.repoid)
    }

    pub fn get_bonsai_from_hg(
        &self,
        ctx: CoreContext,
        hg_cs_id: HgChangesetId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        STATS::get_bonsai_from_hg.add_value(1);
        self.bonsai_hg_mapping
            .get_bonsai_from_hg(ctx, self.repoid, hg_cs_id)
    }

    // Returns only the mapping for valid changests that are known to the server.
    // Result may not contain all the ids from the input.
    pub fn get_hg_bonsai_mapping(
        &self,
        ctx: CoreContext,
        bonsai_or_hg_cs_ids: impl Into<BonsaiOrHgChangesetIds>,
    ) -> BoxFuture<Vec<(HgChangesetId, ChangesetId)>, Error> {
        STATS::get_hg_bonsai_mapping.add_value(1);
        self.bonsai_hg_mapping
            .get(ctx, self.repoid, bonsai_or_hg_cs_ids.into())
            .map(|result| {
                result
                    .into_iter()
                    .map(|entry| (entry.hg_cs_id, entry.bcs_id))
                    .collect()
            })
            // TODO(stash, luk): T37303879 also need to check that entries exist in changeset table
            .boxify()
    }

    pub fn get_bonsai_changeset(
        &self,
        ctx: CoreContext,
        bonsai_cs_id: ChangesetId,
    ) -> BoxFuture<BonsaiChangeset, Error> {
        STATS::get_bonsai_changeset.add_value(1);
        self.fetch(ctx, bonsai_cs_id).boxify()
    }

    // TODO(stash): make it accept ChangesetId
    pub fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs: HgChangesetId,
    ) -> impl Future<Item = Option<Generation>, Error = Error> {
        STATS::get_generation_number.add_value(1);
        let repo = self.clone();
        let repoid = self.repoid.clone();

        self.get_bonsai_from_hg(ctx.clone(), cs)
            .and_then(move |maybebonsai| match maybebonsai {
                Some(bonsai) => repo
                    .changesets
                    .get(ctx, repoid, bonsai)
                    .map(|res| res.map(|res| Generation::new(res.gen)))
                    .left_future(),
                None => Ok(None).into_future().right_future(),
            })
    }

    // TODO(stash): rename to get_generation_number
    pub fn get_generation_number_by_bonsai(
        &self,
        ctx: CoreContext,
        cs: ChangesetId,
    ) -> impl Future<Item = Option<Generation>, Error = Error> {
        STATS::get_generation_number_by_bonsai.add_value(1);
        let repo = self.clone();
        let repoid = self.repoid.clone();
        repo.changesets
            .get(ctx, repoid, cs)
            .map(|res| res.map(|res| Generation::new(res.gen)))
    }

    pub fn get_changeset_fetcher(&self) -> Arc<dyn ChangesetFetcher> {
        (self.changeset_fetcher_factory)()
    }

    fn upload_blobstore_bytes(
        &self,
        ctx: CoreContext,
        key: String,
        contents: BlobstoreBytes,
    ) -> impl Future<Item = (), Error = Error> + Send {
        fn log_upload_stats(
            logger: Logger,
            blobstore_key: String,
            phase: &str,
            stats: FutureStats,
        ) {
            trace!(logger, "Upload blob stats";
                "phase" => String::from(phase),
                "blobstore_key" => blobstore_key,
                "poll_count" => stats.poll_count,
                "poll_time_us" => stats.poll_time.as_micros_unchecked(),
                "completion_time_us" => stats.completion_time.as_micros_unchecked(),
            );
        }

        self.blobstore
            .put(ctx.clone(), key.clone(), contents)
            .timed({
                let logger = ctx.logger().clone();
                move |stats, result| {
                    if result.is_ok() {
                        log_upload_stats(logger, key, "blob uploaded", stats)
                    }
                    Ok(())
                }
            })
    }

    // TODO: Should we get rid of this function? It's only used for test code and Bundle2 upload.
    pub fn upload_blob<Id>(
        &self,
        ctx: CoreContext,
        blob: Blob<Id>,
    ) -> impl Future<Item = Id, Error = Error> + Send
    where
        Id: MononokeId,
    {
        STATS::upload_blob.add_value(1);
        let id = blob.id().clone();
        let blobstore_key = id.blobstore_key();
        let blob_contents: BlobstoreBytes = blob.into();

        // Upload {blobstore_key: blob_contents}
        self.upload_blobstore_bytes(ctx, blobstore_key, blob_contents.clone())
            .map(move |_| id)
    }

    pub fn upload_file(
        &self,
        ctx: CoreContext,
        req: &StoreRequest,
        data: impl Stream<Item = Bytes, Error = Error>,
    ) -> impl Future<Item = ContentMetadata, Error = Error> {
        filestore::store(&self.blobstore, &self.filestore_config, ctx, req, data)
    }

    // This is used by tests
    pub fn get_blobstore(&self) -> RepoBlobstore {
        self.blobstore.clone()
    }

    pub fn get_repoid(&self) -> RepositoryId {
        self.repoid
    }

    pub fn get_filenodes(&self) -> Arc<dyn Filenodes> {
        self.filenodes.clone()
    }

    fn store_file_change(
        &self,
        ctx: CoreContext,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
        path: &MPath,
        change: &FileChange,
        copy_from: Option<(MPath, HgFileNodeId)>,
    ) -> impl Future<Item = (HgBlobEntry, Option<IncompleteFilenodeInfo>), Error = Error> + Send
    {
        assert!(change.copy_from().is_some() == copy_from.is_some());
        // we can reuse same HgFileNodeId if we have only one parent with same
        // file content but different type (Regular|Executable)
        match (p1, p2) {
            (Some(parent), None) | (None, Some(parent)) => {
                let store = self.get_blobstore();
                cloned!(ctx, change, path);
                fetch_file_envelope(ctx.clone(), &store, parent)
                    .map(move |parent_envelope| {
                        if parent_envelope.content_id() == change.content_id()
                            && change.copy_from().is_none()
                        {
                            Some((
                                HgBlobEntry::new(
                                    store,
                                    path.basename().clone(),
                                    parent.into_nodehash(),
                                    Type::File(change.file_type()),
                                ),
                                None,
                            ))
                        } else {
                            None
                        }
                    })
                    .right_future()
            }
            _ => future::ok(None).left_future(),
        }
        .and_then({
            let repo = self.clone();
            cloned!(path, change);
            move |maybe_entry| match maybe_entry {
                Some(entry) => future::ok(entry).left_future(),
                None => {
                    // Mercurial has complicated logic of finding file parents, especially
                    // if a file was also copied/moved.
                    // See mercurial/localrepo.py:_filecommit(). We have to replicate this
                    // logic in Mononoke.
                    // TODO(stash): T45618931 replicate all the cases from _filecommit()

                    let parents_fut = if let Some((ref copy_from_path, _)) = copy_from {
                        if copy_from_path != &path && p1.is_some() && p2.is_none() {
                            // This case can happen if a file existed in it's parent
                            // but it was copied over:
                            // ```
                            // echo 1 > 1 && echo 2 > 2 && hg ci -A -m first
                            // hg cp 2 1 --force && hg ci -m second
                            // # File '1' has both p1 and copy from.
                            // ```
                            // In that case Mercurial discards p1 i.e. `hg log` will
                            // use copy from revision as a parent. Arguably not the best
                            // decision, but we have to keep it.
                            ok((None, None)).left_future()
                        } else {
                            ok((p1, p2)).left_future()
                        }
                    } else if p1.is_none() {
                        ok((p2, None)).left_future()
                    } else if p2.is_some() {
                        crate::file_history::check_if_related(
                            ctx.clone(),
                            repo.clone(),
                            p1.unwrap(),
                            p2.unwrap(),
                            path.clone(),
                        )
                        .map(move |res| {
                            use crate::file_history::FilenodesRelatedResult::*;

                            match res {
                                Unrelated => (p1, p2),
                                FirstAncestorOfSecond => (p2, None),
                                SecondAncestorOfFirst => (p1, None),
                            }
                        })
                        .right_future()
                    } else {
                        ok((p1, p2)).left_future()
                    };

                    parents_fut
                        .and_then({
                            move |(p1, p2)| {
                                let upload_entry = UploadHgFileEntry {
                                    upload_node_id: UploadHgNodeHash::Generate,
                                    contents: UploadHgFileContents::ContentUploaded(
                                        ContentBlobMeta {
                                            id: change.content_id(),
                                            size: change.size(),
                                            copy_from: copy_from.clone(),
                                        },
                                    ),
                                    file_type: change.file_type(),
                                    p1,
                                    p2,
                                    path: path.clone(),
                                };
                                match upload_entry.upload(ctx, &repo) {
                                    Ok((_, upload_fut)) => upload_fut
                                        .map(move |(entry, _)| {
                                            let node_info = IncompleteFilenodeInfo {
                                                path: RepoPath::FilePath(path),
                                                filenode: HgFileNodeId::new(
                                                    entry.get_hash().into_nodehash(),
                                                ),
                                                p1,
                                                p2,
                                                copyfrom: copy_from
                                                    .map(|(p, h)| (RepoPath::FilePath(p), h)),
                                            };
                                            (entry, Some(node_info))
                                        })
                                        .left_future(),
                                    Err(err) => return future::err(err).right_future(),
                                }
                            }
                        })
                        .right_future()
                }
            }
        })
    }

    /// Check if adding a single path to manifest would cause case-conflict
    ///
    /// Implementation traverses manifest and checks if correspoinding path element is present,
    /// if path element is not present, it lowercases current path element and checks if it
    /// collides with any existing elements inside manifest. if so it also needs to check that
    /// child manifest contains this entry, because it might have been removed.
    pub fn check_case_conflict_in_manifest(
        &self,
        ctx: CoreContext,
        parent_mf_id: HgManifestId,
        child_mf_id: HgManifestId,
        path: MPath,
    ) -> impl Future<Item = bool, Error = Error> {
        let repo = self.clone();
        let child_mf_id = child_mf_id.clone();
        self.get_manifest_by_nodeid(ctx.clone(), parent_mf_id)
            .and_then(move |mf| {
                loop_fn(
                    (None, mf, path.into_iter()),
                    move |(cur_path, mf, mut elements): (Option<MPath>, _, _)| {
                        let element = match elements.next() {
                            None => return future::ok(Loop::Break(false)).boxify(),
                            Some(element) => element,
                        };

                        match mf.lookup(&element) {
                            Some(entry) => {
                                let cur_path = MPath::join_opt_element(cur_path.as_ref(), &element);
                                match entry.get_hash() {
                                    HgEntryId::File(..) => future::ok(Loop::Break(false)).boxify(),
                                    HgEntryId::Manifest(manifest_id) => repo
                                        .get_manifest_by_nodeid(ctx.clone(), manifest_id)
                                        .map(move |mf| {
                                            Loop::Continue((Some(cur_path), mf, elements))
                                        })
                                        .boxify(),
                                }
                            }
                            None => {
                                let element_utf8 = String::from_utf8(Vec::from(element.as_ref()));
                                let mut potential_conflicts = vec![];
                                // Find all entries in the manifests that can potentially be a conflict.
                                // Entry can potentially be a conflict if its lowercased version
                                // is the same as lowercased version of the current element

                                for entry in mf.list() {
                                    let basename = entry
                                        .get_name()
                                        .expect("Non-root entry has empty basename");
                                    let path =
                                        MPath::join_element_opt(cur_path.as_ref(), Some(basename));
                                    match (&element_utf8, std::str::from_utf8(basename.as_ref())) {
                                        (Ok(ref element), Ok(ref basename)) => {
                                            if basename.to_lowercase() == element.to_lowercase() {
                                                potential_conflicts.extend(path);
                                            }
                                        }
                                        _ => (),
                                    }
                                }

                                // For each potential conflict we need to check if it's present in
                                // child manifest. If it is, then we've got a conflict, otherwise
                                // this has been deleted and it's no longer a conflict.
                                repo.find_entries_in_manifest(
                                    ctx.clone(),
                                    child_mf_id,
                                    potential_conflicts,
                                )
                                .map(|entries| Loop::Break(!entries.is_empty()))
                                .boxify()
                            }
                        }
                    },
                )
            })
    }

    /// Find files in manifest
    ///
    /// This function correctly handles conflicting paths too.
    pub fn find_files_in_manifest(
        &self,
        ctx: CoreContext,
        manifest_id: HgManifestId,
        paths: impl IntoIterator<Item = MPath>,
    ) -> impl Future<Item = HashMap<MPath, HgFileNodeId>, Error = Error> {
        self.find_entries_in_manifest(ctx, manifest_id, paths)
            .map(|path_to_entry| {
                path_to_entry
                    .into_iter()
                    .filter_map(|(path, entry_id)| {
                        entry_id
                            .to_filenode()
                            .map(move |(_file_type, filenode_id)| (path, filenode_id))
                    })
                    .collect()
            })
    }

    /// Look up manifest entries for multiple paths.
    ///
    /// Given a list of paths and a root manifest ID, walk the tree and
    /// return the manifest entries corresponding to the specified paths.
    pub fn find_entries_in_manifest(
        &self,
        ctx: CoreContext,
        manifest_id: HgManifestId,
        paths: impl IntoIterator<Item = MPath>,
    ) -> impl Future<Item = HashMap<MPath, HgEntryId>, Error = Error> {
        self.query_manifest(ctx, manifest_id, paths, false)
    }

    /// Look up manifest entries for every component of multiple paths.
    ///
    /// Similar to `find_entries_in_manifest`, walks the manifest tree starting from
    /// the given root manifest ID, looking for the specified paths. Unlike
    /// `find_entries_in_manifest`, this method returns the manifest entry of every
    /// path component traversed. This is useful for situations where the client would
    /// like to cache these entries to avoid future roundtrips to the server.
    pub fn find_all_path_component_entries(
        &self,
        ctx: CoreContext,
        manifest_id: HgManifestId,
        paths: impl IntoIterator<Item = MPath>,
    ) -> impl Future<Item = HashMap<MPath, HgEntryId>, Error = Error> {
        self.query_manifest(ctx, manifest_id, paths, true)
    }

    /// Efficiently fetch manifest entries for multiple paths.
    ///
    /// This function correctly handles conflicting paths too.
    fn query_manifest(
        &self,
        ctx: CoreContext,
        manifest_id: HgManifestId,
        paths: impl IntoIterator<Item = MPath>,
        select_all_path_components: bool,
    ) -> impl Future<Item = HashMap<MPath, HgEntryId>, Error = Error> {
        // Note: `children` and `selected` fields are not exclusive, that is
        //       selected might be true and children is not empty.
        struct QueryTree {
            children: HashMap<MPathElement, QueryTree>,
            selected: bool,
        }

        impl QueryTree {
            fn new(selected: bool) -> Self {
                Self {
                    children: HashMap::new(),
                    selected,
                }
            }

            fn insert_path(&mut self, path: MPath, select_all: bool) {
                let mut node = path.into_iter().fold(self, |tree, element| {
                    tree.children
                        .entry(element)
                        .or_insert_with(|| QueryTree::new(select_all))
                });
                node.selected = true;
            }

            fn from_paths(paths: impl IntoIterator<Item = MPath>, select_all: bool) -> Self {
                let mut tree = Self::new(select_all);
                paths
                    .into_iter()
                    .for_each(|path| tree.insert_path(path, select_all));
                tree
            }
        }

        let output = Arc::new(Mutex::new(HashMap::new()));
        bounded_traversal(
            1024,
            (
                QueryTree::from_paths(paths, select_all_path_components),
                manifest_id,
                None,
            ),
            {
                let repo = self.clone();
                cloned!(output);
                move |(QueryTree { children, .. }, manifest_id, path)| {
                    cloned!(path, output);
                    repo.get_manifest_by_nodeid(ctx.clone(), manifest_id)
                        .map(move |manifest| {
                            let children = children
                                .into_iter()
                                .filter_map(|(element, child)| {
                                    let path = MPath::join_opt_element(path.as_ref(), &element);
                                    manifest.lookup(&element).and_then(|entry| {
                                        let entry_id = entry.get_hash();
                                        if child.selected {
                                            output.with(|output| {
                                                output.insert(path.clone(), entry_id)
                                            });
                                        }
                                        entry_id
                                            .to_manifest()
                                            .map(|manifest_id| (child, manifest_id, Some(path)))
                                    })
                                })
                                .collect::<Vec<_>>();
                            ((), children)
                        })
                }
            },
            |_, _| Ok(()),
        )
        .map(move |_| output.with(|output| mem::replace(output, HashMap::new())))
    }

    pub fn get_manifest_from_bonsai(
        &self,
        ctx: CoreContext,
        bcs: BonsaiChangeset,
        manifest_p1: Option<HgManifestId>,
        manifest_p2: Option<HgManifestId>,
    ) -> BoxFuture<(HgManifestId, IncompleteFilenodes), Error> {
        let repo = self.clone();
        let event_id = EventId::new();
        let incomplete_filenodes = IncompleteFilenodes::new();

        let (p1, p2) = {
            let mut parents = bcs.parents();
            let p1 = parents.next();
            let p2 = parents.next();
            assert!(
                parents.next().is_none(),
                "mercurial only supports two parents"
            );
            (p1, p2)
        };
        // paths *modified* by changeset or *copied from parents*
        let mut p1_paths = Vec::new();
        let mut p2_paths = Vec::new();
        for (path, file_change) in bcs.file_changes() {
            if let Some(file_change) = file_change {
                if let Some((copy_path, bcsid)) = file_change.copy_from() {
                    if Some(bcsid) == p1.as_ref() {
                        p1_paths.push(copy_path.clone());
                    }
                    if Some(bcsid) == p2.as_ref() {
                        p2_paths.push(copy_path.clone());
                    }
                };
                p1_paths.push(path.clone());
                p2_paths.push(path.clone());
            }
        }

        // TODO:
        // `derive_manifest` already provides parents for newly created files, so we
        // can remove **all** lookups to files from here, and only leave lookups for
        // files that were copied (i.e bonsai changes that contain `copy_path`)
        let store_file_changes = (
            manifest_p1
                .map(|manifest_p1| {
                    self.find_files_in_manifest(ctx.clone(), manifest_p1, p1_paths)
                        .left_future()
                })
                .unwrap_or_else(|| future::ok(HashMap::new()).right_future()),
            manifest_p2
                .map(|manifest_p2| {
                    self.find_files_in_manifest(ctx.clone(), manifest_p2, p2_paths)
                        .left_future()
                })
                .unwrap_or_else(|| future::ok(HashMap::new()).right_future()),
        )
            .into_future()
            .traced_with_id(
                &ctx.trace(),
                "generate_hg_manifest::traverse_parents",
                trace_args! {},
                event_id,
            )
            .and_then({
                cloned!(ctx, repo, incomplete_filenodes);
                move |(p1s, p2s)| {
                    let file_changes: Vec<_> = bcs
                        .file_changes()
                        .map(|(path, file_change)| (path.clone(), file_change.cloned()))
                        .collect();
                    stream::iter_ok(file_changes)
                        .map({
                            cloned!(ctx);
                            move |(path, file_change)| match file_change {
                                None => future::ok((path, None)).left_future(),
                                Some(file_change) => {
                                    let copy_from =
                                        file_change.copy_from().and_then(|(copy_path, bcsid)| {
                                            if Some(bcsid) == p1.as_ref() {
                                                p1s.get(copy_path)
                                                    .map(|id| (copy_path.clone(), *id))
                                            } else if Some(bcsid) == p2.as_ref() {
                                                p2s.get(copy_path)
                                                    .map(|id| (copy_path.clone(), *id))
                                            } else {
                                                None
                                            }
                                        });
                                    repo.store_file_change(
                                        ctx.clone(),
                                        p1s.get(&path).cloned(),
                                        p2s.get(&path).cloned(),
                                        &path,
                                        &file_change,
                                        copy_from,
                                    )
                                    .map({
                                        cloned!(incomplete_filenodes);
                                        move |(entry, node_infos)| {
                                            for node_info in node_infos {
                                                incomplete_filenodes.add(node_info);
                                            }
                                            (path, Some(entry))
                                        }
                                    })
                                    .right_future()
                                }
                            }
                        })
                        .buffer_unordered(100)
                        .collect()
                        .traced_with_id(
                            &ctx.trace(),
                            "generate_hg_manifest::store_file_changes",
                            trace_args! {},
                            event_id,
                        )
                }
            });

        let create_manifest = {
            cloned!(ctx, repo, incomplete_filenodes);
            move |changes| {
                derive_hg_manifest(
                    ctx.clone(),
                    repo.clone(),
                    incomplete_filenodes,
                    vec![manifest_p1, manifest_p2].into_iter().flatten(),
                    changes,
                )
                .traced_with_id(
                    &ctx.trace(),
                    "generate_hg_manifest::create_manifest",
                    trace_args! {},
                    event_id,
                )
            }
        };

        store_file_changes
            .and_then(create_manifest)
            .map({
                cloned!(incomplete_filenodes);
                move |manifest_id| (manifest_id, incomplete_filenodes)
            })
            .traced_with_id(
                &ctx.trace(),
                "generate_hg_manifest",
                trace_args! {},
                event_id,
            )
            .boxify()
    }

    pub fn get_hg_from_bonsai_changeset(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
    ) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
        STATS::get_hg_from_bonsai_changeset.add_value(1);
        self.get_hg_from_bonsai_changeset_with_impl(ctx, bcs_id)
            .map(|(hg_cs_id, generated_commit_num)| {
                STATS::generate_hg_from_bonsai_generated_commit_num
                    .add_value(generated_commit_num as i64);
                hg_cs_id
            })
            .timed(move |stats, _| {
                STATS::generate_hg_from_bonsai_total_latency_ms
                    .add_value(stats.completion_time.as_millis_unchecked() as i64);
                Ok(())
            })
    }

    pub fn get_derived_data_lease_ops(&self) -> Arc<dyn LeaseOps> {
        self.derived_data_lease.clone()
    }

    fn generate_lease_key(&self, bcs_id: &ChangesetId) -> String {
        let repoid = self.get_repoid();
        format!("repoid.{}.hg-changeset.{}", repoid.id(), bcs_id)
    }

    fn take_hg_generation_lease(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
    ) -> impl Future<Item = Option<HgChangesetId>, Error = Error> + Send {
        let key = self.generate_lease_key(&bcs_id);
        let repoid = self.get_repoid();

        cloned!(self.bonsai_hg_mapping, self.derived_data_lease);
        let repo = self.clone();

        loop_fn((), move |()| {
            cloned!(ctx, key);
            derived_data_lease
                .try_add_put_lease(&key)
                .or_else(|_| Ok(false))
                .and_then({
                    cloned!(bcs_id, bonsai_hg_mapping, derived_data_lease, repo);
                    move |leased| {
                        let maybe_hg_cs =
                            bonsai_hg_mapping.get_hg_from_bonsai(ctx.clone(), repoid, bcs_id);
                        if leased {
                            maybe_hg_cs
                                .and_then(move |maybe_hg_cs| match maybe_hg_cs {
                                    Some(hg_cs) => repo
                                        .release_hg_generation_lease(bcs_id, true)
                                        .then(move |_| Ok(Loop::Break(Some(hg_cs))))
                                        .left_future(),
                                    None => future::ok(Loop::Break(None)).right_future(),
                                })
                                .left_future()
                        } else {
                            maybe_hg_cs
                                .and_then(move |maybe_hg_cs_id| match maybe_hg_cs_id {
                                    Some(hg_cs_id) => {
                                        future::ok(Loop::Break(Some(hg_cs_id))).left_future()
                                    }
                                    None => derived_data_lease
                                        .wait_for_other_leases(&key)
                                        .then(|_| Ok(Loop::Continue(())))
                                        .right_future(),
                                })
                                .right_future()
                        }
                    }
                })
        })
    }

    fn release_hg_generation_lease(
        &self,
        bcs_id: ChangesetId,
        put_success: bool,
    ) -> impl Future<Item = (), Error = ()> + Send {
        let key = self.generate_lease_key(&bcs_id);
        self.derived_data_lease.release_lease(&key, put_success)
    }

    fn generate_hg_changeset(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
        bcs: BonsaiChangeset,
        parents: Vec<HgBlobChangeset>,
    ) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
        let mut parents = parents.into_iter();
        let p1 = parents.next();
        let p2 = parents.next();

        let p1_hash = p1.as_ref().map(|p1| p1.get_changeset_id());
        let p2_hash = p2.as_ref().map(|p2| p2.get_changeset_id());

        let mf_p1 = p1.map(|p| p.manifestid());
        let mf_p2 = p2.map(|p| p.manifestid());

        assert!(
            parents.next().is_none(),
            "more than 2 parents are not supported by hg"
        );
        let hg_parents = HgParents::new(
            p1_hash.map(|h| h.into_nodehash()),
            p2_hash.map(|h| h.into_nodehash()),
        );
        let repo = self.clone();
        repo.get_manifest_from_bonsai(ctx.clone(), bcs.clone(), mf_p1.clone(), mf_p2.clone())
            .and_then({
                cloned!(ctx, repo);
                move |(manifest_id, incomplete_filenodes)| {
                compute_changed_files(ctx, repo, manifest_id.clone(), mf_p1.as_ref(), mf_p2.as_ref())
                    .map(move |files| {
                        (manifest_id, incomplete_filenodes, hg_parents, files)
                    })

            }})
            // create changeset
            .and_then({
                cloned!(ctx, repo, bcs);
                move |(manifest_id, incomplete_filenodes, parents, files)| {
                    let metadata = ChangesetMetadata {
                        user: bcs.author().to_string(),
                        time: *bcs.author_date(),
                        extra: bcs.extra()
                            .map(|(k, v)| {
                                (k.as_bytes().to_vec(), v.to_vec())
                            })
                            .collect(),
                        comments: bcs.message().to_string(),
                    };
                    let content = HgChangesetContent::new_from_parts(
                        parents,
                        manifest_id,
                        metadata,
                        files,
                    );
                    let cs = try_boxfuture!(HgBlobChangeset::new(content));
                    let cs_id = cs.get_changeset_id();

                    cs.save(ctx.clone(), repo.blobstore.clone())
                        .and_then({
                            cloned!(ctx, repo);
                            move |_| incomplete_filenodes.upload(ctx, cs_id, &repo)
                        })
                        .and_then({
                            cloned!(ctx, repo);
                            move |_| repo.bonsai_hg_mapping.add(
                                ctx,
                                BonsaiHgMappingEntry {
                                    repo_id: repo.get_repoid(),
                                    hg_cs_id: cs_id,
                                    bcs_id,
                                },
                            )
                        })
                        .map(move |_| cs_id)
                        .boxify()
                }
            })
            .traced(
                &ctx.trace(),
                "generate_hg_chengeset",
                trace_args! {"changeset" => bcs_id.to_hex().to_string()},
            )
            .timed(move |stats, _| {
                STATS::generate_hg_from_bonsai_single_latency_ms
                    .add_value(stats.completion_time.as_millis_unchecked() as i64);
                Ok(())
            })
    }

    // Converts Bonsai changesets to hg changesets. It either fetches hg changeset id from
    // bonsai-hg mapping or it generates hg changeset and puts hg changeset id in bonsai-hg mapping.
    // Note that it generates parent hg changesets first.
    // This function takes care of making sure the same changeset is not generated at the same time
    // by taking leases. It also avoids using recursion to prevents stackoverflow
    pub fn get_hg_from_bonsai_changeset_with_impl(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
    ) -> impl Future<Item = (HgChangesetId, usize), Error = Error> + Send {
        // Finds parent bonsai commits which do not have corresponding hg changeset generated
        // Avoids using recursion
        fn find_toposorted_bonsai_cs_with_no_hg_cs_generated(
            ctx: CoreContext,
            repo: BlobRepo,
            bcs_id: ChangesetId,
            bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
        ) -> impl Future<Item = Vec<BonsaiChangeset>, Error = Error> {
            let mut queue = VecDeque::new();
            let mut visited: HashSet<ChangesetId> = HashSet::new();
            visited.insert(bcs_id);
            queue.push_back(bcs_id);

            let repoid = repo.repoid;
            loop_fn(
                (queue, vec![], visited),
                move |(mut queue, mut commits_to_generate, mut visited)| {
                    cloned!(ctx, repo);
                    match queue.pop_front() {
                        Some(bcs_id) => bonsai_hg_mapping
                            .get_hg_from_bonsai(ctx.clone(), repoid, bcs_id)
                            .and_then(move |maybe_hg| match maybe_hg {
                                Some(_hg_cs_id) => future::ok(Loop::Continue((
                                    queue,
                                    commits_to_generate,
                                    visited,
                                )))
                                .left_future(),
                                None => repo
                                    .fetch(ctx.clone(), bcs_id)
                                    .map(move |bcs| {
                                        commits_to_generate.push(bcs.clone());
                                        queue.extend(bcs.parents().filter(|p| visited.insert(*p)));
                                        Loop::Continue((queue, commits_to_generate, visited))
                                    })
                                    .right_future(),
                            })
                            .left_future(),
                        None => future::ok(Loop::Break(commits_to_generate)).right_future(),
                    }
                },
            )
            .map(|changesets| {
                let mut graph = hashmap! {};
                let mut id_to_bcs = hashmap! {};
                for cs in changesets {
                    graph.insert(cs.get_changeset_id(), cs.parents().collect());
                    id_to_bcs.insert(cs.get_changeset_id(), cs);
                }
                sort_topological(&graph)
                    .expect("commit graph has cycles!")
                    .into_iter()
                    .map(|cs_id| id_to_bcs.remove(&cs_id))
                    .filter_map(|x| x)
                    .collect()
            })
        }

        // Panics if changeset not found
        fn fetch_hg_changeset_from_mapping(
            ctx: CoreContext,
            repo: BlobRepo,
            bcs_id: ChangesetId,
        ) -> impl Future<Item = HgBlobChangeset, Error = Error> {
            let bonsai_hg_mapping = repo.bonsai_hg_mapping.clone();
            let repoid = repo.repoid;

            cloned!(repo);
            bonsai_hg_mapping
                .get_hg_from_bonsai(ctx.clone(), repoid, bcs_id)
                .and_then(move |maybe_hg| match maybe_hg {
                    Some(hg_cs_id) => repo.get_changeset_by_changesetid(ctx, hg_cs_id),
                    None => panic!("hg changeset must be generated already"),
                })
        }

        // Panics if parent hg changesets are not generated
        // Returns whether a commit was generated or not
        fn generate_single_hg_changeset(
            ctx: CoreContext,
            repo: BlobRepo,
            bcs: BonsaiChangeset,
        ) -> impl Future<Item = (HgChangesetId, bool), Error = Error> {
            let bcs_id = bcs.get_changeset_id();

            repo.take_hg_generation_lease(ctx.clone(), bcs_id.clone())
                .traced(
                    &ctx.trace(),
                    "create_hg_from_bonsai::wait_for_lease",
                    trace_args! {},
                )
                .and_then({
                    cloned!(ctx, repo);
                    move |maybe_hg_cs_id| {
                        match maybe_hg_cs_id {
                            Some(hg_cs_id) => future::ok((hg_cs_id, false)).left_future(),
                            None => {
                                // We have the lease
                                STATS::generate_hg_from_bonsai_changeset.add_value(1);

                                let mut hg_parents = vec![];
                                for p in bcs.parents() {
                                    hg_parents.push(fetch_hg_changeset_from_mapping(
                                        ctx.clone(),
                                        repo.clone(),
                                        p,
                                    ));
                                }

                                future::join_all(hg_parents)
                                    .and_then({
                                        cloned!(repo);
                                        move |hg_parents| {
                                            repo.generate_hg_changeset(ctx, bcs_id, bcs, hg_parents)
                                        }
                                    })
                                    .then(move |res| {
                                        repo.release_hg_generation_lease(bcs_id, res.is_ok())
                                            .then(move |_| res.map(|hg_cs_id| (hg_cs_id, true)))
                                    })
                                    .right_future()
                            }
                        }
                    }
                })
        }

        let repo = self.clone();

        cloned!(self.bonsai_hg_mapping, self.repoid);
        find_toposorted_bonsai_cs_with_no_hg_cs_generated(
            ctx.clone(),
            repo.clone(),
            bcs_id.clone(),
            self.bonsai_hg_mapping.clone(),
        )
        .and_then({
            cloned!(ctx);
            move |commits_to_generate: Vec<BonsaiChangeset>| {
                let start = (0, commits_to_generate);

                loop_fn(
                    start,
                    move |(mut generated_count, mut commits_to_generate)| match commits_to_generate
                        .pop()
                    {
                        Some(bcs) => generate_single_hg_changeset(ctx.clone(), repo.clone(), bcs)
                            .map(move |(_, generated)| {
                                if generated {
                                    generated_count += 1;
                                }
                                Loop::Continue((generated_count, commits_to_generate))
                            })
                            .left_future(),
                        None => {
                            return bonsai_hg_mapping
                                .get_hg_from_bonsai(ctx.clone(), repoid, bcs_id)
                                .map(move |maybe_hg_cs_id| match maybe_hg_cs_id {
                                    Some(hg_cs_id) => Loop::Break((hg_cs_id, generated_count)),
                                    None => panic!("hg changeset must be generated already"),
                                })
                                .right_future();
                        }
                    },
                )
            }
        })
    }
}

/// Node hash handling for upload entries
pub enum UploadHgNodeHash {
    /// Generate the hash from the uploaded content
    Generate,
    /// This hash is used as the blobstore key, even if it doesn't match the hash of the
    /// parents and raw content. This is done because in some cases like root tree manifests
    /// in hybrid mode, Mercurial sends fake hashes.
    Supplied(HgNodeHash),
    /// As Supplied, but Verify the supplied hash - if it's wrong, you will get an error.
    Checked(HgNodeHash),
}

/// Context for uploading a Mercurial manifest entry.
pub struct UploadHgTreeEntry {
    pub upload_node_id: UploadHgNodeHash,
    pub contents: Bytes,
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub path: RepoPath,
}

impl UploadHgTreeEntry {
    // Given the content of a manifest, ensure that there is a matching HgBlobEntry in the repo.
    // This may not upload the entry or the data blob if the repo is aware of that data already
    // existing in the underlying store.
    //
    // Note that the HgBlobEntry may not be consistent - parents do not have to be uploaded at this
    // point, as long as you know their HgNodeHashes; this is also given to you as part of the
    // result type, so that you can parallelise uploads. Consistency will be verified when
    // adding the entries to a changeset.
    // adding the entries to a changeset.
    pub fn upload(
        self,
        ctx: CoreContext,
        repo: &BlobRepo,
    ) -> Result<(HgNodeHash, BoxFuture<(HgBlobEntry, RepoPath), Error>)> {
        self.upload_to_blobstore(ctx, &repo.blobstore)
    }

    pub(crate) fn upload_to_blobstore(
        self,
        ctx: CoreContext,
        blobstore: &RepoBlobstore,
    ) -> Result<(HgNodeHash, BoxFuture<(HgBlobEntry, RepoPath), Error>)> {
        STATS::upload_hg_tree_entry.add_value(1);
        let UploadHgTreeEntry {
            upload_node_id,
            contents,
            p1,
            p2,
            path,
        } = self;

        let logger = ctx.logger().clone();
        let computed_node_id = HgBlobNode::new(contents.clone(), p1, p2).nodeid();
        let node_id: HgNodeHash = match upload_node_id {
            UploadHgNodeHash::Generate => computed_node_id,
            UploadHgNodeHash::Supplied(node_id) => node_id,
            UploadHgNodeHash::Checked(node_id) => {
                if node_id != computed_node_id {
                    bail_err!(ErrorKind::InconsistentEntryHash(
                        path,
                        node_id,
                        computed_node_id
                    ));
                }
                node_id
            }
        };

        // This is the blob that gets uploaded. Manifest contents are usually small so they're
        // stored inline.
        let envelope = HgManifestEnvelopeMut {
            node_id,
            p1,
            p2,
            computed_node_id,
            contents,
        };
        let envelope_blob = envelope.freeze().into_blob();

        let manifest_id = HgManifestId::new(node_id);
        let blobstore_key = manifest_id.blobstore_key();

        let blob_entry = match path.mpath().and_then(|m| m.into_iter().last()) {
            Some(m) => {
                let entry_path = m.clone();
                HgBlobEntry::new(blobstore.clone(), entry_path, node_id, Type::Tree)
            }
            None => HgBlobEntry::new_root(blobstore.clone(), manifest_id),
        };

        fn log_upload_stats(
            logger: Logger,
            path: RepoPath,
            node_id: HgNodeHash,
            computed_node_id: HgNodeHash,
            stats: FutureStats,
        ) {
            trace!(logger, "Upload HgManifestEnvelope stats";
                "phase" => "manifest_envelope_uploaded".to_string(),
                "path" => format!("{}", path),
                "node_id" => format!("{}", node_id),
                "computed_node_id" => format!("{}", computed_node_id),
                "poll_count" => stats.poll_count,
                "poll_time_us" => stats.poll_time.as_micros_unchecked(),
                "completion_time_us" => stats.completion_time.as_micros_unchecked(),
            );
        }

        // Upload the blob.
        let upload = blobstore
            .put(ctx, blobstore_key, envelope_blob.into())
            .map({
                let path = path.clone();
                move |()| (blob_entry, path)
            })
            .timed({
                let logger = logger.clone();
                move |stats, result| {
                    if result.is_ok() {
                        log_upload_stats(logger, path, node_id, computed_node_id, stats);
                    }
                    Ok(())
                }
            });

        Ok((node_id, upload.boxify()))
    }
}

/// What sort of file contents are available to upload.
pub enum UploadHgFileContents {
    /// Content already uploaded (or scheduled to be uploaded). Metadata will be inlined in
    /// the envelope.
    ContentUploaded(ContentBlobMeta),
    /// Raw bytes as would be sent by Mercurial, including any metadata prepended in the standard
    /// Mercurial format.
    RawBytes(Bytes),
}

impl UploadHgFileContents {
    /// Upload the file contents if necessary, and asynchronously return the hash of the file node
    /// and metadata.
    fn execute(
        self,
        ctx: CoreContext,
        repo: &BlobRepo,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
        path: MPath,
    ) -> (
        ContentBlobInfo,
        // The future that does the upload and the future that computes the node ID/metadata are
        // split up to allow greater parallelism.
        impl Future<Item = (), Error = Error> + Send,
        impl Future<Item = (HgFileNodeId, Bytes, u64), Error = Error> + Send,
    ) {
        let (cbinfo, upload_fut, compute_fut) = match self {
            UploadHgFileContents::ContentUploaded(cbmeta) => {
                let upload_fut = future::ok(());

                let size = cbmeta.size;
                let cbinfo = ContentBlobInfo { path, meta: cbmeta };

                let lookup_fut = lookup_filenode_id(
                    ctx.clone(),
                    &repo.blobstore,
                    FileNodeIdPointer::new(&cbinfo.meta.id, &cbinfo.meta.copy_from, &p1, &p2),
                );

                let metadata_fut = Self::compute_metadata(
                    ctx.clone(),
                    repo,
                    cbinfo.meta.id,
                    cbinfo.meta.copy_from.clone(),
                );

                let content_id = cbinfo.meta.id;

                // Attempt to lookup filenode ID by alias. Fallback to computing it if we cannot.
                let compute_fut = (lookup_fut, metadata_fut).into_future().and_then({
                    cloned!(ctx, repo);
                    move |(res, metadata)| {
                        res.ok_or(())
                            .into_future()
                            .or_else({
                                cloned!(metadata);
                                move |_| {
                                    Self::compute_filenode_id(
                                        ctx, &repo, content_id, metadata, p1, p2,
                                    )
                                }
                            })
                            .map(move |fnid| (fnid, metadata, size))
                    }
                });

                (cbinfo, upload_fut.left_future(), compute_fut.left_future())
            }
            UploadHgFileContents::RawBytes(raw_content) => {
                let node_id = HgFileNodeId::new(
                    HgBlobNode::new(
                        raw_content.clone(),
                        p1.map(HgFileNodeId::into_nodehash),
                        p2.map(HgFileNodeId::into_nodehash),
                    )
                    .nodeid(),
                );

                let f = File::new(raw_content, p1, p2);
                let metadata = f.metadata();

                let copy_from = match f.copied_from() {
                    Ok(copy_from) => copy_from,
                    // XXX error out if copy-from information couldn't be read?
                    Err(_err) => None,
                };
                // Upload the contents separately (they'll be used for bonsai changesets as well).
                let file_bytes = f.file_contents();

                STATS::upload_blob.add_value(1);
                let (contents, upload_fut) =
                    filestore::store_bytes(&repo.blobstore, ctx.clone(), file_bytes.into_bytes());

                let upload_fut = upload_fut.timed({
                    cloned!(path);
                    let logger = ctx.logger().clone();
                    move |stats, result| {
                        if result.is_ok() {
                            UploadHgFileEntry::log_stats(
                                logger,
                                path,
                                node_id,
                                "content_uploaded",
                                stats,
                            );
                        }
                        Ok(())
                    }
                });

                let id = contents.content_id();
                let size = contents.size();

                let cbinfo = ContentBlobInfo {
                    path,
                    meta: ContentBlobMeta {
                        id,
                        size,
                        copy_from,
                    },
                };

                let compute_fut = future::ok((node_id, metadata, size));

                (
                    cbinfo,
                    upload_fut.right_future(),
                    compute_fut.right_future(),
                )
            }
        };

        let key = FileNodeIdPointer::new(&cbinfo.meta.id, &cbinfo.meta.copy_from, &p1, &p2);

        let compute_fut = compute_fut.and_then({
            cloned!(ctx, repo);
            move |(filenode_id, metadata, size)| {
                store_filenode_id(ctx, &repo.blobstore, key, &filenode_id)
                    .map(move |_| (filenode_id, metadata, size))
            }
        });

        (cbinfo, upload_fut, compute_fut)
    }

    fn compute_metadata(
        ctx: CoreContext,
        repo: &BlobRepo,
        content_id: ContentId,
        copy_from: Option<(MPath, HgFileNodeId)>,
    ) -> impl Future<Item = Bytes, Error = Error> {
        filestore::peek(
            &repo.blobstore,
            ctx,
            &FetchKey::Canonical(content_id),
            META_SZ,
        )
        .and_then(move |bytes| bytes.ok_or(ErrorKind::ContentBlobMissing(content_id).into()))
        .context("While computing metadata")
        .from_err()
        .map(move |bytes| {
            let mut metadata = Vec::new();
            File::generate_metadata(copy_from.as_ref(), &FileBytes(bytes), &mut metadata)
                .expect("Vec::write_all should never fail");

            // TODO: Introduce Metadata bytes?
            Bytes::from(metadata)
        })
    }

    fn compute_filenode_id(
        ctx: CoreContext,
        repo: &BlobRepo,
        content_id: ContentId,
        metadata: Bytes,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
    ) -> impl Future<Item = HgFileNodeId, Error = Error> {
        let file_bytes = filestore::fetch(&repo.blobstore, ctx, &FetchKey::Canonical(content_id))
            .and_then(move |stream| stream.ok_or(ErrorKind::ContentBlobMissing(content_id).into()))
            .flatten_stream();

        let all_bytes = once(Ok(metadata)).chain(file_bytes);

        let hg_parents = HgParents::new(
            p1.map(HgFileNodeId::into_nodehash),
            p2.map(HgFileNodeId::into_nodehash),
        );

        calculate_hg_node_id_stream(all_bytes, &hg_parents)
            .map(HgFileNodeId::new)
            .context("While computing a filenode id")
            .from_err()
    }
}

/// Context for uploading a Mercurial file entry.
pub struct UploadHgFileEntry {
    pub upload_node_id: UploadHgNodeHash,
    pub contents: UploadHgFileContents,
    pub file_type: FileType,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub path: MPath,
}

impl UploadHgFileEntry {
    pub fn upload(
        self,
        ctx: CoreContext,
        repo: &BlobRepo,
    ) -> Result<(ContentBlobInfo, BoxFuture<(HgBlobEntry, RepoPath), Error>)> {
        STATS::upload_hg_file_entry.add_value(1);
        let UploadHgFileEntry {
            upload_node_id,
            contents,
            file_type,
            p1,
            p2,
            path,
        } = self;

        let (cbinfo, content_upload, compute_fut) =
            contents.execute(ctx.clone(), repo, p1, p2, path.clone());
        let content_id = cbinfo.meta.id;

        let blobstore = repo.blobstore.clone();
        let logger = ctx.logger().clone();

        let envelope_upload =
            compute_fut.and_then(move |(computed_node_id, metadata, content_size)| {
                let node_id = match upload_node_id {
                    UploadHgNodeHash::Generate => computed_node_id,
                    UploadHgNodeHash::Supplied(node_id) => HgFileNodeId::new(node_id),
                    UploadHgNodeHash::Checked(node_id) => {
                        let node_id = HgFileNodeId::new(node_id);
                        if node_id != computed_node_id {
                            return Either::A(future::err(
                                ErrorKind::InconsistentEntryHash(
                                    RepoPath::FilePath(path),
                                    node_id.into_nodehash(),
                                    computed_node_id.into_nodehash(),
                                )
                                .into(),
                            ));
                        }
                        node_id
                    }
                };

                let file_envelope = HgFileEnvelopeMut {
                    node_id,
                    p1,
                    p2,
                    content_id,
                    content_size,
                    metadata,
                };
                let envelope_blob = file_envelope.freeze().into_blob();

                let blobstore_key = node_id.blobstore_key();

                let blob_entry = HgBlobEntry::new(
                    blobstore.clone(),
                    path.basename().clone(),
                    node_id.into_nodehash(),
                    Type::File(file_type),
                );

                let envelope_upload = blobstore
                    .put(ctx, blobstore_key, envelope_blob.into())
                    .timed({
                        let path = path.clone();
                        move |stats, result| {
                            if result.is_ok() {
                                Self::log_stats(
                                    logger,
                                    path,
                                    node_id,
                                    "file_envelope_uploaded",
                                    stats,
                                );
                            }
                            Ok(())
                        }
                    })
                    .map(move |()| (blob_entry, RepoPath::FilePath(path)));
                Either::B(envelope_upload)
            });

        let fut = envelope_upload
            .join(content_upload)
            .map(move |(envelope_res, ())| envelope_res);
        Ok((cbinfo, fut.boxify()))
    }

    fn log_stats(
        logger: Logger,
        path: MPath,
        nodeid: HgFileNodeId,
        phase: &str,
        stats: FutureStats,
    ) {
        let path = format!("{}", path);
        let nodeid = format!("{}", nodeid);
        trace!(logger, "Upload blob stats";
            "phase" => String::from(phase),
            "path" => path,
            "nodeid" => nodeid,
            "poll_count" => stats.poll_count,
            "poll_time_us" => stats.poll_time.as_micros_unchecked(),
            "completion_time_us" => stats.completion_time.as_micros_unchecked(),
        );
    }
}

/// Information about a content blob associated with a push that is available in
/// the blobstore. (This blob wasn't necessarily uploaded in this push.)
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentBlobInfo {
    pub path: MPath,
    pub meta: ContentBlobMeta,
}

/// Metadata associated with a content blob being uploaded as part of changeset creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentBlobMeta {
    pub id: ContentId,
    pub size: u64,
    // The copy info will later be stored as part of the commit.
    pub copy_from: Option<(MPath, HgFileNodeId)>,
}

/// This function uploads bonsai changests object to blobstore in parallel, and then does
/// sequential writes to changesets table. Parents of the changesets should already by saved
/// in the repository.
pub fn save_bonsai_changesets(
    bonsai_changesets: Vec<BonsaiChangeset>,
    ctx: CoreContext,
    repo: BlobRepo,
) -> impl Future<Item = (), Error = Error> {
    let complete_changesets = repo.changesets.clone();
    let blobstore = repo.blobstore.clone();
    let repoid = repo.repoid.clone();

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

    let mut topo_sorted_commits = sort_topological(&bcs_parents).expect("loop in commit chain!");
    // Reverse output to have parents in the beginning
    topo_sorted_commits.reverse();
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
        .and_then(move |_| {
            loop_fn(
                bonsai_complete_futs.into_iter(),
                move |mut futs| match futs.next() {
                    Some(fut) => fut
                        .and_then({ move |_| ok(Loop::Continue(futs)) })
                        .left_future(),
                    None => ok(Loop::Break(())).right_future(),
                },
            )
        })
        .and_then(|_| ok(()))
}

pub struct CreateChangeset {
    /// This should always be provided, keeping it an Option for tests
    pub expected_nodeid: Option<HgNodeHash>,
    pub expected_files: Option<Vec<MPath>>,
    pub p1: Option<ChangesetHandle>,
    pub p2: Option<ChangesetHandle>,
    // root_manifest can be None f.e. when commit removes all the content of the repo
    pub root_manifest: BoxFuture<Option<(HgBlobEntry, RepoPath)>, Error>,
    pub sub_entries: BoxStream<(HgBlobEntry, RepoPath), Error>,
    pub cs_metadata: ChangesetMetadata,
    pub must_check_case_conflicts: bool,
    // draft changesets don't have their filenodes stored in the filenodes table
    pub draft: bool,
}

impl CreateChangeset {
    pub fn create(
        self,
        ctx: CoreContext,
        repo: &BlobRepo,
        mut scuba_logger: ScubaSampleBuilder,
    ) -> ChangesetHandle {
        STATS::create_changeset.add_value(1);
        // This is used for logging, so that we can tie up all our pieces without knowing about
        // the final commit hash
        let uuid = Uuid::new_v4();
        scuba_logger.add("changeset_uuid", format!("{}", uuid));
        let event_id = EventId::new();

        let entry_processor = UploadEntries::new(
            repo.blobstore.clone(),
            repo.repoid.clone(),
            scuba_logger.clone(),
            self.draft,
        );
        let (signal_parent_ready, can_be_parent) = oneshot::channel();
        let expected_nodeid = self.expected_nodeid;

        let upload_entries = process_entries(
            ctx.clone(),
            &entry_processor,
            self.root_manifest,
            self.sub_entries,
        )
        .context("While processing entries")
        .traced_with_id(&ctx.trace(), "uploading entries", trace_args!(), event_id);

        let parents_complete = extract_parents_complete(&self.p1, &self.p2);
        let parents_data = handle_parents(scuba_logger.clone(), self.p1, self.p2)
            .context("While waiting for parents to upload data")
            .traced_with_id(
                &ctx.trace(),
                "waiting for parents data",
                trace_args!(),
                event_id,
            );
        let must_check_case_conflicts = self.must_check_case_conflicts.clone();
        let changeset = {
            let mut scuba_logger = scuba_logger.clone();
            upload_entries
                .join(parents_data)
                .from_err()
                .and_then({
                    cloned!(ctx, repo, repo.filenodes, repo.blobstore, mut scuba_logger);
                    let expected_files = self.expected_files;
                    let cs_metadata = self.cs_metadata;

                    move |(root_mf_id, (parents, parent_manifest_hashes, bonsai_parents))| {
                        let files = if let Some(expected_files) = expected_files {
                            STATS::create_changeset_expected_cf.add_value(1);
                            // We are trusting the callee to provide a list of changed files, used
                            // by the import job
                            future::ok(expected_files).boxify()
                        } else {
                            STATS::create_changeset_compute_cf.add_value(1);
                            compute_changed_files(
                                ctx.clone(),
                                repo.clone(),
                                root_mf_id,
                                parent_manifest_hashes.get(0),
                                parent_manifest_hashes.get(1),
                            )
                        };

                        let p1_mf = parent_manifest_hashes.get(0).cloned();
                        let check_case_conflicts = if must_check_case_conflicts {
                            check_case_conflicts(
                                ctx.clone(),
                                repo.clone(),
                                root_mf_id.clone(),
                                p1_mf,
                            )
                            .left_future()
                        } else {
                            future::ok(()).right_future()
                        };

                        let changesets = files
                            .join(check_case_conflicts)
                            .and_then(move |(files, ())| {
                                STATS::create_changeset_cf_count.add_value(files.len() as i64);
                                make_new_changeset(parents, root_mf_id, cs_metadata, files)
                            })
                            .and_then({
                                cloned!(ctx);
                                move |hg_cs| {
                                    create_bonsai_changeset_object(
                                        ctx,
                                        hg_cs.clone(),
                                        parent_manifest_hashes,
                                        bonsai_parents,
                                        repo.clone(),
                                    )
                                    .map(|bonsai_cs| (hg_cs, bonsai_cs))
                                }
                            });

                        changesets
                            .context("While computing changed files")
                            .and_then({
                                cloned!(ctx);
                                move |(blobcs, bonsai_cs)| {
                                    let fut: BoxFuture<(HgBlobChangeset, BonsaiChangeset), Error> =
                                        (move || {
                                            let bonsai_blob = bonsai_cs.clone().into_blob();
                                            let bcs_id = bonsai_blob.id().clone();

                                            let cs_id = blobcs.get_changeset_id().into_nodehash();
                                            let manifest_id = blobcs.manifestid();

                                            if let Some(expected_nodeid) = expected_nodeid {
                                                if cs_id != expected_nodeid {
                                                    return future::err(
                                                        ErrorKind::InconsistentChangesetHash(
                                                            expected_nodeid,
                                                            cs_id,
                                                            blobcs,
                                                        )
                                                        .into(),
                                                    )
                                                    .boxify();
                                                }
                                            }

                                            scuba_logger
                                                .add("changeset_id", format!("{}", cs_id))
                                                .log_with_msg(
                                                    "Changeset uuid to hash mapping",
                                                    None,
                                                );
                                            // NOTE(luk): an attempt was made in D8187210 to split the
                                            // upload_entries signal into upload_entries and
                                            // processed_entries and to signal_parent_ready after
                                            // upload_entries, so that one doesn't need to wait for the
                                            // entries to be processed. There were no performance gains
                                            // from that experiment
                                            //
                                            // We deliberately eat this error - this is only so that
                                            // another changeset can start verifying data in the blob
                                            // store while we verify this one
                                            let _ = signal_parent_ready.send((
                                                bcs_id,
                                                cs_id,
                                                manifest_id,
                                            ));

                                            let bonsai_cs_fut = save_bonsai_changeset_object(
                                                ctx.clone(),
                                                blobstore.clone(),
                                                bonsai_cs.clone(),
                                            );

                                            blobcs
                                                .save(ctx.clone(), blobstore)
                                                .join(bonsai_cs_fut)
                                                .context("While writing to blobstore")
                                                .join(
                                                    entry_processor
                                                        .finalize(ctx, filenodes, cs_id)
                                                        .context("While finalizing processing"),
                                                )
                                                .from_err()
                                                .map(move |_| (blobcs, bonsai_cs))
                                                .boxify()
                                        })();

                                    fut.context(
                                        "While creating and verifying Changeset for blobstore",
                                    )
                                }
                            })
                            .traced_with_id(
                                &ctx.trace(),
                                "uploading changeset",
                                trace_args!(),
                                event_id,
                            )
                            .from_err()
                    }
                })
                .timed(move |stats, result| {
                    if result.is_ok() {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Changeset created", None);
                    }
                    Ok(())
                })
        };

        let parents_complete = parents_complete
            .context("While waiting for parents to complete")
            .traced_with_id(
                &ctx.trace(),
                "waiting for parents complete",
                trace_args!(),
                event_id,
            )
            .timed({
                let mut scuba_logger = scuba_logger.clone();
                move |stats, result| {
                    if result.is_ok() {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Parents completed", None);
                    }
                    Ok(())
                }
            });

        let complete_changesets = repo.changesets.clone();
        cloned!(repo, repo.repoid);
        let changeset_complete_fut = changeset
            .join(parents_complete)
            .and_then({
                cloned!(ctx, repo.bonsai_hg_mapping);
                move |((hg_cs, bonsai_cs), _)| {
                    let bcs_id = bonsai_cs.get_changeset_id();
                    let bonsai_hg_entry = BonsaiHgMappingEntry {
                        repo_id: repoid.clone(),
                        hg_cs_id: hg_cs.get_changeset_id(),
                        bcs_id,
                    };

                    bonsai_hg_mapping
                        .add(ctx.clone(), bonsai_hg_entry)
                        .map(move |_| (hg_cs, bonsai_cs))
                        .context("While inserting mapping")
                        .traced_with_id(
                            &ctx.trace(),
                            "uploading bonsai hg mapping",
                            trace_args!(),
                            event_id,
                        )
                }
            })
            .and_then(move |(hg_cs, bonsai_cs)| {
                let completion_record = ChangesetInsert {
                    repo_id: repo.repoid,
                    cs_id: bonsai_cs.get_changeset_id(),
                    parents: bonsai_cs.parents().into_iter().collect(),
                };
                complete_changesets
                    .add(ctx.clone(), completion_record)
                    .map(|_| (bonsai_cs, hg_cs))
                    .context("While inserting into changeset table")
                    .traced_with_id(
                        &ctx.trace(),
                        "uploading final changeset",
                        trace_args!(),
                        event_id,
                    )
            })
            .with_context(move |_| {
                format!(
                    "While creating Changeset {:?}, uuid: {}",
                    expected_nodeid, uuid
                )
            })
            .map_err(Error::from)
            .timed({
                move |stats, result| {
                    if result.is_ok() {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("CreateChangeset Finished", None);
                    }
                    Ok(())
                }
            });

        ChangesetHandle::new_pending(
            can_be_parent.shared(),
            spawn_future(changeset_complete_fut)
                .map_err(|e| Error::from(e).compat())
                .boxify()
                .shared(),
        )
    }
}

impl Clone for BlobRepo {
    fn clone(&self) -> Self {
        Self {
            bookmarks: self.bookmarks.clone(),
            blobstore: self.blobstore.clone(),
            filenodes: self.filenodes.clone(),
            changesets: self.changesets.clone(),
            bonsai_hg_mapping: self.bonsai_hg_mapping.clone(),
            repoid: self.repoid.clone(),
            changeset_fetcher_factory: self.changeset_fetcher_factory.clone(),
            derived_data_lease: self.derived_data_lease.clone(),
            filestore_config: self.filestore_config.clone(),
        }
    }
}

fn to_hg_bookmark_stream<T>(
    repo: &BlobRepo,
    ctx: &CoreContext,
    stream: T,
) -> impl Stream<Item = (Bookmark, HgChangesetId), Error = Error>
where
    T: Stream<Item = (Bookmark, ChangesetId), Error = Error>,
{
    // TODO: (torozco) T44876554 If this hits the database for all (or most of) the bookmarks,
    // it'll be fairly inefficient.
    stream
        .map({
            cloned!(repo, ctx);
            move |(bookmark, cs_id)| {
                repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                    .map(move |cs_id| (bookmark, cs_id))
            }
        })
        .buffer_unordered(100)
}

impl UnittestOverride<Arc<dyn LeaseOps>> for BlobRepo {
    fn unittest_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(Arc<dyn LeaseOps>) -> Arc<dyn LeaseOps>,
    {
        let derived_data_lease = modify(self.derived_data_lease.clone());
        BlobRepo {
            derived_data_lease,
            ..self.clone()
        }
    }
}
