// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashSet;
use std::mem;
use std::path::Path;
use std::sync::Arc;

use failure::ResultExt;
use futures::{Async, Poll};
use futures::future::Future;
use futures::stream::{self, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use blobstore::Blobstore;
use bookmarks::Bookmarks;
use fileblob::Fileblob;
use filebookmarks::FileBookmarks;
use fileheads::FileHeads;
use filelinknodes::FileLinknodes;
use heads::Heads;
use linknodes::Linknodes;
use manifoldblob::ManifoldBlob;
use memblob::Memblob;
use membookmarks::MemBookmarks;
use memheads::MemHeads;
use memlinknodes::MemLinknodes;
use mercurial_types::{Changeset, Manifest, NodeHash};
use mercurial_types::nodehash::ChangesetId;
use rocksblob::Rocksblob;
use storage_types::Version;
use tokio_core::reactor::Remote;

use BlobChangeset;
use BlobManifest;
use errors::*;
use file::fetch_blob_from_blobstore;

pub struct BlobRepo {
    blobstore: Arc<Blobstore>,
    bookmarks: Arc<Bookmarks>,
    heads: Arc<Heads>,
    linknodes: Arc<Linknodes>,
}

impl BlobRepo {
    pub fn new(
        heads: Arc<Heads>,
        bookmarks: Arc<Bookmarks>,
        blobstore: Arc<Blobstore>,
        linknodes: Arc<Linknodes>,
    ) -> Self {
        BlobRepo {
            heads,
            bookmarks,
            blobstore,
            linknodes,
        }
    }

    pub fn new_files(path: &Path) -> Result<Self> {
        let heads = FileHeads::open(path.join("heads"))
            .context(ErrorKind::StateOpen(StateOpenError::Heads))?;
        let bookmarks = Arc::new(FileBookmarks::open(path.join("books"))
            .context(ErrorKind::StateOpen(StateOpenError::Bookmarks))?);
        let blobstore = Fileblob::open(path.join("blobs"))
            .context(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
        let linknodes = Arc::new(FileLinknodes::open(path.join("linknodes"))
            .context(ErrorKind::StateOpen(StateOpenError::Linknodes))?);

        Ok(Self::new(
            Arc::new(heads),
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(linknodes),
        ))
    }

    pub fn new_rocksdb(path: &Path) -> Result<Self> {
        let heads = FileHeads::open(path.join("heads"))
            .context(ErrorKind::StateOpen(StateOpenError::Heads))?;
        let bookmarks = FileBookmarks::open(path.join("books"))
            .context(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;
        let blobstore = Rocksblob::open(path.join("blobs"))
            .context(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
        let linknodes = FileLinknodes::open(path.join("linknodes"))
            .context(ErrorKind::StateOpen(StateOpenError::Linknodes))?;

        Ok(Self::new(
            Arc::new(heads),
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(linknodes),
        ))
    }

    pub fn new_memblob(
        heads: MemHeads,
        bookmarks: MemBookmarks,
        blobstore: Memblob,
        linknodes: MemLinknodes,
    ) -> Self {
        Self::new(
            Arc::new(heads),
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(linknodes),
        )
    }

    pub fn new_test_manifold<T: ToString>(bucket: T, remote: &Remote) -> Result<Self> {
        let heads = MemHeads::new();
        let bookmarks = MemBookmarks::new();
        let blobstore = ManifoldBlob::new_may_panic(bucket.to_string(), remote);
        let linknodes = MemLinknodes::new();
        Ok(Self::new(
            Arc::new(heads),
            Arc::new(bookmarks),
            Arc::new(blobstore),
            Arc::new(linknodes),
        ))
    }

    pub fn get_blob(&self, key: &NodeHash) -> BoxFuture<Vec<u8>, Error> {
        fetch_blob_from_blobstore(&self.blobstore, *key)
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
        BlobChangeset::load(&self.blobstore, &changesetid)
            .map(|cs| cs.is_some())
            .boxify()
    }

    pub fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Error> {
        let changesetid = ChangesetId::new(*nodeid);
        let nodeid = *nodeid;
        BlobChangeset::load(&self.blobstore, &changesetid)
            .and_then(move |cs| cs.ok_or(ErrorKind::ChangesetMissing(nodeid).into()))
            .map(|cs| cs.boxed())
            .boxify()
    }

    pub fn get_manifest_by_nodeid(
        &self,
        nodeid: &NodeHash,
    ) -> BoxFuture<Box<Manifest + Sync>, Error> {
        let nodeid = *nodeid;
        BlobManifest::load(&self.blobstore, &nodeid)
            .and_then(move |mf| mf.ok_or(ErrorKind::ManifestMissing(nodeid).into()))
            .map(|m| m.boxed())
            .boxify()
    }

    pub fn get_bookmark_keys(&self) -> BoxStream<Vec<u8>, Error> {
        self.bookmarks.keys().boxify()
    }

    pub fn get_bookmark_value(
        &self,
        key: &AsRef<[u8]>,
    ) -> BoxFuture<Option<(NodeHash, Version)>, Error> {
        self.bookmarks.get(key).boxify()
    }
}

impl Clone for BlobRepo {
    fn clone(&self) -> Self {
        Self {
            heads: self.heads.clone(),
            bookmarks: self.bookmarks.clone(),
            blobstore: self.blobstore.clone(),
            linknodes: self.linknodes.clone(),
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
    WaitCS(NodeHash, BoxFuture<Box<Changeset>, Error>),
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
                            WaitCS(next, self.repo.get_changeset_by_nodeid(&next))
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
