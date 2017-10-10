// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate futures;
extern crate serde;
#[macro_use]
extern crate serde_derive;

extern crate futures_ext;

use std::error;
use std::marker::PhantomData;
use std::sync::Arc;

use futures::{Future, Stream};

use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

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
    type Keys: Stream<Item = Vec<u8>, Error = Self::Error> + Send + 'static;

    // Basic operations.
    fn get(&self, key: &AsRef<[u8]>) -> Self::Get;
    fn keys(&self) -> Self::Keys;
}

pub struct BoxedBookmarks<B, E>
where
    B: Bookmarks,
{
    inner: B,
    cvt_err: fn(B::Error) -> E,
    _phantom: PhantomData<E>,
}

impl<B, E> BoxedBookmarks<B, E>
where
    B: Bookmarks,
    E: error::Error + Send + 'static,
{
    pub fn new(
        inner: B,
    ) -> Box<
        Bookmarks<
            Value = B::Value,
            Error = E,
            Get = BoxFuture<Option<(B::Value, Version)>, E>,
            Keys = BoxStream<Vec<u8>, E>,
        >,
    >
    where
        E: From<B::Error>,
    {
        Self::new_cvt(inner, E::from)
    }

    pub fn new_cvt(
        inner: B,
        cvt_err: fn(B::Error) -> E,
    ) -> Box<
        Bookmarks<
            Value = B::Value,
            Error = E,
            Get = BoxFuture<Option<(B::Value, Version)>, E>,
            Keys = BoxStream<Vec<u8>, E>,
        >,
    > {
        let res = Self {
            inner,
            cvt_err,
            _phantom: PhantomData,
        };
        Box::new(res)
    }
}

impl<B, E> Bookmarks for BoxedBookmarks<B, E>
where
    B: Bookmarks + Send + 'static,
    B::Value: Send + 'static,
    E: error::Error + Send + 'static,
{
    type Error = E;
    type Value = B::Value;
    type Get = BoxFuture<Option<(Self::Value, Version)>, E>;
    type Keys = BoxStream<Vec<u8>, E>;

    fn get(&self, key: &AsRef<[u8]>) -> Self::Get {
        self.inner.get(key).map_err(self.cvt_err).boxify()
    }

    fn keys(&self) -> Self::Keys {
        self.inner.keys().map_err(self.cvt_err).boxify()
    }
}

// Implement Bookmarks for boxed Bookmarks trait object
impl<V, E, G, K> Bookmarks for Box<Bookmarks<Value = V, Error = E, Get = G, Keys = K>>
where
    V: Send + 'static,
    E: error::Error + Send + 'static,
    G: Future<Item = Option<(V, Version)>, Error = E> + Send + 'static,
    K: Stream<Item = Vec<u8>, Error = E> + Send + 'static,
{
    type Value = V;
    type Error = E;
    type Get = G;
    type Keys = K;

    fn get(&self, key: &AsRef<[u8]>) -> Self::Get {
        (**self).get(key)
    }

    fn keys(&self) -> Self::Keys {
        (**self).keys()
    }
}

// Implement Bookmarks for Arced Bookmarks trait object
impl<V, E, G, K> Bookmarks for Arc<Bookmarks<Value = V, Error = E, Get = G, Keys = K> + Sync>
where
    V: Send + 'static,
    E: error::Error + Send + 'static,
    G: Future<Item = Option<(V, Version)>, Error = E> + Send + 'static,
    K: Stream<Item = Vec<u8>, Error = E> + Send + 'static,
{
    type Value = V;
    type Error = E;
    type Get = G;
    type Keys = K;

    fn get(&self, key: &AsRef<[u8]>) -> Self::Get {
        (**self).get(key)
    }

    fn keys(&self) -> Self::Keys {
        (**self).keys()
    }
}

// Implement Bookmarks for Arc-wrapped Bookmark type
impl<B> Bookmarks for Arc<B>
where
    B: Bookmarks + Sync,
{
    type Value = B::Value;
    type Error = B::Error;
    type Get = B::Get;
    type Keys = B::Keys;

    fn get(&self, key: &AsRef<[u8]>) -> Self::Get {
        (**self).get(key)
    }

    fn keys(&self) -> Self::Keys {
        (**self).keys()
    }
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

    let _: Box<
        Bookmarks<Value = Vec<u8>, Error = io::Error, Get = GetFuture, Keys = KeysStream>,
    >;
    let _: Box<
        Bookmarks<Value = Vec<u8>, Error = io::Error, Get = GetFuture, Keys = KeysStream>,
    >;
    let _: Box<
        BookmarksMut<
            Value = Vec<u8>,
            Error = io::Error,
            Get = GetFuture,
            Keys = KeysStream,
            Set = SetFuture,
        >,
    >;
}
