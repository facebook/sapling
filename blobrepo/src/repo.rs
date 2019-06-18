// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::alias::{get_content_id_alias_key, get_sha256_alias, get_sha256_alias_key};
use super::utils::{sort_topological, IncompleteFilenodeInfo, IncompleteFilenodes};
use crate::bonsai_generation::{create_bonsai_changeset_object, save_bonsai_changeset_object};
use crate::errors::*;
use crate::file::{
    fetch_file_content_from_blobstore, fetch_file_content_id_from_blobstore,
    fetch_file_content_sha256_from_blobstore, fetch_file_contents, fetch_file_envelope,
    fetch_file_parents_from_blobstore, fetch_file_size_from_blobstore, fetch_raw_filenode_bytes,
    fetch_rename_from_blobstore, get_rename_from_envelope, HgBlobEntry,
};
use crate::memory_manifest::MemoryRootManifest;
use crate::repo_commit::*;
use crate::{BlobManifest, HgBlobChangeset};
use blob_changeset::{ChangesetMetadata, HgChangesetContent, RepoBlobstore};
use blobstore::Blobstore;
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
use futures::future::{self, loop_fn, ok, Either, Future, Loop};
use futures::stream::{FuturesUnordered, Stream};
use futures::sync::oneshot;
use futures::IntoFuture;
use futures_ext::{spawn_future, try_boxfuture, BoxFuture, BoxStream, FutureExt};
use futures_stats::{FutureStats, Timed};
use mercurial::file::File;
use mercurial_types::manifest::Content;
use mercurial_types::{
    Changeset, Entry, HgBlob, HgBlobNode, HgChangesetId, HgEntryId, HgFileEnvelopeMut,
    HgFileNodeId, HgManifestEnvelopeMut, HgManifestId, HgNodeHash, HgParents, Manifest, RepoPath,
    Type,
};
use mononoke_types::{
    hash::Blake2, hash::Sha256, Blob, BlobstoreBytes, BlobstoreValue, BonsaiChangeset, ChangesetId,
    ContentId, FileChange, FileContents, FileType, Generation, MPath, MPathElement, MononokeId,
    RepositoryId, Timestamp,
};
use prefixblob::PrefixBlobstore;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::{trace, Logger};
use stats::{define_stats, Histogram, Timeseries};
use std::collections::HashMap;
use std::convert::From;
use std::str::FromStr;
use std::sync::Arc;
use time_ext::DurationExt;
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
    logger: Logger,
    blobstore: RepoBlobstore,
    bookmarks: Arc<Bookmarks>,
    filenodes: Arc<Filenodes>,
    changesets: Arc<Changesets>,
    bonsai_hg_mapping: Arc<BonsaiHgMapping>,
    repoid: RepositoryId,
    // Returns new ChangesetFetcher that can be used by operation that work with commit graph
    // (for example, revsets).
    changeset_fetcher_factory: Arc<Fn() -> Arc<ChangesetFetcher + Send + Sync> + Send + Sync>,
    hg_generation_lease: Arc<LeaseOps>,
}

impl BlobRepo {
    pub fn new(
        logger: Logger,
        bookmarks: Arc<Bookmarks>,
        blobstore: Arc<Blobstore>,
        filenodes: Arc<Filenodes>,
        changesets: Arc<Changesets>,
        bonsai_hg_mapping: Arc<BonsaiHgMapping>,
        repoid: RepositoryId,
        hg_generation_lease: Arc<LeaseOps>,
    ) -> Self {
        let changeset_fetcher_factory = {
            cloned!(changesets, repoid);
            move || {
                let res: Arc<ChangesetFetcher + Send + Sync> = Arc::new(
                    SimpleChangesetFetcher::new(changesets.clone(), repoid.clone()),
                );
                res
            }
        };

        BlobRepo {
            logger,
            bookmarks,
            blobstore: PrefixBlobstore::new(blobstore, repoid.prefix()),
            filenodes,
            changesets,
            bonsai_hg_mapping,
            repoid,
            changeset_fetcher_factory: Arc::new(changeset_fetcher_factory),
            hg_generation_lease,
        }
    }

    pub fn new_with_changeset_fetcher_factory(
        logger: Logger,
        bookmarks: Arc<Bookmarks>,
        blobstore: Arc<Blobstore>,
        filenodes: Arc<Filenodes>,
        changesets: Arc<Changesets>,
        bonsai_hg_mapping: Arc<BonsaiHgMapping>,
        repoid: RepositoryId,
        changeset_fetcher_factory: Arc<Fn() -> Arc<ChangesetFetcher + Send + Sync> + Send + Sync>,
        hg_generation_lease: Arc<LeaseOps>,
    ) -> Self {
        BlobRepo {
            logger,
            bookmarks,
            blobstore: PrefixBlobstore::new(blobstore, repoid.prefix()),
            filenodes,
            changesets,
            bonsai_hg_mapping,
            repoid,
            changeset_fetcher_factory,
            hg_generation_lease,
        }
    }

    /// Convert this BlobRepo instance into one that only does writes in memory.
    ///
    /// ------------
    /// IMPORTANT!!!
    /// ------------
    /// Currently this applies to the blobstore *ONLY*. A future improvement would be to also
    /// do database writes in-memory.
    #[allow(non_snake_case)]
    pub fn in_memory_writes_READ_DOC_COMMENT(self) -> BlobRepo {
        let BlobRepo {
            logger,
            bookmarks,
            blobstore,
            filenodes,
            changesets,
            bonsai_hg_mapping,
            repoid,
            hg_generation_lease,
            ..
        } = self;

        // Drop the PrefixBlobstore (it will be wrapped up in one again by BlobRepo::new)
        let blobstore = blobstore.into_inner();
        let blobstore = Arc::new(MemWritesBlobstore::new(blobstore));

        BlobRepo::new(
            logger,
            bookmarks,
            blobstore,
            filenodes,
            changesets,
            bonsai_hg_mapping,
            repoid,
            hg_generation_lease,
        )
    }

    fn fetch<K>(
        &self,
        ctx: CoreContext,
        key: &K,
    ) -> impl Future<Item = K::Value, Error = Error> + Send
    where
        K: MononokeId,
    {
        let blobstore_key = key.blobstore_key();
        self.blobstore
            .get(ctx, blobstore_key.clone())
            .and_then(move |blob| {
                blob.ok_or(ErrorKind::MissingTypedKeyEntry(blobstore_key).into())
                    .and_then(|blob| <<K as MononokeId>::Value>::from_blob(blob.into()))
            })
    }

    // this is supposed to be used only from unittest
    pub fn unittest_fetch<K>(
        &self,
        ctx: CoreContext,
        key: &K,
    ) -> impl Future<Item = K::Value, Error = Error> + Send
    where
        K: MononokeId,
    {
        self.fetch(ctx, key)
    }

    fn store<K, V>(&self, ctx: CoreContext, value: V) -> impl Future<Item = K, Error = Error> + Send
    where
        V: BlobstoreValue<Key = K>,
        K: MononokeId<Value = V>,
    {
        let blob = value.into_blob();
        let key = *blob.id();
        self.blobstore
            .put(ctx, key.blobstore_key(), blob.into())
            .map(move |_| key)
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
    ) -> BoxFuture<FileContents, Error> {
        STATS::get_file_content.add_value(1);
        fetch_file_content_from_blobstore(ctx, &self.blobstore, key).boxify()
    }

    pub fn get_file_content_by_content_id(
        &self,
        ctx: CoreContext,
        id: ContentId,
    ) -> impl Future<Item = FileContents, Error = Error> {
        fetch_file_contents(ctx, &self.blobstore, id)
    }

    pub fn get_file_size(
        &self,
        ctx: CoreContext,
        key: HgFileNodeId,
    ) -> impl Future<Item = u64, Error = Error> {
        fetch_file_size_from_blobstore(ctx, &self.blobstore, key)
    }

    pub fn get_file_content_id(
        &self,
        ctx: CoreContext,
        key: HgFileNodeId,
    ) -> impl Future<Item = ContentId, Error = Error> {
        fetch_file_content_id_from_blobstore(ctx, &self.blobstore, key)
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
    ) -> impl Future<Item = Sha256, Error = Error> {
        let blobrepo = self.clone();
        cloned!(content_id, self.blobstore);

        // try to get sha256 from blobstore from a blob to avoid calculation
        self.get_alias_content_id_to_sha256(ctx.clone(), content_id)
            .and_then(move |res| match res {
                Some(file_content_sha256) => Ok(file_content_sha256).into_future().left_future(),
                None => {
                    fetch_file_content_sha256_from_blobstore(ctx.clone(), &blobstore, content_id)
                        .and_then(move |alias| {
                            blobrepo
                                .put_alias_content_id_to_sha256(ctx, content_id, alias)
                                .map(move |()| alias)
                        })
                        .right_future()
                }
            })
    }

    fn put_alias_content_id_to_sha256(
        &self,
        ctx: CoreContext,
        content_id: ContentId,
        alias_content: Sha256,
    ) -> impl Future<Item = (), Error = Error> {
        let alias_key = get_content_id_alias_key(content_id);
        // Contents = alias.sha256.SHA256HASH (BlobstoreBytes)
        let contents = BlobstoreBytes::from_bytes(Bytes::from(alias_content.as_ref()));

        self.upload_blobstore_bytes(ctx, alias_key, contents)
            .map(|_| ())
    }

    fn get_alias_content_id_to_sha256(
        &self,
        ctx: CoreContext,
        content_id: ContentId,
    ) -> impl Future<Item = Option<Sha256>, Error = Error> {
        // Ok: Some(value) - found alias blob, None - alias blob nor found (lazy upload)
        // Invalid alias blob content is considered as "Not found"
        // Err: Error from server, does not proceed the opertion further
        let alias_content_id = get_content_id_alias_key(content_id);

        self.blobstore
            .get(ctx, alias_content_id.clone())
            .map(|content_key_bytes| {
                content_key_bytes.and_then(|bytes| Sha256::from_bytes(bytes.as_bytes()).ok())
            })
    }

    pub fn upload_file_content_by_alias(
        &self,
        ctx: CoreContext,
        _alias: Sha256,
        raw_file_content: Bytes,
    ) -> impl Future<Item = (), Error = Error> {
        // Get alias of raw file contents
        let alias_key = get_sha256_alias(&raw_file_content);
        // Raw contents = file content only, excluding metadata in the beginning
        let contents = FileContents::Bytes(raw_file_content);
        self.upload_blob(ctx, contents.into_blob(), alias_key)
            .map(|_| ())
            .boxify()
    }

    pub fn get_file_content_by_alias(
        &self,
        ctx: CoreContext,
        alias: Sha256,
    ) -> impl Future<Item = FileContents, Error = Error> {
        let blobstore = self.blobstore.clone();

        self.get_file_content_id_by_alias(ctx.clone(), alias)
            .and_then(move |content_id| fetch_file_contents(ctx, &blobstore, content_id))
            .from_err()
    }

    pub fn get_file_content_id_by_alias(
        &self,
        ctx: CoreContext,
        alias: Sha256,
    ) -> impl Future<Item = ContentId, Error = Error> {
        STATS::get_file_content.add_value(1);
        let prefixed_key = get_sha256_alias_key(alias.to_hex().to_string());
        let blobstore = self.blobstore.clone();

        blobstore
            .get(ctx, prefixed_key.clone())
            .and_then(move |bytes| {
                let content_key_bytes = match bytes {
                    Some(bytes) => bytes,
                    None => bail_err!(ErrorKind::MissingTypedKeyEntry(prefixed_key)),
                };
                Ok(content_key_bytes)
            })
            .and_then(move |content_key_bytes| {
                let content_key = content_key_bytes.as_bytes().as_ref();

                // check expected prefix
                let content_prefix = ContentId::blobstore_key_prefix();
                let prefix_len = content_prefix.len();

                if prefix_len > content_key.len()
                    || &content_key[..prefix_len] != content_prefix.as_bytes()
                {
                    let e: Error = ErrorKind::IncorrectAliasBlobContent(alias).into();
                    try_boxfuture!(Err(e))
                }

                let blake2_hash = &content_key[prefix_len..];

                // Need to convert hex_bytes -> String -> bytes for Blake2
                String::from_utf8(blake2_hash.to_vec())
                    .into_future()
                    .from_err()
                    .and_then(|blake2_str| Blake2::from_str(&blake2_str))
                    .map(ContentId::new)
                    .context("While casting alias blob contents, to content id")
                    .from_err()
                    .boxify()
            })
    }

    pub fn generate_lfs_file(
        &self,
        ctx: CoreContext,
        content_id: ContentId,
        file_size: u64,
    ) -> impl Future<Item = FileContents, Error = Error> {
        self.get_file_sha256(ctx, content_id)
            .and_then(move |alias| File::generate_lfs_file(alias, file_size))
            .map(|bytes| FileContents::Bytes(bytes))
    }

    // TODO: (rain1) T30456231 It should be possible in principle to make the return type a wrapper
    // around a Chain, but it isn't because of API deficiencies in bytes::Buf. See D8412210.

    /// The raw filenode content is crucial for operation like delta application. It is stored in
    /// untouched represenation that came from Mercurial client.
    pub fn get_raw_hg_content(
        &self,
        ctx: CoreContext,
        key: HgFileNodeId,
        validate_hash: bool,
    ) -> BoxFuture<HgBlob, Error> {
        STATS::get_raw_hg_content.add_value(1);
        fetch_raw_filenode_bytes(ctx, &self.blobstore, key, validate_hash)
    }

    // Fetches copy data from blobstore instead of from filenodes db. This should be used only
    // during committing.
    pub(crate) fn get_hg_file_copy_from_blobstore(
        &self,
        ctx: CoreContext,
        key: HgFileNodeId,
    ) -> BoxFuture<Option<(RepoPath, HgFileNodeId)>, Error> {
        STATS::get_hg_file_copy_from_blobstore.add_value(1);
        fetch_rename_from_blobstore(ctx, &self.blobstore, key)
            .map(|rename| rename.map(|(path, hash)| (RepoPath::FilePath(path), hash)))
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
    ) -> BoxFuture<Box<Manifest + Sync>, Error> {
        STATS::get_manifest_by_nodeid.add_value(1);
        BlobManifest::load(ctx, &self.blobstore, manifestid)
            .and_then(move |mf| mf.ok_or(ErrorKind::ManifestMissing(manifestid).into()))
            .map(|m| m.boxed())
            .boxify()
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

    pub fn update_bookmark_transaction(&self, ctx: CoreContext) -> Box<bookmarks::Transaction> {
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
                    let copyfrom = get_rename_from_envelope(envelope)
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

    pub fn get_all_filenodes(
        &self,
        ctx: CoreContext,
        path: RepoPath,
    ) -> BoxFuture<Vec<FilenodeInfo>, Error> {
        STATS::get_all_filenodes.add_value(1);
        self.filenodes.get_all_filenodes(ctx, &path, self.repoid)
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
        self.fetch(ctx, &bonsai_cs_id).boxify()
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

    pub fn get_changeset_fetcher(&self) -> Arc<ChangesetFetcher> {
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

        self.blobstore.put(ctx, key.clone(), contents).timed({
            let logger = self.logger.clone();
            move |stats, result| {
                if result.is_ok() {
                    log_upload_stats(logger, key, "blob uploaded", stats)
                }
                Ok(())
            }
        })
    }

    pub fn upload_blob_no_alias<Id>(
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

    pub fn upload_blob<Id>(
        &self,
        ctx: CoreContext,
        blob: Blob<Id>,
        alias_key: String,
    ) -> impl Future<Item = Id, Error = Error> + Send
    where
        Id: MononokeId,
    {
        STATS::upload_blob.add_value(1);
        let id = blob.id().clone();
        let blobstore_key = id.blobstore_key();
        let blob_contents: BlobstoreBytes = blob.into();

        // Upload {alias.sha256.sha256(blob_contents): blobstore_key}
        let alias_key_operation = {
            let contents = BlobstoreBytes::from_bytes(blobstore_key.as_bytes());
            self.upload_blobstore_bytes(ctx.clone(), alias_key, contents)
        };

        // Upload {blobstore_key: blob_contents}
        let blobstore_key_operation =
            self.upload_blobstore_bytes(ctx, blobstore_key, blob_contents.clone());

        blobstore_key_operation
            .join(alias_key_operation)
            .map(move |((), ())| id)
    }

    pub fn upload_alias_to_file_content_id(
        &self,
        ctx: CoreContext,
        alias: Sha256,
        content_id: ContentId,
    ) -> impl Future<Item = (), Error = Error> + Send {
        self.upload_blobstore_bytes(
            ctx,
            get_sha256_alias_key(alias.to_hex().to_string()),
            BlobstoreBytes::from_bytes(content_id.blobstore_key().as_bytes()),
        )
    }

    // This is used by tests
    pub fn get_blobstore(&self) -> RepoBlobstore {
        self.blobstore.clone()
    }

    pub fn get_logger(&self) -> Logger {
        self.logger.clone()
    }

    pub fn get_repoid(&self) -> RepositoryId {
        self.repoid
    }

    pub fn get_filenodes(&self) -> Arc<Filenodes> {
        self.filenodes.clone()
    }

    pub fn store_file_change_or_reuse(
        &self,
        ctx: CoreContext,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
        path: &MPath,
        change: Option<&FileChange>,
    ) -> impl Future<Item = Option<(HgBlobEntry, Option<IncompleteFilenodeInfo>)>, Error = Error>
    {
        // we can reuse same HgFileNodeId if we have only one parent with same
        // file content but different type (Regular|Executable)
        match (p1, p2, change) {
            (Some(parent), None, Some(change)) | (None, Some(parent), Some(change)) => {
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
            let change = change.cloned();
            let path = path.clone();

            move |maybe_entry| match maybe_entry {
                None => repo
                    .store_file_change(ctx, p1, p2, &path, change.as_ref())
                    .right_future(),
                _ => future::ok(maybe_entry).left_future(),
            }
        })
    }

    pub fn store_file_change(
        &self,
        ctx: CoreContext,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
        path: &MPath,
        change: Option<&FileChange>,
    ) -> impl Future<Item = Option<(HgBlobEntry, Option<IncompleteFilenodeInfo>)>, Error = Error> + Send
    {
        let repo = self.clone();
        match change {
            None => future::ok(None).left_future(),
            Some(change) => {
                let copy_from_fut = match change.copy_from() {
                    None => future::ok(None).left_future(),
                    Some((path, bcs_id)) => self
                        .get_hg_from_bonsai_changeset(ctx.clone(), *bcs_id)
                        .and_then({
                            cloned!(ctx, repo);
                            move |cs_id| repo.get_changeset_by_changesetid(ctx, cs_id)
                        })
                        .and_then({
                            cloned!(ctx, repo, path);
                            move |cs| repo.find_file_in_manifest(ctx, &path, cs.manifestid())
                        })
                        .and_then({
                            cloned!(path);
                            move |res| match res {
                                Some((_, node_id)) => Ok(Some((path, node_id))),
                                None => Err(ErrorKind::PathNotFound(path).into()),
                            }
                        })
                        .right_future(),
                };
                let upload_fut = copy_from_fut.and_then({
                    cloned!(ctx, repo, path, change);
                    move |copy_from| {
                        let mut p1 = p1;
                        let mut p2 = p2;

                        // Mercurial has complicated logic of finding file parents, especially
                        // if a file was also copied/moved.
                        // See mercurial/localrepo.py:_filecommit(). We have to replicate this
                        // logic in Mononoke.
                        // TODO(stash): T45618931 replicate all the cases from _filecommit()
                        if let Some((ref copy_from_path, _)) = copy_from {
                            if copy_from_path != &path {
                                if p1.is_some() && p2.is_none() {
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
                                    p1 = None;
                                    p2 = None;
                                }
                            }
                        }

                        let upload_entry = UploadHgFileEntry {
                            upload_node_id: UploadHgNodeHash::Generate,
                            contents: UploadHgFileContents::ContentUploaded(ContentBlobMeta {
                                id: change.content_id(),
                                copy_from: copy_from.clone(),
                            }),
                            file_type: change.file_type(),
                            p1,
                            p2,
                            path: path.clone(),
                        };
                        let upload_fut = match upload_entry.upload(ctx, &repo) {
                            Ok((_, upload_fut)) => upload_fut.map(move |(entry, _)| {
                                let node_info = IncompleteFilenodeInfo {
                                    path: RepoPath::FilePath(path),
                                    filenode: HgFileNodeId::new(entry.get_hash().into_nodehash()),
                                    p1,
                                    p2,
                                    copyfrom: copy_from.map(|(p, h)| (RepoPath::FilePath(p), h)),
                                };
                                Some((entry, Some(node_info)))
                            }),
                            Err(err) => return future::err(err).left_future(),
                        };
                        upload_fut.right_future()
                    }
                });
                upload_fut.right_future()
            }
        }
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
                        let next_element = elements.next();
                        if let None = next_element {
                            return future::ok(Loop::Break(false)).boxify();
                        }
                        let element = next_element.unwrap();

                        match mf.lookup(&element) {
                            Some(entry) => {
                                let cur_path = MPath::join_opt_element(cur_path.as_ref(), &element);
                                // avoid fetching file content
                                match entry.get_type() {
                                    Type::File(_) => future::ok(Loop::Break(false)).boxify(),
                                    Type::Tree => entry
                                        .get_content(ctx.clone())
                                        .map(move |content| match content {
                                            Content::Tree(mf) => {
                                                Loop::Continue((Some(cur_path), mf, elements))
                                            }
                                            _ => Loop::Break(false),
                                        })
                                        .boxify(),
                                }
                            }
                            None => {
                                let element_utf8 = String::from_utf8(element.to_bytes());
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
                                    match (&element_utf8, String::from_utf8(basename.to_bytes())) {
                                        (Ok(ref element), Ok(ref basename)) => {
                                            if basename.to_lowercase() == element.to_lowercase() {
                                                potential_conflicts.push(path);
                                            }
                                        }
                                        _ => (),
                                    }
                                }

                                // For each potential conflict we need to check if it's present in
                                // child manifest. If it is, then we've got a conflict, otherwise
                                // this has been deleted and it's no longer a conflict.
                                let mut check_futs = vec![];
                                for fullpath in potential_conflicts {
                                    let check_fut = repo
                                        .find_path_in_manifest(
                                            ctx.clone(),
                                            fullpath,
                                            child_mf_id.clone(),
                                        )
                                        .map(|content_and_node| content_and_node.is_some());
                                    check_futs.push(check_fut);
                                }

                                future::join_all(check_futs.into_iter())
                                    .map(|potential_conflicts| {
                                        let has_case_conflict =
                                            potential_conflicts.iter().any(|val| *val);
                                        Loop::Break(has_case_conflict)
                                    })
                                    .boxify()
                            }
                        }
                    },
                )
            })
    }

    pub fn find_path_in_manifest(
        &self,
        ctx: CoreContext,
        path: Option<MPath>,
        manifest_id: HgManifestId,
    ) -> impl Future<Item = Option<(Content, HgEntryId)>, Error = Error> + Send {
        // single fold step, converts path elemnt in content to content, if any
        fn find_content_in_content(
            ctx: CoreContext,
            content: BoxFuture<Option<(Content, HgEntryId)>, Error>,
            path_element: MPathElement,
        ) -> BoxFuture<Option<(Content, HgEntryId)>, Error> {
            content
                .and_then(move |content_and_node| match content_and_node {
                    None => future::ok(None).left_future(),
                    Some((Content::Tree(manifest), _)) => match manifest.lookup(&path_element) {
                        None => future::ok(None).left_future(),
                        Some(entry) => {
                            let hash = entry.get_hash();
                            entry
                                .get_content(ctx)
                                .map(move |content| (content, hash))
                                .map(Some)
                                .right_future()
                        }
                    },
                    Some(_) => future::ok(None).left_future(),
                })
                .boxify()
        }

        self.get_manifest_by_nodeid(ctx.clone(), manifest_id)
            .and_then(move |manifest| {
                let content_init =
                    { future::ok(Some((Content::Tree(manifest), manifest_id.into()))).boxify() };
                match path {
                    None => content_init,
                    Some(path) => {
                        path.into_iter()
                            .fold(content_init, move |content, path_element| {
                                find_content_in_content(ctx.clone(), content, path_element)
                            })
                    }
                }
            })
    }

    pub fn find_file_in_manifest(
        &self,
        ctx: CoreContext,
        path: &MPath,
        manifest: HgManifestId,
    ) -> impl Future<Item = Option<(FileType, HgFileNodeId)>, Error = Error> + Send {
        let (dirname, basename) = path.split_dirname();
        self.find_path_in_manifest(ctx, dirname, manifest).map({
            let basename = basename.clone();
            move |content_and_node| match content_and_node {
                None => None,
                Some((Content::Tree(manifest), _)) => match manifest.lookup(&basename) {
                    None => None,
                    Some(entry) => {
                        if let Type::File(t) = entry.get_type() {
                            Some((t, HgFileNodeId::new(entry.get_hash().into_nodehash())))
                        } else {
                            None
                        }
                    }
                },
                Some(_) => None,
            }
        })
    }

    pub fn get_manifest_from_bonsai(
        &self,
        ctx: CoreContext,
        bcs: BonsaiChangeset,
        manifest_p1: Option<HgManifestId>,
        manifest_p2: Option<HgManifestId>,
    ) -> BoxFuture<(HgManifestId, IncompleteFilenodes), Error> {
        let p1 = manifest_p1.map(|id| id.into_nodehash());
        let p2 = manifest_p2.map(|id| id.into_nodehash());
        MemoryRootManifest::new(
            ctx.clone(),
            self.clone(),
            IncompleteFilenodes::new(),
            p1,
            p2,
        )
        .and_then({
            let repo = self.clone();
            move |memory_manifest| {
                let memory_manifest = Arc::new(memory_manifest);
                let incomplete_filenodes = memory_manifest.get_incomplete_filenodes();
                let mut futures = Vec::new();

                for (path, entry) in bcs.file_changes() {
                    cloned!(path, memory_manifest, incomplete_filenodes);
                    let p1 = manifest_p1
                        .map(|manifest| {
                            repo.find_file_in_manifest(ctx.clone(), &path, manifest)
                                .map(|o| o.map(|(_, x)| x))
                        })
                        .into_future();
                    let p2 = manifest_p2
                        .map(|manifest| {
                            repo.find_file_in_manifest(ctx.clone(), &path, manifest)
                                .map(|o| o.map(|(_, x)| x))
                        })
                        .into_future();
                    let future = (p1, p2)
                        .into_future()
                        .and_then({
                            let entry = entry.cloned();
                            cloned!(ctx, repo, path);
                            move |(p1, p2)| {
                                repo.store_file_change_or_reuse(
                                    ctx,
                                    p1.and_then(|x| x),
                                    p2.and_then(|x| x),
                                    &path,
                                    entry.as_ref(),
                                )
                            }
                        })
                        .and_then({
                            cloned!(ctx);
                            move |entry| match entry {
                                None => memory_manifest.change_entry(ctx, &path, None),
                                Some((entry, node_infos)) => {
                                    for node_info in node_infos {
                                        incomplete_filenodes.add(node_info);
                                    }
                                    memory_manifest.change_entry(ctx, &path, Some(entry))
                                }
                            }
                        });
                    futures.push(future);
                }

                future::join_all(futures)
                    .and_then({
                        cloned!(ctx, memory_manifest);
                        move |_| memory_manifest.resolve_trivial_conflicts(ctx)
                    })
                    .and_then(move |_| memory_manifest.save(ctx))
                    .map({
                        cloned!(incomplete_filenodes);
                        move |m| {
                            (
                                HgManifestId::new(m.get_hash().into_nodehash()),
                                incomplete_filenodes,
                            )
                        }
                    })
            }
        })
        .boxify()
    }

    pub fn get_hg_from_bonsai_changeset(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
    ) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
        STATS::get_hg_from_bonsai_changeset.add_value(1);
        self.get_hg_from_bonsai_changeset_with_impl(ctx, bcs_id, 0)
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

    fn generate_lease_key(&self, bcs_id: &ChangesetId) -> String {
        let repoid = self.get_repoid();
        format!("repoid.{}.bonsai.{}", repoid.id(), bcs_id)
    }

    fn take_hg_generation_lease(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
    ) -> impl Future<Item = Option<HgChangesetId>, Error = Error> + Send {
        let key = self.generate_lease_key(&bcs_id);
        let repoid = self.get_repoid();

        cloned!(self.bonsai_hg_mapping, self.hg_generation_lease);
        let repo = self.clone();

        loop_fn((), move |()| {
            cloned!(ctx, key);
            hg_generation_lease
                .try_add_put_lease(&key)
                .or_else(|_| Ok(false))
                .and_then({
                    cloned!(bcs_id, bonsai_hg_mapping, hg_generation_lease, repo);
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
                                    None => hg_generation_lease
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
        self.hg_generation_lease.release_lease(&key, put_success)
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
            .timed(move |stats, _| {
                STATS::generate_hg_from_bonsai_single_latency_ms
                    .add_value(stats.completion_time.as_millis_unchecked() as i64);
                Ok(())
            })
    }

    pub fn get_hg_from_bonsai_changeset_with_impl(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
        generated_commit_num: usize,
    ) -> impl Future<Item = (HgChangesetId, usize), Error = Error> + Send {
        fn create_hg_from_bonsai_changeset(
            ctx: CoreContext,
            repo: &BlobRepo,
            bcs_id: ChangesetId,
            generated_commit_num: usize,
        ) -> BoxFuture<(HgChangesetId, usize), Error> {
            repo.fetch(ctx.clone(), &bcs_id)
                .and_then({
                    cloned!(ctx, repo);
                    move |bcs| {
                        let parents_futs = bcs
                            .parents()
                            .map(|p_bcs_id| {
                                repo.get_hg_from_bonsai_changeset_with_impl(
                                    ctx.clone(),
                                    p_bcs_id,
                                    generated_commit_num + 1,
                                )
                                .and_then({
                                    cloned!(ctx, repo);
                                    move |(p_cs_id, generated_commit_num)| {
                                        repo.get_changeset_by_changesetid(ctx, p_cs_id)
                                            .map(move |cs| (cs, generated_commit_num))
                                    }
                                })
                            })
                            .collect::<Vec<_>>();
                        future::join_all(parents_futs)
                        // fetch parents
                        .and_then({
                            cloned!(ctx, bcs, repo);
                            move |parents_with_generated_commit_num| {
                                let mut parents_gen_num = 0;
                                let parents: Vec<_> = parents_with_generated_commit_num.into_iter()
                                    .map(|(p, generated_commit_num)|{
                                        parents_gen_num += generated_commit_num;
                                        p
                                    })
                                    .collect();

                                repo.take_hg_generation_lease(ctx.clone(), bcs_id.clone())
                                    .and_then(move |maybe_hg_cs_id| {
                                        match maybe_hg_cs_id {
                                            Some(hg_cs_id) => {
                                                future::ok((hg_cs_id, parents_gen_num)).left_future()
                                            }
                                            None => {
                                                // We have the lease
                                                STATS::generate_hg_from_bonsai_changeset.add_value(1);
                                                repo.generate_hg_changeset(ctx, bcs_id, bcs, parents)
                                                    .map(move |hg_cs_id| (hg_cs_id, parents_gen_num + 1))
                                                    .then(move |res| {
                                                        repo.release_hg_generation_lease(bcs_id, res.is_ok())
                                                            .then(move |_| res)
                                                    })
                                                    .right_future()
                                            }
                                        }
                                    })
                            }
                        })
                    }
                })
                .boxify()
        }

        self.bonsai_hg_mapping
            .get_hg_from_bonsai(ctx.clone(), self.repoid, bcs_id)
            .and_then({
                let repo = self.clone();
                move |cs_id| match cs_id {
                    Some(cs_id) => future::ok((cs_id, generated_commit_num)).left_future(),
                    None => {
                        create_hg_from_bonsai_changeset(ctx, &repo, bcs_id, generated_commit_num)
                            .right_future()
                    }
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
        self.upload_to_blobstore(ctx, &repo.blobstore, &repo.logger)
    }

    pub(crate) fn upload_to_blobstore(
        self,
        ctx: CoreContext,
        blobstore: &RepoBlobstore,
        logger: &Logger,
    ) -> Result<(HgNodeHash, BoxFuture<(HgBlobEntry, RepoPath), Error>)> {
        STATS::upload_hg_tree_entry.add_value(1);
        let UploadHgTreeEntry {
            upload_node_id,
            contents,
            p1,
            p2,
            path,
        } = self;

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
        match self {
            UploadHgFileContents::ContentUploaded(cbmeta) => {
                let upload_fut = future::ok(());
                let compute_fut = Self::compute(ctx, cbmeta.clone(), repo, p1, p2);
                let cbinfo = ContentBlobInfo { path, meta: cbmeta };
                (cbinfo, Either::A(upload_fut), Either::A(compute_fut))
            }
            UploadHgFileContents::RawBytes(raw_content) => {
                let node_id = Self::node_id(raw_content.clone(), p1, p2);
                let f = File::new(raw_content, p1, p2);
                let metadata = f.metadata();

                let copy_from = match f.copied_from() {
                    Ok(copy_from) => copy_from,
                    // XXX error out if copy-from information couldn't be read?
                    Err(_err) => None,
                };
                // Upload the contents separately (they'll be used for bonsai changesets as well).
                let contents = f.file_contents();
                let size = contents.size() as u64;
                // Get alias of raw file contents
                // TODO(anastasiyaz) T33391519 case with file renaming
                let alias_key = get_sha256_alias(&contents.as_bytes());
                let contents_blob = contents.into_blob();
                let cbinfo = ContentBlobInfo {
                    path: path.clone(),
                    meta: ContentBlobMeta {
                        id: *contents_blob.id(),
                        copy_from,
                    },
                };

                let upload_fut = repo
                    .upload_blob(ctx, contents_blob, alias_key)
                    .map(|_content_id| ())
                    .timed({
                        let logger = repo.logger.clone();
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
                let compute_fut = future::ok((node_id, metadata, size));

                (cbinfo, Either::B(upload_fut), Either::B(compute_fut))
            }
        }
    }

    fn compute(
        ctx: CoreContext,
        cbmeta: ContentBlobMeta,
        repo: &BlobRepo,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
    ) -> impl Future<Item = (HgFileNodeId, Bytes, u64), Error = Error> {
        // Computing the file node hash requires fetching the blob and gluing it together with the
        // metadata.
        repo.fetch(ctx, &cbmeta.id).map(move |file_contents| {
            let size = file_contents.size() as u64;
            let mut metadata = Vec::new();
            File::generate_metadata(cbmeta.copy_from.as_ref(), &file_contents, &mut metadata)
                .expect("Vec::write_all should never fail");

            let file_bytes = file_contents.into_bytes();

            // XXX this is just a hash computation, so it shouldn't require a copy
            let raw_content = [&metadata[..], &file_bytes[..]].concat();
            let node_id = Self::node_id(raw_content, p1, p2);
            (node_id, Bytes::from(metadata), size)
        })
    }

    #[inline]
    fn node_id<B: Into<Bytes>>(
        raw_content: B,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
    ) -> HgFileNodeId {
        let raw_content = raw_content.into();
        HgFileNodeId::new(
            HgBlobNode::new(
                raw_content,
                p1.map(HgFileNodeId::into_nodehash),
                p2.map(HgFileNodeId::into_nodehash),
            )
            .nodeid(),
        )
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
        let logger = repo.logger.clone();

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
            logger: self.logger.clone(),
            bookmarks: self.bookmarks.clone(),
            blobstore: self.blobstore.clone(),
            filenodes: self.filenodes.clone(),
            changesets: self.changesets.clone(),
            bonsai_hg_mapping: self.bonsai_hg_mapping.clone(),
            repoid: self.repoid.clone(),
            changeset_fetcher_factory: self.changeset_fetcher_factory.clone(),
            hg_generation_lease: self.hg_generation_lease.clone(),
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
