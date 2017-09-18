// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashSet;
use std::mem;
use std::sync::Arc;

use futures::{Async, Poll};
use futures::future::Future;
use futures::stream::{self, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use blobstore::Blobstore;
use bookmarks::{Bookmarks, BoxedBookmarks};
use heads::Heads;
use mercurial_types::{repo, Changeset, Manifest, NodeHash, Repo};

use BlobChangeset;
use BlobManifest;
use errors::*;
use file::fetch_file_blob_from_blobstore;

pub struct BlobRepo<Head, Book, Blob> {
    inner: Arc<BlobRepoInner<Head, Book, Blob>>,
}

struct BlobRepoInner<Head, Book, Blob> {
    bookmarks: Arc<Book>,
    heads: Head,
    blobstore: Blob,
}

impl<Head, Book, Blob> BlobRepo<Head, Book, Blob> {
    pub fn new(heads: Head, bookmarks: Book, blobstore: Blob) -> Self {
        Self {
            inner: Arc::new(BlobRepoInner {
                heads,
                bookmarks: Arc::new(bookmarks),
                blobstore,
            }),
        }
    }
}

impl<Head, Book, Blob> BlobRepo<Head, Book, Blob>
where
    Blob: Blobstore<Key = String> + Clone + Sync,
{
    pub fn get_file_blob(&self, key: &NodeHash) -> BoxFuture<Vec<u8>, Error> {
        fetch_file_blob_from_blobstore(self.inner.blobstore.clone(), *key)
    }
}

impl<Head, Book, Blob> Repo for BlobRepo<Head, Book, Blob>
where
    Head: Heads<Key = NodeHash> + Sync,
    Book: Bookmarks<Value = NodeHash> + Sync,
    Blob: Blobstore<Key = String> + Clone + Sync,
{
    type Error = Error;

    fn get_changesets(&self) -> BoxStream<NodeHash, Self::Error> {
        BlobChangesetStream {
            repo: BlobRepo {
                inner: self.inner.clone(),
            },
            heads: self.inner.heads.heads().map_err(heads_err).boxify(),
            state: BCState::Idle,
            seen: HashSet::new(),
        }.boxify()
    }

    fn get_heads(&self) -> BoxStream<NodeHash, Self::Error> {
        self.inner.heads.heads().map_err(heads_err).boxify()
    }

    fn changeset_exists(&self, nodeid: &NodeHash) -> BoxFuture<bool, Self::Error> {
        BlobChangeset::load(&self.inner.blobstore, nodeid)
            .map(|cs| cs.is_some())
            .boxify()
    }

    fn get_changeset_by_nodeid(&self, nodeid: &NodeHash) -> BoxFuture<Box<Changeset>, Self::Error> {
        let nodeid = *nodeid;
        BlobChangeset::load(&self.inner.blobstore, &nodeid)
            .and_then(move |cs| {
                cs.ok_or(ErrorKind::ChangesetMissing(nodeid).into())
            })
            .map(|cs| cs.boxed())
            .boxify()
    }

    fn get_manifest_by_nodeid(
        &self,
        nodeid: &NodeHash,
    ) -> BoxFuture<Box<Manifest<Error = Self::Error> + Sync>, Self::Error> {
        let nodeid = *nodeid;
        BlobManifest::load(&self.inner.blobstore, &nodeid)
            .and_then(move |mf| {
                mf.ok_or(ErrorKind::ManifestMissing(nodeid).into())
            })
            .map(|m| m.boxed())
            .boxify()
    }

    fn get_bookmarks(&self) -> Result<repo::BoxedBookmarks<Self::Error>> {
        let res = self.inner.bookmarks.clone();

        Ok(BoxedBookmarks::new_cvt(res, bookmarks_err))
    }
}

impl<Head, Book, Blob> Clone for BlobRepo<Head, Book, Blob> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

pub struct BlobChangesetStream<Head, Book, Blob>
where
    Head: Heads<Key = NodeHash> + Sync,
    Book: Bookmarks + Sync,
    Blob: Blobstore<Key = String> + Clone + Sync,
{
    repo: BlobRepo<Head, Book, Blob>,
    seen: HashSet<NodeHash>,
    heads: BoxStream<NodeHash, Error>,
    state: BCState,
}

enum BCState {
    Idle,
    WaitCS(NodeHash, BoxFuture<Box<Changeset>, Error>),
}

impl<Head, Book, Blob> Stream for BlobChangesetStream<Head, Book, Blob>
where
    Head: Heads<Key = NodeHash> + Sync,
    Book: Bookmarks<Value = NodeHash> + Sync,
    Blob: Blobstore<Key = String> + Clone + Sync,
{
    type Item = NodeHash;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
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
