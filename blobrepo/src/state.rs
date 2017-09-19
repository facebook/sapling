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
use heads::Heads;
use memblob::Memblob;
use membookmarks::MemBookmarks;
use memheads::MemHeads;
use mercurial_types::NodeHash;
use rocksblob::Rocksblob;

use errors::*;

/// Represents all the state used by a blob store.
pub trait BlobState: 'static + Send + Sync {
    type Heads: Heads<Key = NodeHash> + Sync;
    type Bookmarks: Bookmarks<Value = NodeHash> + Clone + Sync;
    type Blobstore: Blobstore<Key = String> + Clone + Sync;

    fn heads(&self) -> &Self::Heads;
    fn bookmarks(&self) -> &Self::Bookmarks;
    fn blobstore(&self) -> &Self::Blobstore;
}

macro_rules! impl_blob_state {
    {
        $struct_type: ident {
            heads: $head_type: ty,
            bookmarks: $book_type: ty,
            blobstore: $blob_type: ty,
        }
    } => {
        pub struct $struct_type {
            heads: $head_type,
            bookmarks: $book_type,
            blobstore: $blob_type,
        }

        impl BlobState for $struct_type {
            type Heads = $head_type;
            type Bookmarks = $book_type;
            type Blobstore = $blob_type;

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
        }
    }
}

impl_blob_state! {
    FilesBlobState {
        heads: FileHeads<NodeHash>,
        bookmarks: Arc<FileBookmarks<NodeHash>>,
        blobstore: Fileblob<String, Vec<u8>>,
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

        Ok(FilesBlobState {
            heads,
            bookmarks,
            blobstore,
        })
    }
}

impl_blob_state! {
    RocksBlobState {
        heads: FileHeads<NodeHash>,
        bookmarks: Arc<FileBookmarks<NodeHash>>,
        blobstore: Rocksblob<String>,
    }
}

impl RocksBlobState {
    pub fn new(path: &Path) -> Result<Self> {
        let heads = FileHeads::open(path.with_extension("heads"))
            .chain_err(|| ErrorKind::StateOpen(StateOpenError::Heads))?;
        let bookmarks = Arc::new(
            FileBookmarks::open(path.with_extension("books"))
                .chain_err(|| ErrorKind::StateOpen(StateOpenError::Bookmarks))?,
        );
        let blobstore = Rocksblob::open(path.with_extension("rocks"))
            .chain_err(|| ErrorKind::StateOpen(StateOpenError::Blobstore))?;

        Ok(RocksBlobState {
            heads,
            bookmarks,
            blobstore,
        })
    }
}

impl_blob_state! {
    MemBlobState {
        heads: MemHeads<NodeHash>,
        bookmarks: Arc<MemBookmarks<NodeHash>>,
        blobstore: Memblob,
    }
}

impl MemBlobState {
    pub fn new(
        heads: MemHeads<NodeHash>,
        bookmarks: MemBookmarks<NodeHash>,
        blobstore: Memblob,
    ) -> Self {
        MemBlobState {
            heads,
            bookmarks: Arc::new(bookmarks),
            blobstore,
        }
    }
}
