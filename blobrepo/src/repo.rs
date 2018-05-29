// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{BTreeMap, HashSet};
use std::mem;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;
use std::usize;

use db::{get_connection_params, InstanceRequirement, ProxyRequirement};
use failure::{Fail, ResultExt};
use futures::{Async, Poll};
use futures::future::{self, Future};
use futures::stream::{self, Stream};
use futures::sync::oneshot;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_stats::{Stats, Timed};
use slog::{Discard, Drain, Logger};
use time_ext::DurationExt;
use uuid::Uuid;

use blobstore::{Blobstore, CachingBlobstore};
use bookmarks::{self, Bookmark, BookmarkPrefix, Bookmarks};
use changesets::{CachingChangests, ChangesetInsert, Changesets, MysqlChangesets, SqliteChangesets};
use dbbookmarks::{MysqlDbBookmarks, SqliteDbBookmarks};
use delayblob::DelayBlob;
use dieselfilenodes::{MysqlFilenodes, SqliteFilenodes, DEFAULT_INSERT_CHUNK_SIZE};
use filenodes::{CachingFilenodes, Filenodes};
use manifoldblob::ManifoldBlob;
use memblob::EagerMemblob;
use mercurial::{HgBlobNode, HgNodeHash, HgParents, NodeHashConversion};
use mercurial_types::{Changeset, DChangesetId, DFileNodeId, DNodeHash, DParents, Entry, HgBlob,
                      Manifest, RepoPath, RepositoryId, Type};
use mercurial_types::manifest;
use mercurial_types::nodehash::DManifestId;
use mononoke_types::{Blob, BlobstoreBytes, BonsaiChangeset, ContentId, DateTime, FileChange,
                     FileContents, MPath, MononokeId};
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

pub struct BlobRepo {
    logger: Logger,
    blobstore: Arc<Blobstore>,
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
            blobstore,
            filenodes,
            changesets,
            repoid,
        }
    }

    pub fn new_rocksdb(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<Self> {
        let bookmarks = SqliteDbBookmarks::open_or_create(path.join("books").to_string_lossy())
            .context(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;

        let options = rocksdb::Options::new().create_if_missing(true);
        let blobstore = Rocksblob::open_with_options(path.join("blobs"), options)
            .context(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
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
            Arc::new(blobstore),
            Arc::new(filenodes),
            Arc::new(changesets),
            repoid,
        ))
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
        let bookmarks = SqliteDbBookmarks::open_or_create(path.join("books").to_string_lossy())
            .context(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;

        let options = rocksdb::Options::new().create_if_missing(true);
        let blobstore = Rocksblob::open_with_options(path.join("blobs"), options)
            .context(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
        let filenodes = SqliteFilenodes::open_or_create(
            path.join("filenodes").to_string_lossy(),
            DEFAULT_INSERT_CHUNK_SIZE,
        ).context(ErrorKind::StateOpen(StateOpenError::Filenodes))?;
        let changesets = SqliteChangesets::open_or_create(
            path.join("changesets").to_string_lossy(),
        ).context(ErrorKind::StateOpen(StateOpenError::Changesets))?;

        let blobstore = DelayBlob::new(
            Box::new(blobstore),
            delay_gen,
            get_roundtrips,
            put_roundtrips,
            is_present_roundtrips,
            assert_present_roundtrips,
        );

        Ok(Self::new(
            logger,
            Arc::new(bookmarks),
            Arc::new(blobstore),
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
        let bookmarks = MysqlDbBookmarks::open(connection_params.clone())
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
        let blobstore =
            CachingBlobstore::new(Arc::new(blobstore), usize::MAX, blobstore_cache_size);

        let filenodes = MysqlFilenodes::open(connection_params.clone(), DEFAULT_INSERT_CHUNK_SIZE)
            .context(ErrorKind::StateOpen(StateOpenError::Filenodes))?;
        let filenodes = CachingFilenodes::new(Arc::new(filenodes), filenodes_cache_size);

        let changesets = MysqlChangesets::open(connection_params)
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

    pub fn get_file_content(&self, key: &DNodeHash) -> BoxFuture<FileContents, Error> {
        fetch_file_content_and_renames_from_blobstore(&self.blobstore, *key)
            .map(|contentrename| contentrename.0)
            .boxify()
    }

    /// The raw filenode content is crucial for operation like delta application. It is stored in
    /// untouched represenation that came from Mercurial client
    pub fn get_raw_filenode_content(&self, key: &DNodeHash) -> BoxFuture<BlobstoreBytes, Error> {
        fetch_raw_filenode_bytes(&self.blobstore, *key)
    }

    pub fn get_parents(&self, path: &RepoPath, node: &DNodeHash) -> BoxFuture<DParents, Error> {
        let path = path.clone();
        let node = DFileNodeId::new(*node);
        self.filenodes
            .get_filenode(&path, &node, &self.repoid)
            .and_then({
                move |filenode| {
                    filenode
                        .ok_or(ErrorKind::MissingFilenode(path, node).into())
                        .map(|filenode| {
                            let p1 = filenode.p1.map(|p| p.into_nodehash());
                            let p2 = filenode.p2.map(|p| p.into_nodehash());
                            DParents::new(p1.as_ref(), p2.as_ref())
                        })
                }
            })
            .boxify()
    }

    pub fn get_file_copy(
        &self,
        path: &RepoPath,
        node: &DNodeHash,
    ) -> BoxFuture<Option<(RepoPath, DNodeHash)>, Error> {
        let path = path.clone();
        let node = DFileNodeId::new(*node);
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

    pub fn get_changesets(&self) -> BoxStream<DNodeHash, Error> {
        BlobChangesetStream {
            repo: self.clone(),
            state: BCState::Idle,
            heads: self.get_heads().boxify(),
            seen: HashSet::new(),
        }.boxify()
    }

    pub fn get_heads(&self) -> impl Stream<Item = DNodeHash, Error = Error> {
        self.bookmarks
            .list_by_prefix(&BookmarkPrefix::empty(), &self.repoid)
            .map(|(_, cs)| cs.into_nodehash())
    }

    pub fn changeset_exists(&self, changesetid: &DChangesetId) -> BoxFuture<bool, Error> {
        self.changesets
            .get(self.repoid, *changesetid)
            .map(|res| res.is_some())
            .boxify()
    }

    pub fn get_changeset_parents(
        &self,
        changesetid: &DChangesetId,
    ) -> BoxFuture<Vec<DChangesetId>, Error> {
        let changesetid = *changesetid;
        self.changesets
            .get(self.repoid, changesetid.clone())
            .and_then(move |res| res.ok_or(ErrorKind::ChangesetMissing(changesetid).into()))
            .map(|res| res.parents)
            .boxify()
    }

    pub fn get_changeset_by_changesetid(
        &self,
        changesetid: &DChangesetId,
    ) -> BoxFuture<BlobChangeset, Error> {
        let chid = changesetid.clone();
        BlobChangeset::load(&self.blobstore, &chid)
            .and_then(move |cs| cs.ok_or(ErrorKind::ChangesetMissing(chid).into()))
            .boxify()
    }

    pub fn get_manifest_by_nodeid(
        &self,
        nodeid: &DNodeHash,
    ) -> BoxFuture<Box<Manifest + Sync>, Error> {
        let nodeid = *nodeid;
        let manifestid = DManifestId::new(nodeid);
        BlobManifest::load(&self.blobstore, &manifestid)
            .and_then(move |mf| mf.ok_or(ErrorKind::ManifestMissing(nodeid).into()))
            .map(|m| m.boxed())
            .boxify()
    }

    pub fn get_root_entry(&self, manifestid: &DManifestId) -> Box<Entry + Sync> {
        Box::new(HgBlobEntry::new_root(self.blobstore.clone(), *manifestid))
    }

    pub fn get_bookmark(&self, name: &Bookmark) -> BoxFuture<Option<DChangesetId>, Error> {
        self.bookmarks.get(name, &self.repoid)
    }

    // TODO(stash): rename to get_all_bookmarks()?
    pub fn get_bookmarks(&self) -> BoxStream<(Bookmark, DChangesetId), Error> {
        self.bookmarks
            .list_by_prefix(&BookmarkPrefix::empty(), &self.repoid)
    }

    pub fn update_bookmark_transaction(&self) -> Box<bookmarks::Transaction> {
        self.bookmarks.create_transaction(&self.repoid)
    }

    pub fn get_linknode(&self, path: RepoPath, node: &DNodeHash) -> BoxFuture<DNodeHash, Error> {
        let node = DFileNodeId::new(*node);
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

    pub fn get_generation_number(&self, cs: &DChangesetId) -> BoxFuture<Option<u64>, Error> {
        self.changesets
            .get(self.repoid, *cs)
            .map(|res| res.map(|res| res.gen))
            .boxify()
    }

    pub fn upload_blob<Id>(&self, blob: Blob<Id>) -> impl Future<Item = Id, Error = Error> + Send
    where
        Id: MononokeId,
    {
        let id = blob.id().clone();
        let blobstore_key = id.blobstore_key();

        fn log_upload_stats(logger: Logger, blobstore_key: String, phase: &str, stats: Stats) {
            debug!(logger, "Upload blob stats";
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
    pub fn get_blobstore(&self) -> Arc<Blobstore> {
        self.blobstore.clone()
    }

    // TODO(T29283916): make this save the file change as a Mercurial format HgBlobEntry
    pub fn store_file_change(
        _blobstore: Arc<Blobstore>,
        _change: Option<&FileChange>,
    ) -> BoxFuture<Option<HgBlobEntry>, Error> {
        unimplemented!()
    }

    // TODO(T29283916): Using caching to avoid wasting compute, change this to find the manifest_p1
    // and manifest_p2 from bcs, so that you can remove manifest_p1 and manifest_p2 from the args
    // to this function
    pub fn get_manifest_from_bonsai(
        &self,
        bcs: BonsaiChangeset,
        manifest_p1: Option<&HgNodeHash>,
        manifest_p2: Option<&HgNodeHash>,
    ) -> BoxFuture<DNodeHash, Error> {
        MemoryRootManifest::new(
            self.blobstore.clone(),
            self.logger.clone(),
            manifest_p1,
            manifest_p2,
        ).and_then({
            let blobstore = self.blobstore.clone();
            move |memory_manifest| {
                let memory_manifest = Arc::new(memory_manifest);
                let mut futures = Vec::new();

                for (path, entry) in bcs.file_changes() {
                    let path = path.clone();
                    let memory_manifest = memory_manifest.clone();
                    futures.push(
                        Self::store_file_change(blobstore.clone(), entry.clone())
                            .and_then(move |entry| memory_manifest.change_entry(&path, entry)),
                    );
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
        blobstore: &Arc<Blobstore>,
        logger: &Logger,
    ) -> Result<(HgNodeHash, BoxFuture<(HgBlobEntry, RepoPath), Error>)> {
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
                HgBlobEntry::new(
                    blobstore.clone(),
                    entry_path,
                    nodeid.into_mononoke(),
                    content_type,
                )
            }
            None => {
                if content_type != Type::Tree {
                    return Err(
                        ErrorKind::NotAManifest(nodeid.into_mononoke(), content_type).into(),
                    );
                }
                HgBlobEntry::new_root(blobstore.clone(), DManifestId::new(nodeid.into_mononoke()))
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
            debug!(logger, "Upload blob stats";
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
        let node_upload = blobstore.put(
            get_node_key(nodeid.into_mononoke()),
            raw_node.serialize(&nodeid)?.into(),
        );

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
        let entry_processor = UploadEntries::new(repo.blobstore.clone(), repo.repoid.clone());
        let (signal_parent_ready, can_be_parent) = oneshot::channel();
        // This is used for logging, so that we can tie up all our pieces without knowing about
        // the final commit hash
        let uuid = Uuid::new_v4();

        let upload_entries = process_entries(
            repo.logger.clone(),
            uuid,
            repo.clone(),
            &entry_processor,
            self.root_manifest,
            self.sub_entries,
        );

        let parents_complete = extract_parents_complete(&self.p1, &self.p2);
        let parents_data =
            handle_parents(repo.logger.clone(), uuid, repo.clone(), self.p1, self.p2);
        let changeset = {
            let logger = repo.logger.clone();
            upload_entries
                .join(parents_data)
                .and_then({
                    let filenodes = repo.filenodes.clone();
                    let blobstore = repo.blobstore.clone();
                    let logger = repo.logger.clone();
                    let expected_nodeid = self.expected_nodeid;
                    let expected_files = self.expected_files;
                    let user = self.user;
                    let time = self.time;
                    let extra = self.extra;
                    let comments = self.comments;

                    move |((root_manifest, root_hash), (parents, p1_manifest, p2_manifest))| {
                        let files = if let Some(expected_files) = expected_files {
                            // We are trusting the callee to provide a list of changed files, used
                            // by the import job
                            future::ok(expected_files).boxify()
                        } else {
                            compute_changed_files(
                                &root_manifest,
                                p1_manifest.as_ref(),
                                p2_manifest.as_ref(),
                            )
                        };

                        files.and_then({
                            move |files| {
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
                                    let cs_id = cs_id.into_mercurial();
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

                                blobcs
                                    .save(blobstore)
                                    .join(entry_processor.finalize(filenodes, cs_id))
                                    .map(move |_| {
                                        // We deliberately eat this error - this is only so that
                                        // another changeset can start uploading to the blob store
                                        // while we complete this one
                                        let _ = signal_parent_ready.send((cs_id, manifest_id));
                                    })
                                    .map(move |_| blobcs)
                                    .boxify()
                            }
                        })
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
            .map_err(|e| ErrorKind::ParentsFailed.context(e).into())
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
                            .map(|n| DChangesetId::new(n))
                            .collect(),
                    };
                    complete_changesets.add(completion_record).map(|_| cs)
                })
                .map_err(Error::compat)
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
    seen: HashSet<DNodeHash>,
    heads: BoxStream<DNodeHash, Error>,
    state: BCState,
}

enum BCState {
    Idle,
    WaitCS(DNodeHash, BoxFuture<BlobChangeset, Error>),
}

impl Stream for BlobChangesetStream {
    type Item = DNodeHash;
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
                                    .get_changeset_by_changesetid(&DChangesetId::new(next)),
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
