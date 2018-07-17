// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{BTreeMap, HashSet};
use std::mem;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::usize;

use bytes::Bytes;
use db::{get_connection_params, InstanceRequirement, ProxyRequirement};
use futures::{Async, IntoFuture, Poll};
use futures::future::{self, Either, Future};
use futures::stream::{self, Stream};
use futures::sync::oneshot;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_stats::{Stats, Timed};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::{Discard, Drain, Logger};
use stats::Timeseries;
use time_ext::DurationExt;
use uuid::Uuid;

use blobstore::{new_memcache_blobstore, Blobstore, EagerMemblob, MemoizedBlobstore,
                PrefixBlobstore};
use bonsai_hg_mapping::{BonsaiHgMapping, CachingBonsaiHgMapping, MysqlBonsaiHgMapping,
                        SqliteBonsaiHgMapping};
use bookmarks::{self, Bookmark, BookmarkPrefix, Bookmarks};
use changesets::{CachingChangests, ChangesetInsert, Changesets, MysqlChangesets, SqliteChangesets};
use dbbookmarks::{MysqlDbBookmarks, SqliteDbBookmarks};
use delayblob::DelayBlob;
use dieselfilenodes::{MysqlFilenodes, SqliteFilenodes, DEFAULT_INSERT_CHUNK_SIZE};
use fileblob::Fileblob;
use filenodes::{CachingFilenodes, FilenodeInfo, Filenodes};
use manifoldblob::ManifoldBlob;
use mercurial::file::File;
use mercurial_types::{Changeset, Entry, HgBlob, HgBlobNode, HgChangesetId, HgFileEnvelopeMut,
                      HgFileNodeId, HgManifestEnvelopeMut, HgManifestId, HgNodeHash, HgParents,
                      Manifest, RepoPath, RepositoryId, Type};
use mercurial_types::manifest::Content;
use mononoke_types::{Blob, BlobstoreValue, BonsaiChangeset, ContentId, DateTime, FileChange,
                     FileContents, FileType, Generation, MPath, MPathElement, MononokeId};
use rocksblob::Rocksblob;
use rocksdb;

use BlobChangeset;
use BlobManifest;
use errors::*;
use file::{fetch_file_content_and_renames_from_blobstore, fetch_raw_filenode_bytes, HgBlobEntry};
use memory_manifest::MemoryRootManifest;
use repo_commit::*;

define_stats! {
    prefix = "mononoke.blobrepo";
    get_file_content: timeseries(RATE, SUM),
    get_raw_hg_content: timeseries(RATE, SUM),
    get_parents: timeseries(RATE, SUM),
    get_file_copy: timeseries(RATE, SUM),
    get_changesets: timeseries(RATE, SUM),
    get_heads: timeseries(RATE, SUM),
    changeset_exists: timeseries(RATE, SUM),
    get_changeset_parents: timeseries(RATE, SUM),
    get_changeset_by_changesetid: timeseries(RATE, SUM),
    get_manifest_by_nodeid: timeseries(RATE, SUM),
    get_root_entry: timeseries(RATE, SUM),
    get_bookmark: timeseries(RATE, SUM),
    get_bookmarks: timeseries(RATE, SUM),
    update_bookmark_transaction: timeseries(RATE, SUM),
    get_linknode: timeseries(RATE, SUM),
    get_all_filenodes: timeseries(RATE, SUM),
    get_generation_number: timeseries(RATE, SUM),
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
    /// Size of the blobstore cache.
    /// Currently we need to set separate cache size for each cache (blobstore, filenodes etc)
    /// TODO(stash): have single cache size for all caches
    pub blobstore_cache_size: usize,
    /// Size of the changesets cache.
    pub changesets_cache_size: usize,
    /// Size of the filenodes cache.
    pub filenodes_cache_size: usize,
    /// Size of the bonsai_hg_mapping cache.
    pub bonsai_hg_mapping_cache_size: usize,
    /// Number of IO threads the blobstore uses.
    pub io_threads: usize,
    /// This is a (hopefully) short term hack to overcome the problem of overloading Manifold.
    /// It limits the number of simultaneous requests that can be sent from a single io thread
    /// If not set then default value is used.
    pub max_concurrent_requests_per_io_thread: usize,
}

pub struct BlobRepo {
    logger: Logger,
    blobstore: RepoBlobstore,
    bookmarks: Arc<Bookmarks>,
    filenodes: Arc<Filenodes>,
    changesets: Arc<Changesets>,
    bonsai_hg_mapping: Arc<BonsaiHgMapping>,
    repoid: RepositoryId,
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
    ) -> Self {
        BlobRepo {
            logger,
            bookmarks,
            blobstore: PrefixBlobstore::new(blobstore, repoid.prefix()),
            filenodes,
            changesets,
            bonsai_hg_mapping,
            repoid,
        }
    }

    /// Most local use cases should use new_rocksdb instead. This is only meant for test
    /// fixtures.
    pub fn new_files(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<Self> {
        let blobstore = Fileblob::create(path.join("blobs"))
            .context(ErrorKind::StateOpen(StateOpenError::Blobstore))?;

        Self::new_local(logger, path, Arc::new(blobstore), repoid)
    }

    pub fn new_rocksdb(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<Self> {
        let options = rocksdb::Options::new().create_if_missing(true);
        let blobstore = Rocksblob::open_with_options(path.join("blobs"), options)
            .context(ErrorKind::StateOpen(StateOpenError::Blobstore))?;

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
            .context(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
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
        let bookmarks = SqliteDbBookmarks::open_or_create(path.join("books").to_string_lossy())
            .context(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;
        let filenodes = SqliteFilenodes::open_or_create(
            path.join("filenodes").to_string_lossy(),
            DEFAULT_INSERT_CHUNK_SIZE,
        ).context(ErrorKind::StateOpen(StateOpenError::Filenodes))?;
        let changesets = SqliteChangesets::open_or_create(
            path.join("changesets").to_string_lossy(),
        ).context(ErrorKind::StateOpen(StateOpenError::Changesets))?;
        let bonsai_hg_mapping =
            SqliteBonsaiHgMapping::open_or_create(path.join("bonsai_hg_mapping").to_string_lossy())
                .context(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))?;

        Ok(Self::new(
            logger,
            Arc::new(bookmarks),
            blobstore,
            Arc::new(filenodes),
            Arc::new(changesets),
            Arc::new(bonsai_hg_mapping),
            repoid,
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
            Arc::new(SqliteDbBookmarks::in_memory()?),
            blobstore.unwrap_or_else(|| Arc::new(EagerMemblob::new())),
            Arc::new(SqliteFilenodes::in_memory()
                .context(ErrorKind::StateOpen(StateOpenError::Filenodes))?),
            Arc::new(SqliteChangesets::in_memory()
                .context(ErrorKind::StateOpen(StateOpenError::Changesets))?),
            Arc::new(SqliteBonsaiHgMapping::in_memory()
                .context(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))?),
            RepositoryId::new(0),
        ))
    }

    pub fn new_manifold(logger: Logger, args: &ManifoldArgs, repoid: RepositoryId) -> Result<Self> {
        // TODO(stash): T28429403 use local region first, fallback to master if not found
        let connection_params = get_connection_params(
            &args.db_address,
            InstanceRequirement::Master,
            None,
            Some(ProxyRequirement::Forbidden),
        )?;
        let bookmarks = MysqlDbBookmarks::open(&connection_params)
            .context(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;

        let blobstore = ManifoldBlob::new_with_prefix(
            args.bucket.clone(),
            &args.prefix,
            args.max_concurrent_requests_per_io_thread,
        );
        let blobstore = new_memcache_blobstore(blobstore, "manifold", args.bucket.as_ref())?;
        let blobstore = MemoizedBlobstore::new(blobstore, usize::MAX, args.blobstore_cache_size);

        let filenodes = MysqlFilenodes::open(&args.db_address, DEFAULT_INSERT_CHUNK_SIZE)
            .context(ErrorKind::StateOpen(StateOpenError::Filenodes))?;
        let filenodes = CachingFilenodes::new(Arc::new(filenodes), args.filenodes_cache_size);

        let changesets = MysqlChangesets::open(&args.db_address)
            .context(ErrorKind::StateOpen(StateOpenError::Changesets))?;
        let changesets = CachingChangests::new(Arc::new(changesets), args.changesets_cache_size);

        let bonsai_hg_mapping = MysqlBonsaiHgMapping::open(&args.db_address)
            .context(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))?;
        let bonsai_hg_mapping = CachingBonsaiHgMapping::new(
            Arc::new(bonsai_hg_mapping),
            args.bonsai_hg_mapping_cache_size,
        );

        Ok(Self::new(
            logger,
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(filenodes),
            Arc::new(changesets),
            Arc::new(bonsai_hg_mapping),
            repoid,
        ))
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
        fetch_file_content_and_renames_from_blobstore(&self.blobstore, *key)
            .map(|contentrename| contentrename.0)
            .boxify()
    }

    // TODO: (rain1) T30456231 It should be possible in principle to make the return type a wrapper
    // around a Chain, but it isn't because of API deficiencies in bytes::Buf. See D8412210.

    /// The raw filenode content is crucial for operation like delta application. It is stored in
    /// untouched represenation that came from Mercurial client.
    pub fn get_raw_hg_content(&self, key: &HgNodeHash) -> BoxFuture<HgBlob, Error> {
        STATS::get_raw_hg_content.add_value(1);
        fetch_raw_filenode_bytes(&self.blobstore, *key)
    }

    pub fn get_parents(&self, path: &RepoPath, node: &HgNodeHash) -> BoxFuture<HgParents, Error> {
        STATS::get_parents.add_value(1);
        let path = path.clone();
        let node = HgFileNodeId::new(*node);
        self.filenodes
            .get_filenode(&path, &node, &self.repoid)
            .and_then({
                move |filenode| {
                    filenode
                        .ok_or(ErrorKind::MissingFilenode(path, node).into())
                        .map(|filenode| {
                            let p1 = filenode.p1.map(|p| p.into_nodehash());
                            let p2 = filenode.p2.map(|p| p.into_nodehash());
                            HgParents::new(p1.as_ref(), p2.as_ref())
                        })
                }
            })
            .boxify()
    }

    pub fn get_file_copy(
        &self,
        path: &RepoPath,
        node: &HgNodeHash,
    ) -> BoxFuture<Option<(RepoPath, HgNodeHash)>, Error> {
        STATS::get_file_copy.add_value(1);
        let path = path.clone();
        let node = HgFileNodeId::new(*node);
        self.filenodes
            .get_filenode(&path, &node, &self.repoid)
            .and_then({
                move |filenode| {
                    filenode
                        .ok_or(ErrorKind::MissingFilenode(path, node).into())
                        .map(|filenode| {
                            filenode
                                .copyfrom
                                .map(|(repo, node)| (repo, node.into_nodehash()))
                        })
                }
            })
            .boxify()
    }

    pub fn get_changesets(&self) -> BoxStream<HgNodeHash, Error> {
        STATS::get_changesets.add_value(1);
        BlobChangesetStream {
            repo: self.clone(),
            state: BCState::Idle,
            heads: self.get_heads().boxify(),
            seen: HashSet::new(),
        }.boxify()
    }

    pub fn get_heads(&self) -> impl Stream<Item = HgNodeHash, Error = Error> {
        STATS::get_heads.add_value(1);
        self.bookmarks
            .list_by_prefix(&BookmarkPrefix::empty(), &self.repoid)
            .map(|(_, cs)| cs.into_nodehash())
    }

    pub fn changeset_exists(&self, changesetid: &HgChangesetId) -> BoxFuture<bool, Error> {
        STATS::changeset_exists.add_value(1);
        self.changesets
            .get(self.repoid, *changesetid)
            .map(|res| res.is_some())
            .boxify()
    }

    pub fn get_changeset_parents(
        &self,
        changesetid: &HgChangesetId,
    ) -> BoxFuture<Vec<HgChangesetId>, Error> {
        STATS::get_changeset_parents.add_value(1);
        let changesetid = *changesetid;
        self.changesets
            .get(self.repoid, changesetid.clone())
            .and_then(move |res| res.ok_or(ErrorKind::ChangesetMissing(changesetid).into()))
            .map(|res| res.parents)
            .boxify()
    }

    pub fn get_changeset_by_changesetid(
        &self,
        changesetid: &HgChangesetId,
    ) -> BoxFuture<BlobChangeset, Error> {
        STATS::get_changeset_by_changesetid.add_value(1);
        let chid = changesetid.clone();
        BlobChangeset::load(&self.blobstore, &chid)
            .and_then(move |cs| cs.ok_or(ErrorKind::ChangesetMissing(chid).into()))
            .boxify()
    }

    pub fn get_manifest_by_nodeid(
        &self,
        nodeid: &HgNodeHash,
    ) -> BoxFuture<Box<Manifest + Sync>, Error> {
        STATS::get_manifest_by_nodeid.add_value(1);
        let nodeid = *nodeid;
        let manifestid = HgManifestId::new(nodeid);
        BlobManifest::load(&self.blobstore, &manifestid)
            .and_then(move |mf| mf.ok_or(ErrorKind::ManifestMissing(nodeid).into()))
            .map(|m| m.boxed())
            .boxify()
    }

    pub fn get_root_entry(&self, manifestid: &HgManifestId) -> Box<Entry + Sync> {
        STATS::get_root_entry.add_value(1);
        Box::new(HgBlobEntry::new_root(self.blobstore.clone(), *manifestid))
    }

    pub fn get_bookmark(&self, name: &Bookmark) -> BoxFuture<Option<HgChangesetId>, Error> {
        STATS::get_bookmark.add_value(1);
        self.bookmarks.get(name, &self.repoid)
    }

    // TODO(stash): rename to get_all_bookmarks()?
    pub fn get_bookmarks(&self) -> BoxStream<(Bookmark, HgChangesetId), Error> {
        STATS::get_bookmarks.add_value(1);
        self.bookmarks
            .list_by_prefix(&BookmarkPrefix::empty(), &self.repoid)
    }

    pub fn update_bookmark_transaction(&self) -> Box<bookmarks::Transaction> {
        STATS::update_bookmark_transaction.add_value(1);
        self.bookmarks.create_transaction(&self.repoid)
    }

    pub fn get_linknode(&self, path: RepoPath, node: &HgNodeHash) -> BoxFuture<HgNodeHash, Error> {
        STATS::get_linknode.add_value(1);
        let node = HgFileNodeId::new(*node);
        self.filenodes
            .get_filenode(&path, &node, &self.repoid)
            .and_then({
                move |filenode| {
                    filenode
                        .ok_or(ErrorKind::MissingFilenode(path, node).into())
                        .map(|filenode| filenode.linknode.into_nodehash())
                }
            })
            .boxify()
    }

    pub fn get_all_filenodes(&self, path: RepoPath) -> BoxFuture<Vec<FilenodeInfo>, Error> {
        STATS::get_all_filenodes.add_value(1);
        self.filenodes.get_all_filenodes(&path, &self.repoid)
    }

    pub fn get_generation_number(
        &self,
        cs: &HgChangesetId,
    ) -> BoxFuture<Option<Generation>, Error> {
        STATS::get_generation_number.add_value(1);
        self.changesets
            .get(self.repoid, *cs)
            .map(|res| res.map(|res| Generation::new(res.gen)))
            .boxify()
    }

    pub fn upload_blob<Id>(&self, blob: Blob<Id>) -> impl Future<Item = Id, Error = Error> + Send
    where
        Id: MononokeId,
    {
        STATS::upload_blob.add_value(1);
        let id = blob.id().clone();
        let blobstore_key = id.blobstore_key();

        fn log_upload_stats(logger: Logger, blobstore_key: String, phase: &str, stats: Stats) {
            trace!(logger, "Upload blob stats";
                "phase" => String::from(phase),
                "blobstore_key" => blobstore_key,
                "poll_count" => stats.poll_count,
                "poll_time_us" => stats.poll_time.as_micros_unchecked(),
                "completion_time_us" => stats.completion_time.as_micros_unchecked(),
            );
        }

        self.blobstore
            .put(blobstore_key.clone(), blob.into())
            .map(move |_| id)
            .timed({
                let logger = self.logger.clone();
                move |stats, result| {
                    if result.is_ok() {
                        log_upload_stats(logger, blobstore_key, "blob uploaded", stats)
                    }
                    Ok(())
                }
            })
    }

    // This is used by tests in memory_manifest.rs
    pub fn get_blobstore(&self) -> RepoBlobstore {
        self.blobstore.clone()
    }

    pub fn get_logger(&self) -> Logger {
        self.logger.clone()
    }

    pub fn store_file_change(
        &self,
        p1: Option<HgNodeHash>,
        p2: Option<HgNodeHash>,
        path: &MPath,
        change: Option<&FileChange>,
    ) -> impl Future<Item = Option<HgBlobEntry>, Error = Error> + Send {
        match change {
            None => Either::A(future::ok(None)),
            Some(change) => {
                let upload_entry = UploadHgFileEntry {
                    upload_node_id: UploadHgNodeHash::Generate,
                    contents: UploadHgFileContents::ContentUploaded(ContentBlobMeta {
                        id: *change.content_id(),
                        // FIXME: need external {ChangesetID -> HgNodeHash} mapping
                        copy_from: None,
                    }),
                    file_type: change.file_type(),
                    p1,
                    p2,
                    path: path.clone(),
                };
                let upload_fut = match upload_entry.upload(self) {
                    Ok((_, upload_fut)) => upload_fut,
                    Err(err) => return Either::A(future::err(err)),
                };
                Either::B(upload_fut.map(|(entry, _)| Some(entry)))
            }
        }
    }

    pub fn find_path_in_manifest(
        &self,
        path: Option<MPath>,
        manifest: HgNodeHash,
    ) -> impl Future<Item = Option<Content>, Error = Error> + Send {
        // single fold step, converts path elemnt in content to content, if any
        fn find_content_in_content(
            content: BoxFuture<Option<Content>, Error>,
            path_element: MPathElement,
        ) -> BoxFuture<Option<Content>, Error> {
            content
                .and_then(move |content| match content {
                    None => Either::A(future::ok(None)),
                    Some(Content::Tree(manifest)) => match manifest.lookup(&path_element) {
                        None => Either::A(future::ok(None)),
                        Some(entry) => Either::B(entry.get_content().map(Some)),
                    },
                    Some(_) => Either::A(future::ok(None)),
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
        manifest: HgNodeHash,
    ) -> impl Future<Item = Option<HgNodeHash>, Error = Error> + Send {
        let (dirname, basename) = path.split_dirname();
        self.find_path_in_manifest(dirname, manifest).map({
            let basename = basename.clone();
            move |content| match content {
                None => None,
                Some(Content::Tree(manifest)) => match manifest.lookup(&basename) {
                    None => None,
                    Some(entry) => if let Type::File(_) = entry.get_type() {
                        Some(entry.get_hash().into_nodehash())
                    } else {
                        None
                    },
                },
                Some(_) => None,
            }
        })
    }

    // TODO(T29283916): Using caching to avoid wasting compute, change this to find the manifest_p1
    // and manifest_p2 from bcs, so that you can remove manifest_p1 and manifest_p2 from the args
    // to this function
    pub fn get_manifest_from_bonsai(
        &self,
        bcs: BonsaiChangeset,
        manifest_p1: Option<&HgNodeHash>,
        manifest_p2: Option<&HgNodeHash>,
    ) -> BoxFuture<HgNodeHash, Error> {
        MemoryRootManifest::new(self.clone(), manifest_p1, manifest_p2)
            .and_then({
                let blobrepo = self.clone();
                let manifest_p1 = manifest_p1.cloned();
                let manifest_p2 = manifest_p2.cloned();
                move |memory_manifest| {
                    let memory_manifest = Arc::new(memory_manifest);
                    let mut futures = Vec::new();

                    for (path, entry) in bcs.file_changes() {
                        let path = path.clone();
                        let memory_manifest = memory_manifest.clone();
                        let p1 = manifest_p1
                            .map(|manifest| blobrepo.find_file_in_manifest(&path, manifest))
                            .into_future();
                        let p2 = manifest_p2
                            .map(|manifest| blobrepo.find_file_in_manifest(&path, manifest))
                            .into_future();
                        let future = p1.join(p2)
                            .and_then({
                                let blobrepo = blobrepo.clone();
                                let path = path.clone();
                                let entry = entry.cloned();
                                move |(p1, p2)| {
                                    blobrepo.store_file_change(
                                        p1.and_then(|x| x),
                                        p2.and_then(|x| x),
                                        &path,
                                        entry.as_ref(),
                                    )
                                }
                            })
                            .and_then(move |entry| memory_manifest.change_entry(&path, entry));
                        futures.push(future);
                    }

                    future::join_all(futures)
                        .and_then({
                            let memory_manifest = memory_manifest.clone();
                            move |_| memory_manifest.resolve_trivial_conflicts()
                        })
                        .and_then(move |_| memory_manifest.save())
                        .map(|m| m.get_hash().into_nodehash())
                }
            })
            .boxify()
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

        let computed_node_id = HgBlobNode::new(contents.clone(), p1.as_ref(), p2.as_ref())
            .nodeid()
            .expect("impossible state -- contents has data");
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
            stats: Stats,
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
                let contents_blob = contents.into_blob();
                let cbinfo = ContentBlobInfo {
                    path: path.clone(),
                    meta: ContentBlobMeta {
                        id: *contents_blob.id(),
                        copy_from,
                    },
                };

                let upload_fut = repo.upload_blob(contents_blob)
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
        HgBlobNode::new(raw_content, p1, p2)
            .nodeid()
            .expect("contents must have data available")
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

    fn log_stats(logger: Logger, path: MPath, nodeid: HgNodeHash, phase: &str, stats: Stats) {
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

pub struct CreateChangeset {
    /// This should always be provided, keeping it an Option for tests
    pub expected_nodeid: Option<HgNodeHash>,
    pub expected_files: Option<Vec<MPath>>,
    pub p1: Option<ChangesetHandle>,
    pub p2: Option<ChangesetHandle>,
    // root_manifest can be None f.e. when commit removes all the content of the repo
    pub root_manifest: BoxFuture<Option<(HgBlobEntry, RepoPath)>, Error>,
    pub sub_entries: BoxStream<(HgBlobEntry, RepoPath), Error>,
    pub user: String,
    pub time: DateTime,
    pub extra: BTreeMap<Vec<u8>, Vec<u8>>,
    pub comments: String,
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
        let parents_data = handle_parents(scuba_logger.clone(), repo.clone(), self.p1, self.p2)
            .context("While waiting for parents to upload data");
        let changeset = {
            let mut scuba_logger = scuba_logger.clone();
            upload_entries
                .join(parents_data)
                .from_err()
                .and_then({
                    let filenodes = repo.filenodes.clone();
                    let blobstore = repo.blobstore.clone();
                    let mut scuba_logger = scuba_logger.clone();
                    let expected_files = self.expected_files;
                    let user = self.user;
                    let time = self.time;
                    let extra = self.extra;
                    let comments = self.comments;

                    move |((root_manifest, root_hash), (parents, p1_manifest, p2_manifest))| {
                        let files = if let Some(expected_files) = expected_files {
                            STATS::create_changeset_expected_cf.add_value(1);
                            // We are trusting the callee to provide a list of changed files, used
                            // by the import job
                            future::ok(expected_files).boxify()
                        } else {
                            STATS::create_changeset_compute_cf.add_value(1);
                            compute_changed_files(
                                &root_manifest,
                                p1_manifest.as_ref(),
                                p2_manifest.as_ref(),
                            )
                        };

                        files
                            .context("While computing changed files")
                            .and_then({
                                move |files| {
                                    STATS::create_changeset_cf_count.add_value(files.len() as i64);

                                    let fut: BoxFuture<
                                        BlobChangeset,
                                        Error,
                                    > = (move || {
                                        let blobcs = try_boxfuture!(make_new_changeset(
                                            parents,
                                            root_hash,
                                            user,
                                            time,
                                            extra,
                                            files,
                                            comments,
                                        ));

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
                                        let _ = signal_parent_ready.send((cs_id, manifest_id));

                                        blobcs
                                            .save(blobstore)
                                            .context("While writing to blobstore")
                                            .join(
                                                entry_processor
                                                    .finalize(filenodes, cs_id)
                                                    .context("While finalizing processing"),
                                            )
                                            .from_err()
                                            .map(move |_| blobcs)
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
                            .add_stats(&stats)
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
                            .add_stats(&stats)
                            .log_with_msg("Parents completed", None);
                    }
                    Ok(())
                }
            });

        let complete_changesets = repo.changesets.clone();
        let repo_id = repo.repoid;
        ChangesetHandle::new_pending(
            can_be_parent.shared(),
            changeset
                .join(parents_complete)
                .and_then(move |(cs, _)| {
                    let completion_record = ChangesetInsert {
                        repo_id: repo_id,
                        cs_id: cs.get_changeset_id(),
                        parents: cs.parents()
                            .into_iter()
                            .map(|n| HgChangesetId::new(n))
                            .collect(),
                    };
                    complete_changesets
                        .add(completion_record)
                        .map(|_| cs)
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
                                .add_stats(&stats)
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
        }
    }
}

pub struct BlobChangesetStream {
    repo: BlobRepo,
    seen: HashSet<HgNodeHash>,
    heads: BoxStream<HgNodeHash, Error>,
    state: BCState,
}

enum BCState {
    Idle,
    WaitCS(HgNodeHash, BoxFuture<BlobChangeset, Error>),
}

impl Stream for BlobChangesetStream {
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
