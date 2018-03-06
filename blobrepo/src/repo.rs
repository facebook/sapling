// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{BTreeMap, HashSet};
use std::mem;
use std::path::Path;
use std::sync::Arc;

use bincode;
use bytes::Bytes;
use failure::{Fail, ResultExt};
use futures::{Async, Poll};
use futures::future::Future;
use futures::stream::{self, Stream};
use futures::sync::oneshot;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use slog::{Discard, Drain, Logger};

use blobstore::Blobstore;
use bookmarks::Bookmarks;
use changesets::{ChangesetInsert, Changesets, SqliteChangesets};
use fileblob::Fileblob;
use filebookmarks::FileBookmarks;
use fileheads::FileHeads;
use filelinknodes::FileLinknodes;
use heads::Heads;
use linknodes::Linknodes;
use manifoldblob::ManifoldBlob;
use memblob::{EagerMemblob, LazyMemblob};
use membookmarks::MemBookmarks;
use memheads::MemHeads;
use memlinknodes::MemLinknodes;
use mercurial_types::{Blob, BlobNode, Changeset, ChangesetId, Entry, MPath, Manifest, NodeHash,
                      Parents, RepoPath, RepositoryId, Time};
use mercurial_types::manifest;
use mercurial_types::nodehash::ManifestId;
use rocksblob::Rocksblob;
use storage_types::Version;
use tokio_core::reactor::Remote;

use BlobChangeset;
use BlobManifest;
use errors::*;
use file::{fetch_file_content_and_renames_from_blobstore, BlobEntry};
use repo_commit::*;
use utils::{get_node, get_node_key, RawNodeBlob};

pub struct BlobRepo {
    logger: Logger,
    blobstore: Arc<Blobstore>,
    bookmarks: Arc<Bookmarks>,
    heads: Arc<Heads>,
    linknodes: Arc<Linknodes>,
    changesets: Arc<Changesets>,
    repoid: RepositoryId,
}

impl BlobRepo {
    pub fn new(
        logger: Logger,
        heads: Arc<Heads>,
        bookmarks: Arc<Bookmarks>,
        blobstore: Arc<Blobstore>,
        linknodes: Arc<Linknodes>,
        changesets: Arc<Changesets>,
        repoid: RepositoryId,
    ) -> Self {
        BlobRepo {
            logger,
            heads,
            bookmarks,
            blobstore,
            linknodes,
            changesets,
            repoid,
        }
    }

    pub fn new_files(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<Self> {
        let heads = FileHeads::open(path.join("heads"))
            .context(ErrorKind::StateOpen(StateOpenError::Heads))?;
        let bookmarks = FileBookmarks::open(path.join("books"))
            .context(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;
        let blobstore = Fileblob::open(path.join("blobs"))
            .context(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
        let linknodes = FileLinknodes::open(path.join("linknodes"))
            .context(ErrorKind::StateOpen(StateOpenError::Linknodes))?;
        let changesets = SqliteChangesets::open(path.join("changesets").to_string_lossy())
            .context(ErrorKind::StateOpen(StateOpenError::Linknodes))?;

        Ok(Self::new(
            logger,
            Arc::new(heads),
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(linknodes),
            Arc::new(changesets),
            repoid,
        ))
    }

    pub fn new_rocksdb(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<Self> {
        let heads = FileHeads::open(path.join("heads"))
            .context(ErrorKind::StateOpen(StateOpenError::Heads))?;
        let bookmarks = FileBookmarks::open(path.join("books"))
            .context(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;
        let blobstore = Rocksblob::open(path.join("blobs"))
            .context(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
        let linknodes = FileLinknodes::open(path.join("linknodes"))
            .context(ErrorKind::StateOpen(StateOpenError::Linknodes))?;
        let changesets = SqliteChangesets::open(path.join("changesets").to_string_lossy())
            .context(ErrorKind::StateOpen(StateOpenError::Linknodes))?;

        Ok(Self::new(
            logger,
            Arc::new(heads),
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(linknodes),
            Arc::new(changesets),
            repoid,
        ))
    }

    // Memblob repos are test repos, and do not have to have a logger. If we're given None,
    // we won't log.
    pub fn new_memblob(
        logger: Option<Logger>,
        heads: MemHeads,
        bookmarks: MemBookmarks,
        blobstore: EagerMemblob,
        linknodes: MemLinknodes,
        changesets: SqliteChangesets,
        repoid: RepositoryId,
    ) -> Self {
        Self::new(
            logger.unwrap_or(Logger::root(Discard {}.ignore_res(), o!())),
            Arc::new(heads),
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(linknodes),
            Arc::new(changesets),
            repoid,
        )
    }

    pub fn new_lazymemblob(
        logger: Option<Logger>,
        heads: MemHeads,
        bookmarks: MemBookmarks,
        blobstore: LazyMemblob,
        linknodes: MemLinknodes,
        changesets: SqliteChangesets,
        repoid: RepositoryId,
    ) -> Self {
        Self::new(
            logger.unwrap_or(Logger::root(Discard {}.ignore_res(), o!())),
            Arc::new(heads),
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(linknodes),
            Arc::new(changesets),
            repoid,
        )
    }

    pub fn new_memblob_empty(logger: Option<Logger>) -> Result<Self> {
        Ok(Self::new(
            logger.unwrap_or(Logger::root(Discard {}.ignore_res(), o!())),
            Arc::new(MemHeads::new()),
            Arc::new(MemBookmarks::new()),
            Arc::new(EagerMemblob::new()),
            Arc::new(MemLinknodes::new()),
            Arc::new(SqliteChangesets::in_memory()
                .context(ErrorKind::StateOpen(StateOpenError::Changesets))?),
            RepositoryId::new(0),
        ))
    }

    pub fn new_test_manifold<T: ToString>(
        logger: Logger,
        bucket: T,
        remote: &Remote,
        repoid: RepositoryId,
    ) -> Result<Self> {
        let heads = MemHeads::new();
        let bookmarks = MemBookmarks::new();
        let blobstore = ManifoldBlob::new_may_panic(bucket.to_string(), remote);
        let linknodes = MemLinknodes::new();
        let changesets = SqliteChangesets::in_memory()
            .context(ErrorKind::StateOpen(StateOpenError::Changesets))?;

        Ok(Self::new(
            logger,
            Arc::new(heads),
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(linknodes),
            Arc::new(changesets),
            repoid,
        ))
    }

    pub fn get_file_content(&self, key: &NodeHash) -> BoxFuture<Bytes, Error> {
        fetch_file_content_and_renames_from_blobstore(&self.blobstore, *key)
            .map(|contentrename| contentrename.0)
            .boxify()
    }

    pub fn get_parents(&self, key: &NodeHash) -> BoxFuture<Parents, Error> {
        get_node(&self.blobstore, *key)
            .map(|rawnode| rawnode.parents)
            .boxify()
    }

    pub fn get_file_copy(&self, key: &NodeHash) -> BoxFuture<Option<(MPath, NodeHash)>, Error> {
        fetch_file_content_and_renames_from_blobstore(&self.blobstore, *key)
            .map(|contentrename| contentrename.1)
            .boxify()
    }

    pub fn get_changesets(&self) -> BoxStream<NodeHash, Error> {
        BlobChangesetStream {
            repo: self.clone(),
            heads: self.heads.heads().boxify(),
            state: BCState::Idle,
            seen: HashSet::new(),
        }.boxify()
    }

    pub fn get_heads(&self) -> BoxStream<NodeHash, Error> {
        self.heads.heads().boxify()
    }

    pub fn changeset_exists(&self, changesetid: &ChangesetId) -> BoxFuture<bool, Error> {
        self.changesets
            .get(self.repoid, *changesetid)
            .map(|res| res.is_some())
            .boxify()
    }

    pub fn get_changeset_by_changesetid(
        &self,
        changesetid: &ChangesetId,
    ) -> BoxFuture<BlobChangeset, Error> {
        let chid = changesetid.clone();
        BlobChangeset::load(&self.blobstore, &chid)
            .and_then(move |cs| cs.ok_or(ErrorKind::ChangesetMissing(chid).into()))
            .boxify()
    }

    pub fn get_manifest_by_nodeid(
        &self,
        nodeid: &NodeHash,
    ) -> BoxFuture<Box<Manifest + Sync>, Error> {
        let nodeid = *nodeid;
        let manifestid = ManifestId::new(nodeid);
        BlobManifest::load(&self.blobstore, &manifestid)
            .and_then(move |mf| mf.ok_or(ErrorKind::ManifestMissing(nodeid).into()))
            .map(|m| m.boxed())
            .boxify()
    }

    pub fn get_root_entry(&self, manifestid: &ManifestId) -> Box<Entry + Sync> {
        Box::new(BlobEntry::new_root(self.blobstore.clone(), *manifestid))
    }

    pub fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Error> {
        self.bookmarks.keys().boxify()
    }

    pub fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(ChangesetId, Version)>, Error> {
        self.bookmarks.get(key).boxify()
    }

    pub fn get_linknode(&self, path: RepoPath, node: &NodeHash) -> BoxFuture<NodeHash, Error> {
        self.linknodes.get(path, node)
    }

    pub fn get_generation_number(&self, cs: &ChangesetId) -> BoxFuture<Option<u64>, Error> {
        self.changesets
            .get(self.repoid, *cs)
            .map(|res| res.map(|res| res.gen))
            .boxify()
    }

    // Given content, ensure that there is a matching BlobEntry in the repo. This may not upload
    // the entry or the data blob if the repo is aware of that data already existing in the
    // underlying store.
    // Note that the BlobEntry may not be consistent - parents do not have to be uploaded at this
    // point, as long as you know their NodeHashes; this is also given to you as part of the
    // result type, so that you can parallelise uploads. Consistency will be verified when
    // adding the entries to a changeset.
    pub fn upload_entry(
        &self,
        raw_content: Blob,
        content_type: manifest::Type,
        p1: Option<NodeHash>,
        p2: Option<NodeHash>,
        path: RepoPath,
    ) -> Result<(NodeHash, BoxFuture<(BlobEntry, RepoPath), Error>)> {
        let p1 = p1.as_ref();
        let p2 = p2.as_ref();
        let raw_content = raw_content.clean();
        let parents = Parents::new(p1, p2);

        let blob_hash = raw_content
            .hash()
            .ok_or_else(|| Error::from(ErrorKind::BadUploadBlob(raw_content.clone())))?;

        let raw_node = RawNodeBlob {
            parents,
            blob: blob_hash,
        };

        let nodeid = BlobNode::new(raw_content.clone(), p1, p2)
            .nodeid()
            .ok_or_else(|| Error::from(ErrorKind::BadUploadBlob(raw_content.clone())))?;

        let blob_entry = BlobEntry::new(
            self.blobstore.clone(),
            path.mpath()
                .and_then(|m| m.into_iter().last())
                .map(|m| m.clone()),
            nodeid,
            content_type,
        )?;

        // Ensure that content is in the blobstore
        let content_upload = self.blobstore.put(
            format!("sha1-{}", blob_hash.sha1()),
            raw_content
                .clone()
                .into_inner()
                .ok_or_else(|| Error::from(ErrorKind::BadUploadBlob(raw_content.clone())))?,
        );

        // Upload the new node
        let node_upload = self.blobstore.put(
            get_node_key(nodeid),
            bincode::serialize(&raw_node)
                .map_err(|err| Error::from(ErrorKind::SerializationFailed(nodeid, err)))?
                .into(),
        );

        Ok((
            nodeid,
            content_upload
                .join(node_upload)
                .map(|_| (blob_entry, path))
                .boxify(),
        ))
    }

    /// Create a changeset in this repo. This will upload all the blobs to the underlying Blobstore
    /// and ensure that the changeset is marked as "complete".
    /// No attempt is made to clean up the Blobstore if the changeset creation fails
    pub fn create_changeset(
        &self,
        p1: Option<ChangesetHandle>,
        p2: Option<ChangesetHandle>,
        root_manifest: BoxFuture<(BlobEntry, RepoPath), Error>,
        new_child_entries: BoxStream<(BlobEntry, RepoPath), Error>,
        user: String,
        time: Time,
        extra: BTreeMap<Vec<u8>, Vec<u8>>,
        comments: String,
    ) -> ChangesetHandle {
        let entry_processor = UploadEntries::new(self.blobstore.clone());
        let (signal_parent_ready, can_be_parent) = oneshot::channel();

        let upload_entries = process_entries(
            self.clone(),
            &entry_processor,
            root_manifest,
            new_child_entries,
        );

        let parents_complete = extract_parents_complete(&p1, &p2);
        let parents_data = handle_parents(self.clone(), p1, p2);
        let changeset = {
            upload_entries.join(parents_data).and_then({
                let linknodes = self.linknodes.clone();
                let blobstore = self.blobstore.clone();
                let heads = self.heads.clone();

                move |((root_manifest, root_hash), (parents, p1_manifest, p2_manifest))| {
                    compute_changed_files(
                        &root_manifest,
                        p1_manifest.as_ref(),
                        p2_manifest.as_ref(),
                    ).and_then({
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

                            blobcs
                                .save(blobstore)
                                .join(heads.add(&cs_id))
                                .join(entry_processor.finalize(linknodes, cs_id))
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
        };

        let parents_complete =
            parents_complete.map_err(|e| ErrorKind::ParentsFailed.context(e).into());

        let complete_changesets = self.changesets.clone();
        let repo_id = self.repoid;
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
                            .map(|n| ChangesetId::new(n))
                            .collect(),
                    };
                    complete_changesets.add(&completion_record).map(|_| cs)
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
            heads: self.heads.clone(),
            bookmarks: self.bookmarks.clone(),
            blobstore: self.blobstore.clone(),
            linknodes: self.linknodes.clone(),
            changesets: self.changesets.clone(),
            repoid: self.repoid.clone(),
        }
    }
}

pub struct BlobChangesetStream {
    repo: BlobRepo,
    seen: HashSet<NodeHash>,
    heads: BoxStream<NodeHash, Error>,
    state: BCState,
}

enum BCState {
    Idle,
    WaitCS(NodeHash, BoxFuture<BlobChangeset, Error>),
}

impl Stream for BlobChangesetStream {
    type Item = NodeHash;
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
                                    .get_changeset_by_changesetid(&ChangesetId::new(next)),
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
