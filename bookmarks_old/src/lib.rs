// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate failure;
extern crate futures;

extern crate futures_ext;
extern crate mercurial_types;
extern crate storage_types;

use std::sync::Arc;

use futures_ext::{BoxFuture, BoxStream};

use mercurial_types::nodehash::HgChangesetId;
use storage_types::Version;

use failure::Error;

/// Trait representing read-only operations on a bookmark store, which maintains a global mapping
/// of names to commit identifiers. Consistency is maintained using versioning.
pub trait Bookmarks: Sync + Send + 'static {
    // Basic operations.
    fn get(&self, key: &AsRef<[u8]>) -> BoxFuture<Option<(HgChangesetId, Version)>, Error>;
    fn keys(&self) -> BoxStream<Vec<u8>, Error>;
}

// Implement Bookmarks for boxed Bookmarks trait object
impl Bookmarks for Box<Bookmarks> {
    fn get(&self, key: &AsRef<[u8]>) -> BoxFuture<Option<(HgChangesetId, Version)>, Error> {
        (**self).get(key)
    }

    fn keys(&self) -> BoxStream<Vec<u8>, Error> {
        (**self).keys()
    }
}

// Implement Bookmarks for Arced Bookmarks trait object
impl Bookmarks for Arc<Bookmarks> {
    fn get(&self, key: &AsRef<[u8]>) -> BoxFuture<Option<(HgChangesetId, Version)>, Error> {
        (**self).get(key)
    }

    fn keys(&self) -> BoxStream<Vec<u8>, Error> {
        (**self).keys()
    }
}

// Implement Bookmarks for Arc-wrapped Bookmark type
impl<B> Bookmarks for Arc<B>
where
    B: Bookmarks,
{
    fn get(&self, key: &AsRef<[u8]>) -> BoxFuture<Option<(HgChangesetId, Version)>, Error> {
        (**self).get(key)
    }

    fn keys(&self) -> BoxStream<Vec<u8>, Error> {
        (**self).keys()
    }
}

/// Trait representing write operations on a bookmark store. Consistency is maintained using
/// versioning.
pub trait BookmarksMut: Bookmarks {
    // Return type for updating a bookmark value. Must be a future that resolves to either
    // a new version or None if the operation couldn't be completed due to a version mismatch.
    // Basic operations.
    fn set(&self, key: &AsRef<[u8]>, &HgChangesetId, &Version) -> BoxFuture<Option<Version>, Error>;
    fn delete(&self, key: &AsRef<[u8]>, &Version) -> BoxFuture<Option<Version>, Error>;

    // Convenience function for creating new bookmarks (since initial version is always "absent").
    fn create(&self, key: &AsRef<[u8]>, value: &HgChangesetId) -> BoxFuture<Option<Version>, Error> {
        self.set(key, value, &Version::absent())
    }
}
