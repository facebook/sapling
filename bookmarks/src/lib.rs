// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate futures;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use futures::{Future, Stream};
use std::error;

/// Versions are used to ensure consistency of state across all users of the bookmark store.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Version(Option<u64>);

impl Version {
    pub fn absent() -> Self {
        Version::default()
    }
}

impl From<u64> for Version {
    fn from(v: u64) -> Self {
        Version(Some(v))
    }
}

impl Default for Version {
    fn default() -> Self {
        Version(None)
    }
}

/// Trait representing read-only operations on a bookmark store, which maintains a global mapping
/// of names to commit identifiers. Consistency is maintained using versioning.
pub trait Bookmarks: Send + 'static {
    type Value: Send + 'static;
    type Error: error::Error + Send + 'static;

    // Return type for getting a bookmark value. Must be a future that resolves to
    // a tuple of the current value and current version of the bookmark.
    type Get: Future<Item = Option<(Self::Value, Version)>, Error = Self::Error> + Send + 'static;

    // Basic operations.
    fn get(&self, key: &AsRef<[u8]>) -> Self::Get;
}

/// Trait representing write operations on a bookmark store. Consistency is maintained using
/// versioning.
pub trait BookmarksMut: Bookmarks {
    // Return type for updating a bookmark value. Must be a future that resolves to either
    // a new version or None if the operation couldn't be completed due to a version mismatch.
    type Set: Future<Item = Option<Version>, Error = Self::Error> + Send + 'static;

    // Basic operations.
    fn set(&self, key: &AsRef<[u8]>, &Self::Value, &Version) -> Self::Set;
    fn delete(&self, key: &AsRef<[u8]>, &Version) -> Self::Set;

    // Convenience function for creating new bookmarks (since initial version is always "absent").
    fn create(&self, key: &AsRef<[u8]>, value: &Self::Value) -> Self::Set {
        self.set(key, value, &Version::absent())
    }
}

/// Bookmark stores that implement this trait support efficiently enumerating all of
/// the stored bookmarks. Bookmarks are not guaranteed to be returned in any particular order.
pub trait ListBookmarks: Bookmarks {
    type Keys: Stream<Item = Vec<u8>, Error = Self::Error> + Send + 'static;
    fn keys(&self) -> Self::Keys;
}

/// Convenience trait to capture listing bookmarks and write operations.
pub trait ListBookmarksMut: ListBookmarks + BookmarksMut {}

/// Ensure that trait objects can be created from the traits here.
fn _assert_objects() {
    use std::io;

    use futures::future::{FutureResult, IntoStream};

    type GetFuture = FutureResult<Option<(Vec<u8>, Version)>, io::Error>;
    type SetFuture = FutureResult<Option<Version>, io::Error>;
    type KeysStream = IntoStream<FutureResult<Vec<u8>, io::Error>>;

    // TODO: we definitely need Bookmarks and ListBookmarks to be trait objects, but do we also
    // need BookmarksMut and ListBookmarksMut to be trait objects? If not, then it may be possible
    // to make set, delete and create to have an AsRef type param rather than taking an AsRef trait
    // object.

    let _: Box<Bookmarks<Value = Vec<u8>, Error = io::Error, Get = GetFuture>>;
    let _: Box<
        BookmarksMut<Value = Vec<u8>, Error = io::Error, Get = GetFuture, Set = SetFuture>,
    >;
    let _: Box<
        ListBookmarks<Value = Vec<u8>, Error = io::Error, Get = GetFuture, Keys = KeysStream>,
    >;
    let _: Box<
        ListBookmarksMut<
            Value = Vec<u8>,
            Error = io::Error,
            Get = GetFuture,
            Set = SetFuture,
            Keys = KeysStream,
        >,
    >;
}
