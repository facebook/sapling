// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::From;
use std::iter::FromIterator;
use std::mem;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use std::usize;

use bytes::Bytes;
use failure::{Error, FutureFailureErrorExt, FutureFailureExt, Result, prelude::*};
use futures::{Async, IntoFuture, Poll};
use futures::future::{self, loop_fn, ok, Either, Future, Loop};
use futures::stream::{self, FuturesUnordered, Stream};
use futures::sync::oneshot;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_stats::{FutureStats, Timed};
use scribe::ScribeClient;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::{Discard, Drain, Logger};
use stats::Timeseries;
use time_ext::DurationExt;
use uuid::Uuid;

use super::alias::{get_content_id_alias_key, get_sha256_alias, get_sha256_alias_key};
use super::changeset::HgChangesetContent;
use super::changeset_fetcher::{CachingChangesetFetcher, ChangesetFetcher, SimpleChangesetFetcher};
use super::utils::{sort_topological, IncompleteFilenodeInfo, IncompleteFilenodes};
use blobstore::{new_cachelib_blobstore, new_memcache_blobstore, Blobstore, EagerMemblob,
                MemWritesBlobstore, PrefixBlobstore};
use bonsai_generation::{create_bonsai_changeset_object, save_bonsai_changeset_object};
use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiHgMappingEntry, BonsaiOrHgChangesetIds,
                        CachingBonsaiHgMapping, SqlBonsaiHgMapping};
use bookmarks::{self, Bookmark, BookmarkPrefix, Bookmarks};
use cachelib;
use changesets::{CachingChangests, ChangesetEntry, ChangesetInsert, Changesets, SqlChangesets};
use dbbookmarks::SqlBookmarks;
use delayblob::DelayBlob;
use file::fetch_file_envelope;
use fileblob::Fileblob;
use filenodes::{CachingFilenodes, FilenodeInfo, Filenodes};
use manifoldblob::ThriftManifoldBlob;
use mercurial::file::File;
use mercurial_types::{Changeset, Entry, HgBlob, HgBlobNode, HgChangesetId, HgFileEnvelopeMut,
                      HgFileNodeId, HgManifestEnvelopeMut, HgManifestId, HgNodeHash, HgParents,
                      Manifest, RepoPath, RepositoryId, Type};
use mercurial_types::manifest::Content;
use mononoke_types::{Blob, BlobstoreBytes, BlobstoreValue, BonsaiChangeset, ChangesetId,
                     ContentId, DateTime, FileChange, FileContents, FileType, Generation, MPath,
                     MPathElement, MononokeId, hash::Blake2, hash::Sha256};
use rocksblob::Rocksblob;
use rocksdb;
use sqlfilenodes::{SqlConstructors, SqlFilenodes};

use BlobManifest;
use HgBlobChangeset;
use errors::*;
use file::{fetch_file_content_from_blobstore, fetch_file_content_id_from_blobstore,
           fetch_file_contents, fetch_file_size_from_blobstore, fetch_raw_filenode_bytes,
           fetch_rename_from_blobstore, HgBlobEntry, fetch_file_content_sha256_from_blobstore};
use memory_manifest::MemoryRootManifest;
use post_commit::{self, PostCommitQueue};
use repo_commit::*;

define_stats! {
    prefix = "mononoke.blobrepo";
    get_bonsai_changeset: timeseries(RATE, SUM),
    get_file_content: timeseries(RATE, SUM),
    get_raw_hg_content: timeseries(RATE, SUM),
    get_changesets: timeseries(RATE, SUM),
    get_heads_maybe_stale: timeseries(RATE, SUM),
    changeset_exists: timeseries(RATE, SUM),
    many_changesets_exists: timeseries(RATE, SUM),
    get_changeset_parents: timeseries(RATE, SUM),
    get_changeset_parents_by_bonsai: timeseries(RATE, SUM),
    get_changeset_by_changesetid: timeseries(RATE, SUM),
    get_hg_file_copy_from_blobstore: timeseries(RATE, SUM),
    get_hg_from_bonsai_changeset: timeseries(RATE, SUM),
    get_manifest_by_nodeid: timeseries(RATE, SUM),
    get_root_entry: timeseries(RATE, SUM),
    get_bookmark: timeseries(RATE, SUM),
    get_bookmarks_maybe_stale: timeseries(RATE, SUM),
    get_bonsai_from_hg: timeseries(RATE, SUM),
    update_bookmark_transaction: timeseries(RATE, SUM),
    get_linknode: timeseries(RATE, SUM),
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

/// Making PrefixBlobstore part of every blobstore does two things:
/// 1. It ensures that the prefix applies first, which is important for shared caches like
///    memcache.
/// 2. It ensures that all possible blobrepos use a prefix.
pub type RepoBlobstore = PrefixBlobstore<Arc<Blobstore>>;

/// Arguments for setting up a Manifold blobstore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifoldArgs {
    /// Bucket of the backing Manifold blobstore to connect to
    pub bucket: String,
    /// Prefix to be prepended to all the keys. In prod it should be ""
    pub prefix: String,
    /// Identifies the SQL database to connect to.
    pub db_address: String,
    /// If present, the number of shards to spread filenodes across
    pub filenode_shards: Option<usize>,
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
    postcommit_queue: Arc<PostCommitQueue>,
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
        postcommit_queue: Arc<PostCommitQueue>,
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
            postcommit_queue,
        }
    }

    fn new_with_changeset_fetcher_factory(
        logger: Logger,
        bookmarks: Arc<Bookmarks>,
        blobstore: Arc<Blobstore>,
        filenodes: Arc<Filenodes>,
        changesets: Arc<Changesets>,
        bonsai_hg_mapping: Arc<BonsaiHgMapping>,
        repoid: RepositoryId,
        postcommit_queue: Arc<PostCommitQueue>,
        changeset_fetcher_factory: Arc<Fn() -> Arc<ChangesetFetcher + Send + Sync> + Send + Sync>,
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
            postcommit_queue,
        }
    }

    /// Most local use cases should use new_rocksdb instead. This is only meant for test
    /// fixtures.
    pub fn new_files(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<Self> {
        let blobstore = Fileblob::create(path.join("blobs"))
            .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))?;

        Self::new_local(logger, path, Arc::new(blobstore), repoid)
    }

    pub fn new_rocksdb(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<Self> {
        let options = rocksdb::Options::new().create_if_missing(true);
        let blobstore = Rocksblob::open_with_options(path.join("blobs"), options)
            .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))?;

        Self::new_local(logger, path, Arc::new(blobstore), repoid)
    }

    pub fn new_rocksdb_delayed<F>(
        logger: Logger,
        path: &Path,
        repoid: RepositoryId,
        delay_gen: F,
        get_roundtrips: usize,
        put_roundtrips: usize,
        is_present_roundtrips: usize,
        assert_present_roundtrips: usize,
    ) -> Result<Self>
    where
        F: FnMut(()) -> Duration + 'static + Send + Sync,
    {
        let options = rocksdb::Options::new().create_if_missing(true);
        let blobstore = Rocksblob::open_with_options(path.join("blobs"), options)
            .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
        let blobstore = DelayBlob::new(
            Box::new(blobstore),
            delay_gen,
            get_roundtrips,
            put_roundtrips,
            is_present_roundtrips,
            assert_present_roundtrips,
        );

        Self::new_local(logger, path, Arc::new(blobstore), repoid)
    }

    /// Create a new BlobRepo with purely local state.
    fn new_local(
        logger: Logger,
        path: &Path,
        blobstore: Arc<Blobstore>,
        repoid: RepositoryId,
    ) -> Result<Self> {
        let bookmarks = SqlBookmarks::with_sqlite_path(path.join("books"))
            .chain_err(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;
        let filenodes = SqlFilenodes::with_sqlite_path(path.join("filenodes"))
            .chain_err(ErrorKind::StateOpen(StateOpenError::Filenodes))?;
        let changesets = SqlChangesets::with_sqlite_path(path.join("changesets"))
            .chain_err(ErrorKind::StateOpen(StateOpenError::Changesets))?;
        let bonsai_hg_mapping =
            SqlBonsaiHgMapping::with_sqlite_path(path.join("bonsai_hg_mapping"))
                .chain_err(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))?;

        Ok(Self::new(
            logger,
            Arc::new(bookmarks),
            blobstore,
            Arc::new(filenodes),
            Arc::new(changesets),
            Arc::new(bonsai_hg_mapping),
            repoid,
            Arc::new(post_commit::Discard::new()),
        ))
    }

    // Memblob repos are test repos, and do not have to have a logger. If we're given None,
    // we won't log.
    pub fn new_memblob_empty(
        logger: Option<Logger>,
        blobstore: Option<Arc<Blobstore>>,
    ) -> Result<Self> {
        Ok(Self::new(
            logger.unwrap_or(Logger::root(Discard {}.ignore_res(), o!())),
            Arc::new(SqlBookmarks::with_sqlite_in_memory()?),
            blobstore.unwrap_or_else(|| Arc::new(EagerMemblob::new())),
            Arc::new(SqlFilenodes::with_sqlite_in_memory()
                .chain_err(ErrorKind::StateOpen(StateOpenError::Filenodes))?),
            Arc::new(SqlChangesets::with_sqlite_in_memory()
                .chain_err(ErrorKind::StateOpen(StateOpenError::Changesets))?),
            Arc::new(SqlBonsaiHgMapping::with_sqlite_in_memory()
                .chain_err(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))?),
            RepositoryId::new(0),
            Arc::new(post_commit::Discard::new()),
        ))
    }

    pub fn new_manifold_scribe_commits<C>(
        logger: Logger,
        args: &ManifoldArgs,
        repoid: RepositoryId,
        myrouter_port: u16,
        scribe: C,
    ) -> Result<Self>
    where
        C: ScribeClient + Sync + Send + 'static,
    {
        let mut repo = Self::new_manifold_no_postcommit(logger, args, repoid, myrouter_port)?;
        let category = format!("mononoke_commits");
        repo.postcommit_queue = Arc::new(post_commit::LogToScribe::new(scribe, category));
        Ok(repo)
    }

    pub fn new_manifold_no_postcommit(
        logger: Logger,
        args: &ManifoldArgs,
        repoid: RepositoryId,
        myrouter_port: u16,
    ) -> Result<Self> {
        let bookmarks = SqlBookmarks::with_myrouter(&args.db_address, myrouter_port);

        let blobstore = ThriftManifoldBlob::new(args.bucket.clone())?;
        let blobstore = PrefixBlobstore::new(blobstore, format!("flat/{}", args.prefix));
        let blobstore = new_memcache_blobstore(blobstore, "manifold", args.bucket.as_ref())?;
        let blob_pool = Arc::new(cachelib::get_pool("blobstore-blobs").ok_or(Error::from(
            ErrorKind::MissingCachePool("blobstore-blobs".to_string()),
        ))?);
        let presence_pool =
            Arc::new(cachelib::get_pool("blobstore-presence").ok_or(Error::from(
                ErrorKind::MissingCachePool("blobstore-presence".to_string()),
            ))?);
        let blobstore = Arc::new(new_cachelib_blobstore(blobstore, blob_pool, presence_pool));

        let filenodes = match args.filenode_shards {
            Some(shards) => {
                SqlFilenodes::with_sharded_myrouter(&args.db_address, myrouter_port, shards)
            }
            None => SqlFilenodes::with_myrouter(&args.db_address, myrouter_port),
        };
        let filenodes = CachingFilenodes::new(
            Arc::new(filenodes),
            cachelib::get_pool("filenodes").ok_or(Error::from(ErrorKind::MissingCachePool(
                "filenodes".to_string(),
            )))?,
            "sqlfilenodes",
            &args.db_address,
        );

        let changesets = SqlChangesets::with_myrouter(&args.db_address, myrouter_port);
        let changesets_cache_pool = cachelib::get_pool("changesets").ok_or(Error::from(
            ErrorKind::MissingCachePool("changesets".to_string()),
        ))?;
        let changesets = CachingChangests::new(Arc::new(changesets), changesets_cache_pool.clone());
        let changesets = Arc::new(changesets);

        let bonsai_hg_mapping = SqlBonsaiHgMapping::with_myrouter(&args.db_address, myrouter_port);
        let bonsai_hg_mapping = CachingBonsaiHgMapping::new(
            Arc::new(bonsai_hg_mapping),
            cachelib::get_pool("bonsai_hg_mapping").ok_or(Error::from(
                ErrorKind::MissingCachePool("bonsai_hg_mapping".to_string()),
            ))?,
        );

        let changeset_fetcher_factory = {
            cloned!(blobstore, changesets, repoid);
            move || {
                let res: Arc<ChangesetFetcher + Send + Sync> =
                    Arc::new(CachingChangesetFetcher::new(
                        changesets.clone(),
                        repoid.clone(),
                        changesets_cache_pool.clone(),
                        blobstore.clone(),
                    ));
                res
            }
        };
        Ok(Self::new_with_changeset_fetcher_factory(
            logger,
            Arc::new(bookmarks),
            blobstore,
            Arc::new(filenodes),
            changesets,
            Arc::new(bonsai_hg_mapping),
            repoid,
            Arc::new(post_commit::Discard::new()),
            Arc::new(changeset_fetcher_factory),
        ))
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
            ..
        } = self;

        let postcommit_queue = Arc::new(post_commit::Discard::new());

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
            postcommit_queue,
        )
    }

    fn fetch<K>(&self, key: &K) -> impl Future<Item = K::Value, Error = Error> + Send
    where
        K: MononokeId,
    {
        let blobstore_key = key.blobstore_key();
        self.blobstore
            .get(blobstore_key.clone())
            .and_then(move |blob| {
                blob.ok_or(ErrorKind::MissingTypedKeyEntry(blobstore_key).into())
                    .and_then(|blob| <<K as MononokeId>::Value>::from_blob(blob.into()))
            })
    }

    // this is supposed to be used only from unittest
    pub fn unittest_fetch<K>(&self, key: &K) -> impl Future<Item = K::Value, Error = Error> + Send
    where
        K: MononokeId,
    {
        self.fetch(key)
    }

    fn store<K, V>(&self, value: V) -> impl Future<Item = K, Error = Error> + Send
    where
        V: BlobstoreValue<Key = K>,
        K: MononokeId<Value = V>,
    {
        let blob = value.into_blob();
        let key = *blob.id();
        self.blobstore
            .put(key.blobstore_key(), blob.into())
            .map(move |_| key)
    }

    // this is supposed to be used only from unittest
    pub fn unittest_store<K, V>(&self, value: V) -> impl Future<Item = K, Error = Error> + Send
    where
        V: BlobstoreValue<Key = K>,
        K: MononokeId<Value = V>,
    {
        self.store(value)
    }

    pub fn get_file_content(&self, key: &HgNodeHash) -> BoxFuture<FileContents, Error> {
        STATS::get_file_content.add_value(1);
        fetch_file_content_from_blobstore(&self.blobstore, *key).boxify()
    }

    pub fn get_file_content_by_content_id(
        &self,
        id: ContentId,
    ) -> impl Future<Item = FileContents, Error = Error> {
        fetch_file_contents(&self.blobstore, id)
    }

    pub fn get_file_size(&self, key: &HgFileNodeId) -> impl Future<Item = u64, Error = Error> {
        fetch_file_size_from_blobstore(&self.blobstore, *key)
    }

    pub fn get_file_content_id(
        &self,
        key: &HgFileNodeId,
    ) -> impl Future<Item = ContentId, Error = Error> {
        fetch_file_content_id_from_blobstore(&self.blobstore, *key)
    }

    pub fn get_file_sha256(
        &self,
        content_id: ContentId,
    ) -> impl Future<Item = Sha256, Error = Error> {
        let blobrepo = self.clone();
        cloned!(content_id, self.blobstore);

        // try to get sha256 from blobstore from a blob to avoid calculation
        self.get_alias_content_id_to_sha256(content_id)
            .and_then(move |res| match res {
                Some(file_content_sha256) => Ok(file_content_sha256).into_future().left_future(),
                None => fetch_file_content_sha256_from_blobstore(&blobstore, content_id)
                    .and_then(move |alias| {
                        blobrepo
                            .put_alias_content_id_to_sha256(content_id, alias)
                            .map(move |()| alias)
                    })
                    .right_future(),
            })
    }

    fn put_alias_content_id_to_sha256(
        &self,
        content_id: ContentId,
        alias_content: Sha256,
    ) -> impl Future<Item = (), Error = Error> {
        let alias_key = get_content_id_alias_key(content_id);
        // Contents = alias.sha256.SHA256HASH (BlobstoreBytes)
        let contents = BlobstoreBytes::from_bytes(Bytes::from(alias_content.as_ref()));

        self.upload_blobstore_bytes(alias_key, contents).map(|_| ())
    }

    fn get_alias_content_id_to_sha256(
        &self,
        content_id: ContentId,
    ) -> impl Future<Item = Option<Sha256>, Error = Error> {
        // Ok: Some(value) - found alias blob, None - alias blob nor found (lazy upload)
        // Invalid alias blob content is considered as "Not found"
        // Err: Error from server, does not proceed the opertion further
        let alias_content_id = get_content_id_alias_key(content_id);

        self.blobstore
            .get(alias_content_id.clone())
            .map(|content_key_bytes| {
                content_key_bytes.and_then(|bytes| Sha256::from_bytes(bytes.as_bytes()).ok())
            })
    }

    pub fn upload_file_content_by_alias(
        &self,
        _alias: Sha256,
        raw_file_content: Bytes,
    ) -> impl Future<Item = (), Error = Error> {
        // Get alias of raw file contents
        let alias_key = get_sha256_alias(&raw_file_content);
        // Raw contents = file content only, excluding metadata in the beginning
        let contents = FileContents::Bytes(raw_file_content);
        self.upload_blob(contents.into_blob(), alias_key)
            .map(|_| ())
            .boxify()
    }

    pub fn get_file_content_by_alias(
        &self,
        alias: Sha256,
    ) -> impl Future<Item = FileContents, Error = Error> {
        let blobstore = self.blobstore.clone();

        self.get_file_content_id_by_alias(alias)
            .and_then(move |content_id| fetch_file_contents(&blobstore, content_id))
            .from_err()
    }

    pub fn get_file_content_id_by_alias(
        &self,
        alias: Sha256,
    ) -> impl Future<Item = ContentId, Error = Error> {
        STATS::get_file_content.add_value(1);
        let prefixed_key = get_sha256_alias_key(alias.to_hex().to_string());
        let blobstore = self.blobstore.clone();

        blobstore
            .get(prefixed_key.clone())
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
        content_id: ContentId,
        file_size: u64,
    ) -> impl Future<Item = FileContents, Error = Error> {
        self.get_file_sha256(content_id)
            .and_then(move |alias| File::generate_lfs_file(alias, file_size))
            .map(|bytes| FileContents::Bytes(bytes))
    }

    // TODO: (rain1) T30456231 It should be possible in principle to make the return type a wrapper
    // around a Chain, but it isn't because of API deficiencies in bytes::Buf. See D8412210.

    /// The raw filenode content is crucial for operation like delta application. It is stored in
    /// untouched represenation that came from Mercurial client.
    pub fn get_raw_hg_content(&self, key: &HgNodeHash) -> BoxFuture<HgBlob, Error> {
        STATS::get_raw_hg_content.add_value(1);
        fetch_raw_filenode_bytes(&self.blobstore, *key)
    }

    // Fetches copy data from blobstore instead of from filenodes db. This should be used only
    // during committing.
    pub(crate) fn get_hg_file_copy_from_blobstore(
        &self,
        key: &HgNodeHash,
    ) -> BoxFuture<Option<(RepoPath, HgNodeHash)>, Error> {
        STATS::get_hg_file_copy_from_blobstore.add_value(1);
        fetch_rename_from_blobstore(&self.blobstore, *key)
            .map(|rename| rename.map(|(path, hash)| (RepoPath::FilePath(path), hash)))
            .boxify()
    }

    pub fn get_changesets(&self) -> BoxStream<HgNodeHash, Error> {
        STATS::get_changesets.add_value(1);
        HgBlobChangesetStream {
            repo: self.clone(),
            state: BCState::Idle,
            heads: self.get_heads_maybe_stale().boxify(),
            seen: HashSet::new(),
        }.boxify()
    }

    /// Heads maybe read from replica, so they may be out of date
    pub fn get_heads_maybe_stale(&self) -> impl Stream<Item = HgNodeHash, Error = Error> {
        STATS::get_heads_maybe_stale.add_value(1);
        self.bookmarks
            .list_by_prefix_maybe_stale(&BookmarkPrefix::empty(), &self.repoid)
            .and_then({
                let repo = self.clone();
                move |(_, cs)| {
                    repo.get_hg_from_bonsai_changeset(cs)
                        .map(|cs| cs.into_nodehash())
                }
            })
    }

    // TODO(stash): make it accept ChangesetId
    pub fn changeset_exists(&self, changesetid: &HgChangesetId) -> BoxFuture<bool, Error> {
        STATS::changeset_exists.add_value(1);
        let changesetid = changesetid.clone();
        let repo = self.clone();
        let repoid = self.repoid.clone();

        self.get_bonsai_from_hg(&changesetid)
            .and_then(move |maybebonsai| match maybebonsai {
                Some(bonsai) => repo.changesets
                    .get(repoid, bonsai)
                    .map(|res| res.is_some())
                    .left_future(),
                None => Ok(false).into_future().right_future(),
            })
            .boxify()
    }

    pub fn many_changesets_exists(
        &self,
        changesetids: &[&HgChangesetId],
    ) -> BoxFuture<Vec<HgChangesetId>, Error> {
        STATS::many_changesets_exists.add_value(1);
        let param = BonsaiOrHgChangesetIds::Hg(Vec::from_iter(changesetids.iter().map(|cs| **cs)));

        self.bonsai_hg_mapping.get(self.repoid, param)
            .map(|entries| entries.into_iter().map(|entry| entry.hg_cs_id).collect())
            // TODO(stash, luk): T37303879 also need to check that entries exist in changeset table
            .boxify()
    }

    // TODO(stash): make it accept ChangesetId
    pub fn get_changeset_parents(
        &self,
        changesetid: &HgChangesetId,
    ) -> BoxFuture<Vec<HgChangesetId>, Error> {
        STATS::get_changeset_parents.add_value(1);
        let changesetid = *changesetid;
        let repo = self.clone();

        self.get_bonsai_cs_entry_or_fail(changesetid)
            .map(|bonsai| bonsai.parents)
            .and_then({
                cloned!(repo);
                move |bonsai_parents| {
                    future::join_all(
                        bonsai_parents.into_iter().map(move |bonsai_parent| {
                            repo.get_hg_from_bonsai_changeset(bonsai_parent)
                        }),
                    )
                }
            })
            .boxify()
    }

    pub fn get_changeset_parents_by_bonsai(
        &self,
        cs: &ChangesetId,
    ) -> impl Future<Item = Vec<ChangesetId>, Error = Error> {
        STATS::get_changeset_parents_by_bonsai.add_value(1);
        let changesetid = *cs;
        let repo = self.clone();
        let repoid = self.repoid.clone();

        repo.changesets
            .get(repoid, changesetid)
            .and_then(move |maybe_bonsai| {
                maybe_bonsai.ok_or(ErrorKind::BonsaiNotFound(changesetid).into())
            })
            .map(|bonsai| bonsai.parents)
    }

    fn get_bonsai_cs_entry_or_fail(
        &self,
        changesetid: HgChangesetId,
    ) -> impl Future<Item = ChangesetEntry, Error = Error> {
        let repoid = self.repoid.clone();
        let changesets = self.changesets.clone();

        self.get_bonsai_from_hg(&changesetid)
            .and_then(move |maybebonsai| {
                maybebonsai.ok_or(ErrorKind::BonsaiMappingNotFound(changesetid).into())
            })
            .and_then(move |bonsai| {
                changesets
                    .get(repoid, bonsai)
                    .and_then(move |maybe_bonsai| {
                        maybe_bonsai.ok_or(ErrorKind::BonsaiNotFound(bonsai).into())
                    })
            })
    }

    pub fn get_changeset_by_changesetid(
        &self,
        changesetid: &HgChangesetId,
    ) -> BoxFuture<HgBlobChangeset, Error> {
        STATS::get_changeset_by_changesetid.add_value(1);
        let chid = changesetid.clone();
        HgBlobChangeset::load(&self.blobstore, &chid)
            .and_then(move |cs| cs.ok_or(ErrorKind::ChangesetMissing(chid).into()))
            .boxify()
    }

    pub fn get_manifest_by_nodeid(
        &self,
        manifestid: &HgManifestId,
    ) -> BoxFuture<Box<Manifest + Sync>, Error> {
        STATS::get_manifest_by_nodeid.add_value(1);
        BlobManifest::load(&self.blobstore, &manifestid)
            .and_then({
                let manifestid = *manifestid;
                move |mf| mf.ok_or(ErrorKind::ManifestMissing(manifestid).into())
            })
            .map(|m| m.boxed())
            .boxify()
    }

    pub fn get_root_entry(&self, manifestid: &HgManifestId) -> Box<Entry + Sync> {
        STATS::get_root_entry.add_value(1);
        Box::new(HgBlobEntry::new_root(self.blobstore.clone(), *manifestid))
    }

    pub fn get_bookmark(&self, name: &Bookmark) -> BoxFuture<Option<HgChangesetId>, Error> {
        STATS::get_bookmark.add_value(1);
        self.bookmarks
            .get(name, &self.repoid)
            .and_then({
                let repo = self.clone();
                move |cs_opt| match cs_opt {
                    None => future::ok(None).left_future(),
                    Some(cs) => repo.get_hg_from_bonsai_changeset(cs)
                        .map(|cs| Some(cs))
                        .right_future(),
                }
            })
            .boxify()
    }

    pub fn get_bonsai_bookmark(&self, name: &Bookmark) -> BoxFuture<Option<ChangesetId>, Error> {
        STATS::get_bookmark.add_value(1);
        self.bookmarks.get(name, &self.repoid)
    }

    /// Heads maybe read from replica, so they may be out of date. Prefer to use this method
    /// over `get_bookmarks` unless you need the most up-to-date bookmarks
    pub fn get_bookmarks_maybe_stale(&self) -> BoxStream<(Bookmark, HgChangesetId), Error> {
        STATS::get_bookmarks_maybe_stale.add_value(1);
        self.bookmarks
            .list_by_prefix_maybe_stale(&BookmarkPrefix::empty(), &self.repoid)
            .and_then({
                let repo = self.clone();
                move |(bm, cs)| {
                    repo.get_hg_from_bonsai_changeset(cs)
                        .map(move |cs| (bm, cs))
                }
            })
            .boxify()
    }

    pub fn update_bookmark_transaction(&self) -> Box<bookmarks::Transaction> {
        STATS::update_bookmark_transaction.add_value(1);
        self.bookmarks.create_transaction(&self.repoid)
    }

    pub fn get_linknode(
        &self,
        path: &RepoPath,
        node: &HgNodeHash,
    ) -> BoxFuture<HgChangesetId, Error> {
        STATS::get_linknode.add_value(1);
        self.get_filenode(path, node)
            .map(|filenode| filenode.linknode)
            .boxify()
    }

    pub fn get_filenode(
        &self,
        path: &RepoPath,
        node: &HgNodeHash,
    ) -> BoxFuture<FilenodeInfo, Error> {
        let path = path.clone();
        let node = HgFileNodeId::new(*node);
        self.filenodes
            .get_filenode(&path, &node, &self.repoid)
            .and_then({
                move |filenode| filenode.ok_or(ErrorKind::MissingFilenode(path, node).into())
            })
            .boxify()
    }

    pub fn get_all_filenodes(&self, path: RepoPath) -> BoxFuture<Vec<FilenodeInfo>, Error> {
        STATS::get_all_filenodes.add_value(1);
        self.filenodes.get_all_filenodes(&path, &self.repoid)
    }

    pub fn get_bonsai_from_hg(
        &self,
        hg_cs_id: &HgChangesetId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        STATS::get_bonsai_from_hg.add_value(1);
        self.bonsai_hg_mapping
            .get_bonsai_from_hg(self.repoid, *hg_cs_id)
    }

    pub fn get_bonsai_changeset(
        &self,
        bonsai_cs_id: ChangesetId,
    ) -> BoxFuture<BonsaiChangeset, Error> {
        STATS::get_bonsai_changeset.add_value(1);
        self.fetch(&bonsai_cs_id).boxify()
    }

    // TODO(stash): make it accept ChangesetId
    pub fn get_generation_number(
        &self,
        cs: &HgChangesetId,
    ) -> impl Future<Item = Option<Generation>, Error = Error> {
        STATS::get_generation_number.add_value(1);
        let repo = self.clone();
        let repoid = self.repoid.clone();

        self.get_bonsai_from_hg(&cs)
            .and_then(move |maybebonsai| match maybebonsai {
                Some(bonsai) => repo.changesets
                    .get(repoid, bonsai)
                    .map(|res| res.map(|res| Generation::new(res.gen)))
                    .left_future(),
                None => Ok(None).into_future().right_future(),
            })
    }

    // TODO(stash): rename to get_generation_number
    pub fn get_generation_number_by_bonsai(
        &self,
        cs: &ChangesetId,
    ) -> impl Future<Item = Option<Generation>, Error = Error> {
        STATS::get_generation_number_by_bonsai.add_value(1);
        let repo = self.clone();
        let repoid = self.repoid.clone();
        repo.changesets
            .get(repoid, *cs)
            .map(|res| res.map(|res| Generation::new(res.gen)))
    }

    pub fn get_changeset_fetcher(&self) -> Arc<ChangesetFetcher> {
        (self.changeset_fetcher_factory)()
    }

    fn upload_blobstore_bytes(
        &self,
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

        self.blobstore.put(key.clone(), contents).timed({
            let logger = self.logger.clone();
            move |stats, result| {
                if result.is_ok() {
                    log_upload_stats(logger, key, "blob uploaded", stats)
                }
                Ok(())
            }
        })
    }

    pub fn upload_blob<Id>(
        &self,
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
            self.upload_blobstore_bytes(alias_key, contents)
        };

        // Upload {blobstore_key: blob_contents}
        let blobstore_key_operation =
            self.upload_blobstore_bytes(blobstore_key, blob_contents.clone());

        blobstore_key_operation
            .join(alias_key_operation)
            .map(move |((), ())| id)
    }

    pub fn upload_alias_to_file_content_id(
        &self,
        alias: Sha256,
        content_id: ContentId,
    ) -> impl Future<Item = (), Error = Error> + Send {
        self.upload_blobstore_bytes(
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
                let parent = parent.into_nodehash();
                cloned!(change, path);
                fetch_file_envelope(&store, parent)
                    .map(move |parent_envelope| {
                        if parent_envelope.content_id() == change.content_id()
                            && change.copy_from().is_none()
                        {
                            Some((
                                HgBlobEntry::new(
                                    store,
                                    path.basename().clone(),
                                    parent,
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
        }.and_then({
            let repo = self.clone();
            let change = change.cloned();
            let path = path.clone();

            move |maybe_entry| match maybe_entry {
                None => repo.store_file_change(p1, p2, &path, change.as_ref())
                    .right_future(),
                _ => future::ok(maybe_entry).left_future(),
            }
        })
    }

    pub fn store_file_change(
        &self,
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
                    Some((path, bcs_id)) => self.get_hg_from_bonsai_changeset(*bcs_id)
                        .and_then({
                            cloned!(repo);
                            move |cs_id| repo.get_changeset_by_changesetid(&cs_id)
                        })
                        .and_then({
                            cloned!(repo, path);
                            move |cs| repo.find_file_in_manifest(&path, *cs.manifestid())
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
                    cloned!(repo, path, change);
                    move |copy_from| {
                        let upload_entry = UploadHgFileEntry {
                            upload_node_id: UploadHgNodeHash::Generate,
                            contents: UploadHgFileContents::ContentUploaded(ContentBlobMeta {
                                id: *change.content_id(),
                                copy_from: copy_from.clone().map(|(p, h)| (p, h.into_nodehash())),
                            }),
                            file_type: change.file_type(),
                            p1: p1.clone().map(|h| h.into_nodehash()),
                            p2: p2.clone().map(|h| h.into_nodehash()),
                            path: path.clone(),
                        };
                        let upload_fut = match upload_entry.upload(&repo) {
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
        parent_mf_id: &HgManifestId,
        child_mf_id: &HgManifestId,
        path: MPath,
    ) -> impl Future<Item = bool, Error = Error> {
        let repo = self.clone();
        let child_mf_id = child_mf_id.clone();
        self.get_manifest_by_nodeid(&parent_mf_id)
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
                                        .get_content()
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
                                    let check_fut =
                                        repo.find_path_in_manifest(fullpath, child_mf_id.clone())
                                            .map(|content| content.is_some());
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
        path: Option<MPath>,
        manifest: HgManifestId,
    ) -> impl Future<Item = Option<Content>, Error = Error> + Send {
        // single fold step, converts path elemnt in content to content, if any
        fn find_content_in_content(
            content: BoxFuture<Option<Content>, Error>,
            path_element: MPathElement,
        ) -> BoxFuture<Option<Content>, Error> {
            content
                .and_then(move |content| match content {
                    None => future::ok(None).left_future(),
                    Some(Content::Tree(manifest)) => match manifest.lookup(&path_element) {
                        None => future::ok(None).left_future(),
                        Some(entry) => entry.get_content().map(Some).right_future(),
                    },
                    Some(_) => future::ok(None).left_future(),
                })
                .boxify()
        }

        self.get_manifest_by_nodeid(&manifest)
            .and_then(move |manifest| {
                let content_init = future::ok(Some(Content::Tree(manifest))).boxify();
                match path {
                    None => content_init,
                    Some(path) => path.into_iter().fold(content_init, find_content_in_content),
                }
            })
    }

    pub fn find_file_in_manifest(
        &self,
        path: &MPath,
        manifest: HgManifestId,
    ) -> impl Future<Item = Option<(FileType, HgFileNodeId)>, Error = Error> + Send {
        let (dirname, basename) = path.split_dirname();
        self.find_path_in_manifest(dirname, manifest).map({
            let basename = basename.clone();
            move |content| match content {
                None => None,
                Some(Content::Tree(manifest)) => match manifest.lookup(&basename) {
                    None => None,
                    Some(entry) => if let Type::File(t) = entry.get_type() {
                        Some((t, HgFileNodeId::new(entry.get_hash().into_nodehash())))
                    } else {
                        None
                    },
                },
                Some(_) => None,
            }
        })
    }

    pub fn get_manifest_from_bonsai(
        &self,
        bcs: BonsaiChangeset,
        manifest_p1: Option<&HgManifestId>,
        manifest_p2: Option<&HgManifestId>,
    ) -> BoxFuture<(HgManifestId, IncompleteFilenodes), Error> {
        let p1 = manifest_p1.map(|id| id.into_nodehash());
        let p2 = manifest_p2.map(|id| id.into_nodehash());
        MemoryRootManifest::new(
            self.clone(),
            IncompleteFilenodes::new(),
            p1.as_ref(),
            p2.as_ref(),
        ).and_then({
            let repo = self.clone();
            let manifest_p1 = manifest_p1.cloned();
            let manifest_p2 = manifest_p2.cloned();
            move |memory_manifest| {
                let memory_manifest = Arc::new(memory_manifest);
                let incomplete_filenodes = memory_manifest.get_incomplete_filenodes();
                let mut futures = Vec::new();

                for (path, entry) in bcs.file_changes() {
                    cloned!(path, memory_manifest, incomplete_filenodes);
                    let p1 = manifest_p1
                        .map(|manifest| {
                            repo.find_file_in_manifest(&path, manifest)
                                .map(|o| o.map(|(_, x)| x))
                        })
                        .into_future();
                    let p2 = manifest_p2
                        .map(|manifest| {
                            repo.find_file_in_manifest(&path, manifest)
                                .map(|o| o.map(|(_, x)| x))
                        })
                        .into_future();
                    let future = (p1, p2)
                        .into_future()
                        .and_then({
                            let entry = entry.cloned();
                            cloned!(repo, path);
                            move |(p1, p2)| {
                                repo.store_file_change_or_reuse(
                                    p1.and_then(|x| x),
                                    p2.and_then(|x| x),
                                    &path,
                                    entry.as_ref(),
                                )
                            }
                        })
                        .and_then(move |entry| match entry {
                            None => memory_manifest.change_entry(&path, None),
                            Some((entry, node_infos)) => {
                                for node_info in node_infos {
                                    incomplete_filenodes.add(node_info);
                                }
                                memory_manifest.change_entry(&path, Some(entry))
                            }
                        });
                    futures.push(future);
                }

                future::join_all(futures)
                    .and_then({
                        let memory_manifest = memory_manifest.clone();
                        move |_| memory_manifest.resolve_trivial_conflicts()
                    })
                    .and_then(move |_| memory_manifest.save())
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
        bcs_id: ChangesetId,
    ) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
        STATS::get_hg_from_bonsai_changeset.add_value(1);
        fn create_hg_from_bonsai_changeset(
            repo: &BlobRepo,
            bcs_id: ChangesetId,
        ) -> BoxFuture<HgChangesetId, Error> {
            repo.fetch(&bcs_id)
                .and_then({
                    cloned!(repo);
                    move |bcs| {
                        let parents_futs = bcs.parents()
                            .map(|p_bcs_id| {
                                repo.get_hg_from_bonsai_changeset(*p_bcs_id).and_then({
                                    cloned!(repo);
                                    move |p_cs_id| repo.get_changeset_by_changesetid(&p_cs_id)
                                })
                            })
                            .collect::<Vec<_>>();
                        future::join_all(parents_futs)
                        // fetch parents
                        .and_then({
                            cloned!(bcs, repo);
                            move |parents| {
                                let mut parents = parents.into_iter();
                                let p0 = parents.next();
                                let p1 = parents.next();

                                let p0_hash = p0.as_ref().map(|p0| p0.get_changeset_id());
                                let p1_hash = p1.as_ref().map(|p1| p1.get_changeset_id());

                                let mf_p0 = p0.map(|p| *p.manifestid());
                                let mf_p1 = p1.map(|p| *p.manifestid());

                                assert!(
                                    parents.next().is_none(),
                                    "more than 2 parents are not supported by hg"
                                );
                                let hg_parents = HgParents::new(
                                    p0_hash.map(|h| h.into_nodehash()).as_ref(),
                                    p1_hash.map(|h| h.into_nodehash()).as_ref(),
                                );
                                repo.get_manifest_from_bonsai(bcs, mf_p0.as_ref(), mf_p1.as_ref())
                                    .map(move |(manifest_id, incomplete_filenodes)| {
                                        (manifest_id, incomplete_filenodes, hg_parents)
                                    })
                            }
                        })
                        // create changeset
                        .and_then({
                            cloned!(repo, bcs);
                            move |(manifest_id, incomplete_filenodes, parents)| {
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
                                let files = {
                                    let mut files: Vec<_> =
                                        bcs.file_changes().map(|(path, _)| path.clone()).collect();
                                    // files must be sorted lexicographically
                                    files.sort_unstable_by(|p0, p1| p0.to_vec().cmp(&p1.to_vec()));
                                    files
                                };
                                let content = HgChangesetContent::new_from_parts(
                                    parents,
                                    manifest_id,
                                    metadata,
                                    files,
                                );
                                let cs = try_boxfuture!(HgBlobChangeset::new(content));
                                let cs_id = cs.get_changeset_id();

                                cs.save(repo.blobstore.clone())
                                    .and_then({
                                        cloned!(repo);
                                        move |_| incomplete_filenodes.upload(cs_id, &repo)
                                    })
                                    .and_then({
                                        cloned!(repo);
                                        move |_| repo.bonsai_hg_mapping.add(BonsaiHgMappingEntry {
                                            repo_id: repo.get_repoid(),
                                            hg_cs_id: cs_id,
                                            bcs_id,
                                        })
                                    })
                                    .map(move |_| cs_id)
                                    .boxify()
                            }
                        })
                    }
                })
                .boxify()
        }

        self.bonsai_hg_mapping
            .get_hg_from_bonsai(self.repoid, bcs_id)
            .and_then({
                let repo = self.clone();
                move |cs_id| match cs_id {
                    Some(cs_id) => future::ok(cs_id).left_future(),
                    None => create_hg_from_bonsai_changeset(&repo, bcs_id).right_future(),
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
        repo: &BlobRepo,
    ) -> Result<(HgNodeHash, BoxFuture<(HgBlobEntry, RepoPath), Error>)> {
        self.upload_to_blobstore(&repo.blobstore, &repo.logger)
    }

    pub(crate) fn upload_to_blobstore(
        self,
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

        let computed_node_id = HgBlobNode::new(contents.clone(), p1.as_ref(), p2.as_ref()).nodeid();
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
            .put(blobstore_key, envelope_blob.into())
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
        repo: &BlobRepo,
        p1: Option<HgNodeHash>,
        p2: Option<HgNodeHash>,
        path: MPath,
    ) -> (
        ContentBlobInfo,
        // The future that does the upload and the future that computes the node ID/metadata are
        // split up to allow greater parallelism.
        impl Future<Item = (), Error = Error> + Send,
        impl Future<Item = (HgNodeHash, Bytes, u64), Error = Error> + Send,
    ) {
        match self {
            UploadHgFileContents::ContentUploaded(cbmeta) => {
                let upload_fut = future::ok(());
                let compute_fut = Self::compute(cbmeta.clone(), repo, p1, p2);
                let cbinfo = ContentBlobInfo { path, meta: cbmeta };
                (cbinfo, Either::A(upload_fut), Either::A(compute_fut))
            }
            UploadHgFileContents::RawBytes(raw_content) => {
                let node_id = Self::node_id(raw_content.clone(), p1.as_ref(), p2.as_ref());
                let f = File::new(raw_content, p1.as_ref(), p2.as_ref());
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

                let upload_fut = repo.upload_blob(contents_blob, alias_key)
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
        cbmeta: ContentBlobMeta,
        repo: &BlobRepo,
        p1: Option<HgNodeHash>,
        p2: Option<HgNodeHash>,
    ) -> impl Future<Item = (HgNodeHash, Bytes, u64), Error = Error> {
        // Computing the file node hash requires fetching the blob and gluing it together with the
        // metadata.
        repo.fetch(&cbmeta.id).map(move |file_contents| {
            let size = file_contents.size() as u64;
            let mut metadata = Vec::new();
            File::generate_metadata(cbmeta.copy_from.as_ref(), &file_contents, &mut metadata)
                .expect("Vec::write_all should never fail");

            let file_bytes = file_contents.into_bytes();

            // XXX this is just a hash computation, so it shouldn't require a copy
            let raw_content = [&metadata[..], &file_bytes[..]].concat();
            let node_id = Self::node_id(raw_content, p1.as_ref(), p2.as_ref());
            (node_id, Bytes::from(metadata), size)
        })
    }

    #[inline]
    fn node_id<B: Into<Bytes>>(
        raw_content: B,
        p1: Option<&HgNodeHash>,
        p2: Option<&HgNodeHash>,
    ) -> HgNodeHash {
        let raw_content = raw_content.into();
        HgBlobNode::new(raw_content, p1, p2).nodeid()
    }
}

/// Context for uploading a Mercurial file entry.
pub struct UploadHgFileEntry {
    pub upload_node_id: UploadHgNodeHash,
    pub contents: UploadHgFileContents,
    pub file_type: FileType,
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub path: MPath,
}

impl UploadHgFileEntry {
    pub fn upload(
        self,
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

        let (cbinfo, content_upload, compute_fut) = contents.execute(repo, p1, p2, path.clone());
        let content_id = cbinfo.meta.id;

        let blobstore = repo.blobstore.clone();
        let logger = repo.logger.clone();

        let envelope_upload =
            compute_fut.and_then(move |(computed_node_id, metadata, content_size)| {
                let node_id = match upload_node_id {
                    UploadHgNodeHash::Generate => computed_node_id,
                    UploadHgNodeHash::Supplied(node_id) => node_id,
                    UploadHgNodeHash::Checked(node_id) => {
                        if node_id != computed_node_id {
                            return Either::A(future::err(
                                ErrorKind::InconsistentEntryHash(
                                    RepoPath::FilePath(path),
                                    node_id,
                                    computed_node_id,
                                ).into(),
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

                let blobstore_key = HgFileNodeId::new(node_id).blobstore_key();

                let blob_entry = HgBlobEntry::new(
                    blobstore.clone(),
                    path.basename().clone(),
                    node_id,
                    Type::File(file_type),
                );

                let envelope_upload = blobstore
                    .put(blobstore_key, envelope_blob.into())
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

    fn log_stats(logger: Logger, path: MPath, nodeid: HgNodeHash, phase: &str, stats: FutureStats) {
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
    pub copy_from: Option<(MPath, HgNodeHash)>,
}

pub struct ChangesetMetadata {
    pub user: String,
    pub time: DateTime,
    pub extra: BTreeMap<Vec<u8>, Vec<u8>>,
    pub comments: String,
}

/// This function uploads bonsai changests object to blobstore in parallel, and then does
/// sequential writes to changesets table. Parents of the changesets should already by saved
/// in the repository.
pub fn save_bonsai_changesets(
    bonsai_changesets: Vec<BonsaiChangeset>,
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
        bonsai_object_futs.push(save_bonsai_changeset_object(blobstore.clone(), bcs.clone()));
    }
    let bonsai_objects = bonsai_object_futs.collect();

    // Order of inserting entries in changeset table matters though, so we first need to
    // topologically sort commits.
    let mut bcs_parents = HashMap::new();
    for bcs in bonsai_changesets.values() {
        let parents: Vec<_> = bcs.parents().cloned().collect();
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
                parents: bcs.parents().into_iter().cloned().collect(),
            };
            let pc = post_commit::PreCommitInfo::new(repo.repoid, bcs_id, bcs);
            bonsai_complete_futs.push(complete_changesets.add(completion_record).and_then({
                cloned!(repo);
                move |_| {
                    repo.get_generation_number_by_bonsai(pc.get_changeset_id())
                        .map(move |gen| {
                            pc.complete(gen.expect(
                                "Just inserted changeset has no generation number",
                            ))
                        })
                }
            }));
        }
    }

    let postcommit_queue = repo.postcommit_queue.clone();
    bonsai_objects.and_then(move |_| {
        // T33825303 - build a future that depends on parent futures instead of inserting them
        // one by one.
        loop_fn(
            bonsai_complete_futs.into_iter(),
            move |mut futs| match futs.next() {
                Some(fut) => fut.and_then({
                    cloned!(postcommit_queue);
                    move |pc| {
                        postcommit_queue
                            .queue_commit(pc)
                            .map(|_| Loop::Continue(futs))
                    }
                }).left_future(),
                None => ok(Loop::Break(())).right_future(),
            },
        )
    })
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
}

impl CreateChangeset {
    pub fn create(self, repo: &BlobRepo, mut scuba_logger: ScubaSampleBuilder) -> ChangesetHandle {
        STATS::create_changeset.add_value(1);
        // This is used for logging, so that we can tie up all our pieces without knowing about
        // the final commit hash
        let uuid = Uuid::new_v4();
        scuba_logger.add("changeset_uuid", format!("{}", uuid));

        let entry_processor = UploadEntries::new(
            repo.blobstore.clone(),
            repo.repoid.clone(),
            scuba_logger.clone(),
        );
        let (signal_parent_ready, can_be_parent) = oneshot::channel();
        let expected_nodeid = self.expected_nodeid;

        let upload_entries = process_entries(
            repo.clone(),
            &entry_processor,
            self.root_manifest,
            self.sub_entries,
        ).context("While processing entries");

        let parents_complete = extract_parents_complete(&self.p1, &self.p2);
        let parents_data = handle_parents(scuba_logger.clone(), self.p1, self.p2)
            .context("While waiting for parents to upload data");
        let must_check_case_conflicts = self.must_check_case_conflicts.clone();
        let changeset = {
            let mut scuba_logger = scuba_logger.clone();
            upload_entries
                .join(parents_data)
                .from_err()
                .and_then({
                    cloned!(repo, repo.filenodes, repo.blobstore, mut scuba_logger);
                    let expected_files = self.expected_files;
                    let cs_metadata = self.cs_metadata;

                    move |(
                        (root_manifest, root_hash),
                        (parents, parent_manifest_hashes, bonsai_parents),
                    )| {
                        let files = if let Some(expected_files) = expected_files {
                            STATS::create_changeset_expected_cf.add_value(1);
                            // We are trusting the callee to provide a list of changed files, used
                            // by the import job
                            future::ok(expected_files).boxify()
                        } else {
                            STATS::create_changeset_compute_cf.add_value(1);
                            fetch_parent_manifests(repo.clone(), &parent_manifest_hashes)
                                .and_then(move |(p1_manifest, p2_manifest)| {
                                    compute_changed_files(
                                        &root_manifest,
                                        p1_manifest.as_ref(),
                                        p2_manifest.as_ref(),
                                    )
                                })
                                .boxify()
                        };

                        let p1_mf = parent_manifest_hashes.get(0).cloned();
                        let check_case_conflicts = if must_check_case_conflicts {
                            check_case_conflicts(repo.clone(), root_hash.clone(), p1_mf)
                                .left_future()
                        } else {
                            future::ok(()).right_future()
                        };

                        let changesets = files
                            .join(check_case_conflicts)
                            .and_then(move |(files, ())| {
                                STATS::create_changeset_cf_count.add_value(files.len() as i64);
                                make_new_changeset(parents, root_hash, cs_metadata, files)
                            })
                            .and_then(move |hg_cs| {
                                create_bonsai_changeset_object(
                                    hg_cs.clone(),
                                    parent_manifest_hashes,
                                    bonsai_parents,
                                    repo.clone(),
                                ).map(|bonsai_cs| (hg_cs, bonsai_cs))
                            });

                        changesets
                            .context("While computing changed files")
                            .and_then({
                                move |(blobcs, bonsai_cs)| {
                                    let fut: BoxFuture<
                                        (HgBlobChangeset, BonsaiChangeset),
                                        Error,
                                    > = (move || {
                                        let bonsai_blob = bonsai_cs.clone().into_blob();
                                        let bcs_id = bonsai_blob.id().clone();

                                        let cs_id = blobcs.get_changeset_id().into_nodehash();
                                        let manifest_id = *blobcs.manifestid();

                                        if let Some(expected_nodeid) = expected_nodeid {
                                            if cs_id != expected_nodeid {
                                                return future::err(
                                                    ErrorKind::InconsistentChangesetHash(
                                                        expected_nodeid,
                                                        cs_id,
                                                        blobcs,
                                                    ).into(),
                                                ).boxify();
                                            }
                                        }

                                        scuba_logger
                                            .add("changeset_id", format!("{}", cs_id))
                                            .log_with_msg("Changeset uuid to hash mapping", None);
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
                                        let _ =
                                            signal_parent_ready.send((bcs_id, cs_id, manifest_id));

                                        let bonsai_cs_fut = save_bonsai_changeset_object(
                                            blobstore.clone(),
                                            bonsai_cs.clone(),
                                        );

                                        blobcs
                                            .save(blobstore)
                                            .join(bonsai_cs_fut)
                                            .context("While writing to blobstore")
                                            .join(
                                                entry_processor
                                                    .finalize(filenodes, cs_id)
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
        ChangesetHandle::new_pending(
            can_be_parent.shared(),
            changeset
                .join(parents_complete)
                .and_then({
                    cloned!(repo.bonsai_hg_mapping);
                    move |((hg_cs, bonsai_cs), _)| {
                        let bcs_id = bonsai_cs.get_changeset_id();
                        let bonsai_hg_entry = BonsaiHgMappingEntry {
                            repo_id: repoid.clone(),
                            hg_cs_id: hg_cs.get_changeset_id(),
                            bcs_id,
                        };

                        bonsai_hg_mapping
                            .add(bonsai_hg_entry)
                            .map(move |_| (hg_cs, bonsai_cs))
                            .context("While inserting mapping")
                    }
                })
                .and_then(move |(hg_cs, bonsai_cs)| {
                    let pc = post_commit::PreCommitInfo::new(
                        repoid,
                        bonsai_cs.get_changeset_id(),
                        &bonsai_cs,
                    );
                    let completion_record = ChangesetInsert {
                        repo_id: repo.repoid,
                        cs_id: bonsai_cs.get_changeset_id(),
                        parents: bonsai_cs.parents().into_iter().cloned().collect(),
                    };
                    complete_changesets
                        .add(completion_record)
                        .and_then({
                            cloned!(repo);
                            move |_| {
                                repo.get_generation_number_by_bonsai(pc.get_changeset_id())
                                    .map(move |gen| {
                                        pc.complete(gen.expect(
                                            "Just inserted changeset has no generation number",
                                        ))
                                    })
                                    .and_then(move |pc| repo.postcommit_queue.queue_commit(pc))
                            }
                        })
                        .map(|_| (bonsai_cs, hg_cs))
                        .context("While inserting into changeset table")
                })
                .with_context(move |_| {
                    format!(
                        "While creating Changeset {:?}, uuid: {}",
                        expected_nodeid, uuid
                    )
                })
                .map_err(|e| Error::from(e).compat())
                .timed({
                    move |stats, result| {
                        if result.is_ok() {
                            scuba_logger
                                .add_future_stats(&stats)
                                .log_with_msg("CreateChangeset Finished", None);
                        }
                        Ok(())
                    }
                })
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
            postcommit_queue: self.postcommit_queue.clone(),
        }
    }
}

pub struct HgBlobChangesetStream {
    repo: BlobRepo,
    seen: HashSet<HgNodeHash>,
    heads: BoxStream<HgNodeHash, Error>,
    state: BCState,
}

enum BCState {
    Idle,
    WaitCS(HgNodeHash, BoxFuture<HgBlobChangeset, Error>),
}

impl Stream for HgBlobChangesetStream {
    type Item = HgNodeHash;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Error> {
        use self::BCState::*;

        loop {
            let (ret, state) = match &mut self.state {
                &mut Idle => {
                    if let Some(next) = try_ready!(self.heads.poll()) {
                        let state = if self.seen.insert(next) {
                            // haven't seen before
                            WaitCS(
                                next,
                                self.repo
                                    .get_changeset_by_changesetid(&HgChangesetId::new(next)),
                            )
                        } else {
                            Idle // already done it
                        };

                        // Nothing to report, keep going
                        (None, state)
                    } else {
                        // Finished
                        (Some(None), Idle)
                    }
                }

                &mut WaitCS(ref next, ref mut csfut) => {
                    let cs = try_ready!(csfut.poll());

                    // get current heads stream and replace it with a placeholder
                    let heads = mem::replace(&mut self.heads, stream::empty().boxify());

                    // Add new heads - existing first, then new to get BFS
                    let parents = cs.parents().into_iter();
                    self.heads = heads.chain(stream::iter_ok(parents)).boxify();

                    (Some(Some(*next)), Idle)
                }
            };

            self.state = state;
            if let Some(ret) = ret {
                return Ok(Async::Ready(ret));
            }
        }
    }
}
