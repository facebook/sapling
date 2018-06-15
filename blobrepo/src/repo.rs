// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::mem;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::thread;
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
use slog::{Discard, Drain, Logger};
use stats::Timeseries;
use time_ext::DurationExt;
use uuid::Uuid;

use blobstore::{Blobstore, EagerMemblob, MemcacheBlobstore, MemoizedBlobstore, PrefixBlobstore};
use bookmarks::{self, Bookmark, BookmarkPrefix, Bookmarks};
use changesets::{CachingChangests, ChangesetInsert, Changesets, MysqlChangesets, SqliteChangesets};
use dbbookmarks::{MysqlDbBookmarks, SqliteDbBookmarks};
use delayblob::DelayBlob;
use dieselfilenodes::{MysqlFilenodes, SqliteFilenodes, DEFAULT_INSERT_CHUNK_SIZE};
use fileblob::Fileblob;
use filenodes::{CachingFilenodes, FilenodeInfo, Filenodes};
use manifoldblob::ManifoldBlob;
use mercurial::file::File;
use mercurial_types::{Changeset, Entry, HgBlob, HgBlobNode, HgChangesetId, HgFileNodeId,
                      HgNodeHash, HgParents, Manifest, RepoPath, RepositoryId, Type};
use mercurial_types::manifest::{self, Content};
use mercurial_types::nodehash::HgManifestId;
use mononoke_types::{Blob, BlobstoreBytes, BlobstoreValue, BonsaiChangeset, ChangesetId,
                     ContentId, DateTime, FileChange, FileContents, MPath, MPathElement,
                     MononokeId};
use rocksblob::Rocksblob;
use rocksdb;
use tokio_core::reactor::Core;

use BlobChangeset;
use BlobManifest;
use errors::*;
use file::{fetch_file_content_and_renames_from_blobstore, fetch_raw_filenode_bytes, HgBlobEntry};
use memory_manifest::MemoryRootManifest;
use repo_commit::*;
use utils::{get_node_key, RawNodeBlob};

define_stats! {
    prefix = "mononoke.blobrepo";
    get_file_content: timeseries(RATE, SUM),
    get_raw_filenode_content: timeseries(RATE, SUM),
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
    upload_hg_entry: timeseries(RATE, SUM),
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

pub struct BlobRepo {
    logger: Logger,
    blobstore: RepoBlobstore,
    bookmarks: Arc<Bookmarks>,
    filenodes: Arc<Filenodes>,
    changesets: Arc<Changesets>,
    repoid: RepositoryId,
}

impl BlobRepo {
    pub fn new(
        logger: Logger,
        bookmarks: Arc<Bookmarks>,
        blobstore: Arc<Blobstore>,
        filenodes: Arc<Filenodes>,
        changesets: Arc<Changesets>,
        repoid: RepositoryId,
    ) -> Self {
        BlobRepo {
            logger,
            bookmarks,
            // XXX Switch the second argument to repoid.prefix() when ready
            blobstore: PrefixBlobstore::new(blobstore, ""),
            filenodes,
            changesets,
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

        Ok(Self::new(
            logger,
            Arc::new(bookmarks),
            blobstore,
            Arc::new(filenodes),
            Arc::new(changesets),
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
            RepositoryId::new(0),
        ))
    }

    pub fn new_test_manifold<T: ToString>(
        logger: Logger,
        bucket: T,
        prefix: &str,
        repoid: RepositoryId,
        db_address: &str,
        blobstore_cache_size: usize,
        changesets_cache_size: usize,
        filenodes_cache_size: usize,
        thread_num: usize,
        max_concurrent_requests_per_io_thread: usize,
    ) -> Result<Self> {
        // TODO(stash): T28429403 use local region first, fallback to master if not found
        let connection_params = get_connection_params(
            db_address.to_string(),
            InstanceRequirement::Master,
            None,
            Some(ProxyRequirement::Forbidden),
        )?;
        let bookmarks = MysqlDbBookmarks::open(&connection_params)
            .context(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;

        let mut io_remotes = vec![];
        for i in 0..thread_num {
            let (sender, recv) = mpsc::channel();
            let builder = thread::Builder::new().name(format!("blobstore_io_{}", i));
            builder
                .spawn(move || {
                    let mut core = Core::new()
                        .expect("failed to create manifold blobrepo: failed to create core");
                    sender
                        .send(core.remote())
                        .expect("failed to create manifold blobrepo: sending remote failed");
                    loop {
                        core.turn(None);
                    }
                })
                .expect("failed to start blobstore io thread");

            let remote = recv.recv()
                .expect("failed to create manifold blobrepo: recv remote failed");
            io_remotes.push(remote);
        }
        let blobstore = ManifoldBlob::new_with_prefix(
            bucket.to_string(),
            prefix,
            io_remotes.iter().collect(),
            max_concurrent_requests_per_io_thread,
        );
        let blobstore = MemcacheBlobstore::new(blobstore);
        let blobstore = MemoizedBlobstore::new(blobstore, usize::MAX, blobstore_cache_size);

        let filenodes = MysqlFilenodes::open(&connection_params, DEFAULT_INSERT_CHUNK_SIZE)
            .context(ErrorKind::StateOpen(StateOpenError::Filenodes))?;
        let filenodes = CachingFilenodes::new(Arc::new(filenodes), filenodes_cache_size);

        let changesets = MysqlChangesets::open(&connection_params)
            .context(ErrorKind::StateOpen(StateOpenError::Changesets))?;

        let changesets = CachingChangests::new(Arc::new(changesets), changesets_cache_size);

        Ok(Self::new(
            logger,
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(filenodes),
            Arc::new(changesets),
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

    /// The raw filenode content is crucial for operation like delta application. It is stored in
    /// untouched represenation that came from Mercurial client
    pub fn get_raw_filenode_content(&self, key: &HgNodeHash) -> BoxFuture<BlobstoreBytes, Error> {
        STATS::get_raw_filenode_content.add_value(1);
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

    pub fn get_generation_number(&self, cs: &HgChangesetId) -> BoxFuture<Option<u64>, Error> {
        STATS::get_generation_number.add_value(1);
        self.changesets
            .get(self.repoid, *cs)
            .map(|res| res.map(|res| res.gen))
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

    pub fn store_file_change(
        &self,
        p1: Option<HgNodeHash>,
        p2: Option<HgNodeHash>,
        path: &MPath,
        change: Option<&FileChange>,
    ) -> impl Future<Item = Option<HgBlobEntry>, Error = Error> + Send {
        fn prepend_metadata(
            content: Bytes,
            _copy_from: Option<&(MPath, ChangesetId)>,
        ) -> Result<Bytes> {
            let mut buf = Vec::new();
            File::generate_copied_from(
                None, //FIXME: we need external {ChangesetId -> HgNodeHash} mapping
                &mut buf,
            )?;
            buf.write(content.as_ref())?;
            Ok(buf.into())
        }

        match change {
            None => Either::A(future::ok(None)),
            Some(change) => {
                let upload_future = self.fetch(change.content_id()).and_then({
                    let blobstore = self.blobstore.clone();
                    let change = change.clone();
                    let logger = self.logger.clone();
                    let path = path.clone();
                    move |file_content| {
                        let hg_content = try_boxfuture!(prepend_metadata(
                            file_content.into_bytes(),
                            change.copy_from()
                        ));
                        let upload_entry = UploadHgEntry {
                            upload_nodeid: UploadHgNodeHash::Generate,
                            raw_content: HgBlob::Dirty(hg_content),
                            content_type: manifest::Type::File(change.file_type()),
                            p1,
                            p2,
                            path: RepoPath::FilePath(path),
                        };
                        let (_, upload_future) =
                            try_boxfuture!(upload_entry.upload_to_blobstore(&blobstore, &logger));
                        upload_future.map(|(entry, _)| Some(entry)).boxify()
                    }
                });
                Either::B(upload_future)
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
                    Some(Content::Tree(manifest)) => Either::B(
                        manifest
                            .lookup(&path_element)
                            .and_then(|entry| match entry {
                                None => Either::A(future::ok(None)),
                                Some(entry) => Either::B(entry.get_content().map(Some)),
                            }),
                    ),
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
        self.find_path_in_manifest(dirname, manifest).and_then({
            let basename = basename.clone();
            move |content| match content {
                None => Either::A(future::ok(None)),
                Some(Content::Tree(manifest)) => {
                    Either::B(manifest.lookup(&basename).map(|entry| match entry {
                        None => None,
                        Some(entry) => if let Type::File(_) = entry.get_type() {
                            Some(entry.get_hash().into_nodehash())
                        } else {
                            None
                        },
                    }))
                }
                Some(_) => Either::A(future::ok(None)),
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
        MemoryRootManifest::new(
            self.blobstore.clone(),
            self.logger.clone(),
            manifest_p1,
            manifest_p2,
        ).and_then({
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

/// Context for uploading a Mercurial entry.
pub struct UploadHgEntry {
    pub upload_nodeid: UploadHgNodeHash,
    pub raw_content: HgBlob,
    pub content_type: manifest::Type,
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub path: RepoPath,
}

impl UploadHgEntry {
    // Given content, ensure that there is a matching HgBlobEntry in the repo. This may not upload
    // the entry or the data blob if the repo is aware of that data already existing in the
    // underlying store.
    // Note that the HgBlobEntry may not be consistent - parents do not have to be uploaded at this
    // point, as long as you know their DNodeHashes; this is also given to you as part of the
    // result type, so that you can parallelise uploads. Consistency will be verified when
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
        STATS::upload_hg_entry.add_value(1);
        let UploadHgEntry {
            upload_nodeid,
            raw_content,
            content_type,
            p1,
            p2,
            path,
        } = self;

        let p1 = p1.as_ref();
        let p2 = p2.as_ref();
        let raw_content = raw_content.clean();

        let nodeid: HgNodeHash = match upload_nodeid {
            UploadHgNodeHash::Generate => HgBlobNode::new(raw_content.clone(), p1, p2)
                .nodeid()
                .expect("raw_content must have data available"),
            UploadHgNodeHash::Supplied(nodeid) => nodeid,
            UploadHgNodeHash::Checked(nodeid) => {
                let computed_nodeid = HgBlobNode::new(raw_content.clone(), p1, p2)
                    .nodeid()
                    .expect("raw_content must have data available");
                if nodeid != computed_nodeid {
                    bail_err!(ErrorKind::InconsistentEntryHash(
                        path,
                        nodeid,
                        computed_nodeid
                    ));
                }
                nodeid
            }
        };

        let parents = HgParents::new(p1, p2);

        let blob_hash = raw_content
            .hash()
            .ok_or_else(|| Error::from(ErrorKind::BadUploadBlob(raw_content.clone())))?;

        let raw_node = RawNodeBlob {
            parents,
            blob: blob_hash,
        };

        let blob_entry = match path.mpath().and_then(|m| m.into_iter().last()) {
            Some(m) => {
                let entry_path = m.clone();
                HgBlobEntry::new(blobstore.clone(), entry_path, nodeid, content_type)
            }
            None => {
                if content_type != Type::Tree {
                    return Err(ErrorKind::NotAManifest(nodeid, content_type).into());
                }
                HgBlobEntry::new_root(blobstore.clone(), HgManifestId::new(nodeid))
            }
        };

        fn log_upload_stats(
            logger: Logger,
            path: RepoPath,
            nodeid: HgNodeHash,
            phase: &str,
            stats: Stats,
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

        // Ensure that content is in the blobstore
        let content_upload = blobstore
            .put(format!("sha1-{}", blob_hash.sha1()), raw_content.into())
            .timed({
                let logger = logger.clone();
                let path = path.clone();
                let nodeid = nodeid.clone();
                move |stats, result| {
                    if result.is_ok() {
                        log_upload_stats(logger, path, nodeid, "content_uploaded", stats)
                    }
                    Ok(())
                }
            });
        // Upload the new node
        let node_upload = blobstore.put(get_node_key(nodeid), raw_node.serialize(&nodeid)?.into());

        Ok((
            nodeid,
            content_upload
                .join(node_upload)
                .map({
                    let path = path.clone();
                    |_| (blob_entry, path)
                })
                .timed({
                    let logger = logger.clone();
                    let path = path.clone();
                    let nodeid = nodeid.clone();
                    move |stats, result| {
                        if result.is_ok() {
                            log_upload_stats(logger, path, nodeid, "finished", stats)
                        }
                        Ok(())
                    }
                })
                .boxify(),
        ))
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
    pub fn create(self, repo: &BlobRepo) -> ChangesetHandle {
        STATS::create_changeset.add_value(1);
        let entry_processor = UploadEntries::new(repo.blobstore.clone(), repo.repoid.clone());
        let (signal_parent_ready, can_be_parent) = oneshot::channel();
        // This is used for logging, so that we can tie up all our pieces without knowing about
        // the final commit hash
        let uuid = Uuid::new_v4();
        let expected_nodeid = self.expected_nodeid;

        let upload_entries = process_entries(
            repo.logger.clone(),
            uuid,
            repo.clone(),
            &entry_processor,
            self.root_manifest,
            self.sub_entries,
        ).context("While processing entries");

        let parents_complete = extract_parents_complete(&self.p1, &self.p2);
        let parents_data =
            handle_parents(repo.logger.clone(), uuid, repo.clone(), self.p1, self.p2)
                .context("While waiting for parents to upload data");
        let changeset = {
            let logger = repo.logger.clone();
            upload_entries
                .join(parents_data)
                .from_err()
                .and_then({
                    let filenodes = repo.filenodes.clone();
                    let blobstore = repo.blobstore.clone();
                    let logger = repo.logger.clone();
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
                                            parents, root_hash, user, time, extra, files, comments,
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

                                        debug!(logger, "Changeset uuid to hash mapping";
                                        "changeset_uuid" => format!("{}", uuid),
                                        "changeset_id" => format!("{}", cs_id));

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
                        log_cs_future_stats(&logger, "changeset_created", stats, uuid);
                    }
                    Ok(())
                })
        };

        let parents_complete = parents_complete
            .context("While waiting for parents to complete")
            .timed({
                let logger = repo.logger.clone();
                move |stats, result| {
                    if result.is_ok() {
                        log_cs_future_stats(&logger, "parents_complete", stats, uuid);
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
                    let logger = repo.logger.clone();
                    move |stats, result| {
                        if result.is_ok() {
                            log_cs_future_stats(&logger, "finished", stats, uuid);
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
