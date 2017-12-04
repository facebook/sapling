// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::path::Path;
use std::sync::Arc;

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
use rocksblob::Rocksblob;
use tokio_core::reactor::Remote;

use errors::*;

/// Represents all the state used by a blob store.
pub trait BlobState: 'static + Send + Sync {
    type Heads: Heads + Sync;
    type Bookmarks: Bookmarks + Clone + Sync;
    type Blobstore: Blobstore + Clone + Sync;
    type Linknodes: Linknodes + Clone;

    fn heads(&self) -> &Self::Heads;
    fn bookmarks(&self) -> &Self::Bookmarks;
    fn blobstore(&self) -> &Self::Blobstore;
    fn linknodes(&self) -> &Self::Linknodes;
}

macro_rules! impl_blob_state {
    {
        $struct_type: ident {
            heads: $head_type: ty,
            bookmarks: $book_type: ty,
            blobstore: $blob_type: ty,
            linknodes: $link_type: ty,
        }
    } => {
        pub struct $struct_type {
            heads: $head_type,
            bookmarks: $book_type,
            blobstore: $blob_type,
            linknodes: $link_type,
        }

        impl BlobState for $struct_type {
            type Heads = $head_type;
            type Bookmarks = $book_type;
            type Blobstore = $blob_type;
            type Linknodes = $link_type;

            #[inline]
            fn heads(&self) -> &Self::Heads {
                &self.heads
            }

            #[inline]
            fn bookmarks(&self) -> &Self::Bookmarks {
                &self.bookmarks
            }

            #[inline]
            fn blobstore(&self) -> &Self::Blobstore {
                &self.blobstore
            }

            #[inline]
            fn linknodes(&self) -> &Self::Linknodes {
                &self.linknodes
            }
        }
    }
}

impl_blob_state! {
    FilesBlobState {
        heads: FileHeads,
        bookmarks: Arc<FileBookmarks>,
        blobstore: Fileblob,
        linknodes: Arc<FileLinknodes>,
    }
}

impl FilesBlobState {
    pub fn new(path: &Path) -> Result<Self> {
        let heads = FileHeads::open(path.join("heads"))
            .chain_err(|| ErrorKind::StateOpen(StateOpenError::Heads))?;
        let bookmarks = Arc::new(
            FileBookmarks::open(path.join("books"))
                .chain_err(|| ErrorKind::StateOpen(StateOpenError::Bookmarks))?,
        );
        let blobstore = Fileblob::open(path.join("blobs"))
            .chain_err(|| ErrorKind::StateOpen(StateOpenError::Blobstore))?;
        let linknodes = Arc::new(
            FileLinknodes::open(path.join("linknodes"))
                .chain_err(|| ErrorKind::StateOpen(StateOpenError::Linknodes))?,
        );

        Ok(FilesBlobState {
            heads,
            bookmarks,
            blobstore,
            linknodes,
        })
    }
}

impl_blob_state! {
    RocksBlobState {
        heads: FileHeads,
        bookmarks: Arc<FileBookmarks>,
        blobstore: Rocksblob,
        linknodes: Arc<FileLinknodes>,
    }
}

impl RocksBlobState {
    pub fn new(path: &Path) -> Result<Self> {
        let heads = FileHeads::open(path.join("heads"))
            .chain_err(|| ErrorKind::StateOpen(StateOpenError::Heads))?;
        let bookmarks = Arc::new(
            FileBookmarks::open(path.join("books"))
                .chain_err(|| ErrorKind::StateOpen(StateOpenError::Bookmarks))?,
        );
        let blobstore = Rocksblob::open(path.join("blobs"))
            .chain_err(|| ErrorKind::StateOpen(StateOpenError::Blobstore))?;
        let linknodes = Arc::new(
            FileLinknodes::open(path.join("linknodes"))
                .chain_err(|| ErrorKind::StateOpen(StateOpenError::Linknodes))?,
        );


        Ok(RocksBlobState {
            heads,
            bookmarks,
            blobstore,
            linknodes,
        })
    }
}

impl_blob_state! {
    MemBlobState {
        heads: MemHeads,
        bookmarks: Arc<MemBookmarks>,
        blobstore: Memblob,
        linknodes: Arc<MemLinknodes>,
    }
}

impl MemBlobState {
    pub fn new(
        heads: MemHeads,
        bookmarks: MemBookmarks,
        blobstore: Memblob,
        linknodes: MemLinknodes,
    ) -> Self {
        MemBlobState {
            heads,
            bookmarks: Arc::new(bookmarks),
            blobstore,
            linknodes: Arc::new(linknodes),
        }
    }
}

impl_blob_state! {
    TestManifoldBlobState {
        heads: MemHeads,
        bookmarks: Arc<MemBookmarks>,
        blobstore: ManifoldBlob,
        linknodes: Arc<MemLinknodes>,
    }
}

impl TestManifoldBlobState {
    pub fn new<T: ToString>(bucket: T, remote: &Remote) -> Result<Self> {
        let heads = MemHeads::new();
        let bookmarks = Arc::new(MemBookmarks::new());
        let blobstore = ManifoldBlob::new_may_panic(bucket.to_string(), remote);
        let linknodes = Arc::new(MemLinknodes::new());
        Ok(TestManifoldBlobState {
            heads,
            bookmarks,
            blobstore,
            linknodes,
        })
    }
}
