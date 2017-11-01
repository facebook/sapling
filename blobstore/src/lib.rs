// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate futures;

extern crate futures_ext;

extern crate tokio_core;

use std::error;
use std::sync::Arc;

use futures::Future;

mod boxed;
mod retrying;

pub use boxed::{ArcBlobstore, BoxBlobstore};
pub use retrying::RetryingBlobstore;

/// Basic trait for the Blob Store interface
///
/// Very simple for now, but main point is that it's async from the start.
///
// TODO: All the associated types make it non-object-safe, so it can't be
// used as a trait object. Work out if that's an issue or not. It will scale
// badly if it gets a lot more methods.
//
// Other design considerations:
//
// Has blob?
// Blob probe might be useful, but it's a performance optimization. If delete exists, then it can
// only ever be a hint (because of probe vs delete race). If range gets exist, then it can be
// emulated by asking for a zero-byte range (with the proviso that this operation must actually
// check the key exists, even if it never materializes any data). A related operation is a verify,
// to check that the blob integrity is OK, even if we don't actually fetch the data.
//
// Delete blob?
// I'll avoid delete for now. The current design for Mononoke doesn't need delete for normal
// operations. If delete is going to be needed then it will be some maintenance operation like gc,
// but that opens up a whole pile of other design questions that we haven't got to yet.
//
// Metadata?
// Will definitely need some kind of metadata interface. The open questions there are:
// - set once, or mutable after?
// - arbitrary user-defined, or pre-defined (size, sha1, etc)? Perhaps different interfaces for
//   each?
// - blob content type (ie, how to parse). Esp useful for "does this blob refer to other blobs, and
//   which ones"? If that exists, then blobstore goes from being lots of discrete blobs to a graph.
//   But it also requires careful thought about how the type id relates to keys.
// - generation number? Useful for making sure that partial/range gets are consistent.
// - e2e checksums?
//
// Batch ops?
// The interface is async so that clients can issue lots of discrete ops to keep the pipeline full.
// An implementation can have batching under the covers if it makes sense. In general I find
// batching is a design antipattern that should be avoided. (Manifold also avoids batching in
// favour of lots of concurrent requests.)
//
// Consistency guarantees?
// I'm not sure about what consistency guarantees to make at this interface level. I'm tempted to
// make them fairly strong, so that an implementation based on a store with weaker consistency is
// responsible for implementing strong consistency. At the very least:
// - successful `put` response means that the data is durable
// - puts are atomic, last put wins (no tearing, ordering determined by implementation)
// - single gets are atomic (no tearing), partial/range gets use generation number for consistency
// - `get` can return stale data for a bounded time (but strong put-get consistency would be
//   better)
//
// "Durability" is defined by the implementation - durable for an in-memory hashtable doesn't mean
// much: if the overall store is OK then individual blobs should be OK, but process crash will lose
// the whole store. For something based on everstore, it would mean globally replicated, able to
// survive losing multiple regions.
//
// Conditional ops?
// There's no requirement for conditional ops in Mononoke; the plan is to use a separate mechanism
// based on Zookeeper (or similar) for global coordination. The only conditional op might be to
// separate "create" (fails if already exists) and "update" (fails if doesn't exist), but mostly as
// a bug-finding consistency check.
//
// How to deal with very large objects?
// - streaming get/put?
// - range get/put? (how does range put work? put-put-put-commit?)
pub trait Blobstore: Send + 'static {
    type Key: Send + 'static;
    type ValueIn: Send + 'static;
    type ValueOut: AsRef<[u8]> + Send + 'static;
    type Error: error::Error + Send + 'static;

    type GetBlob: Future<Item = Option<Self::ValueOut>, Error = Self::Error> + Send + 'static;
    type PutBlob: Future<Item = (), Error = Self::Error> + Send + 'static;

    fn get(&self, key: &Self::Key) -> Self::GetBlob;
    fn put(&self, key: Self::Key, value: Self::ValueIn) -> Self::PutBlob;

    fn boxed<Vi, Vo, E>(self) -> BoxBlobstore<Self::Key, Vi, Vo, E>
    where
        Self: Sized,
        Self::Key: Sized,
        Self::ValueIn: From<Vi>,
        Vi: Send + 'static,
        Vo: From<Self::ValueOut> + AsRef<[u8]> + Send + 'static,
        E: error::Error + From<Self::Error> + Send + 'static,
    {
        boxed::boxed(self)
    }

    fn arced<Vi, Vo, E>(self) -> ArcBlobstore<Self::Key, Vi, Vo, E>
    where
        Self: Sync + Sized,
        Self::Key: Sized,
        Self::ValueIn: From<Vi>,
        Vi: Send + 'static,
        Vo: From<Self::ValueOut> + AsRef<[u8]> + Send + 'static,
        E: error::Error + From<Self::Error> + Send + 'static,
    {
        boxed::arced(self)
    }
}

impl<K, Vi, Vo, E, GB, PB> Blobstore
    for Arc<
        Blobstore<Key = K, ValueIn = Vi, ValueOut = Vo, Error = E, GetBlob = GB, PutBlob = PB>
            + Sync,
    > where
    K: Send + 'static,
    Vi: Send + 'static,
    Vo: AsRef<[u8]> + Send + 'static,
    E: error::Error + Send + 'static,
    GB: Future<Item = Option<Vo>, Error = E> + Send + 'static,
    PB: Future<Item = (), Error = E> + Send + 'static,
{
    type Key = K;
    type ValueIn = Vi;
    type ValueOut = Vo;
    type Error = E;
    type GetBlob = GB;
    type PutBlob = PB;

    fn get(&self, key: &Self::Key) -> Self::GetBlob {
        self.as_ref().get(key)
    }

    fn put(&self, key: Self::Key, val: Self::ValueIn) -> Self::PutBlob {
        self.as_ref().put(key, val)
    }
}

impl<K, Vi, Vo, E, GB, PB> Blobstore
    for Box<Blobstore<Key = K, ValueIn = Vi, ValueOut = Vo, Error = E, GetBlob = GB, PutBlob = PB>>
where
    K: Send + 'static,
    Vi: Send + 'static,
    Vo: AsRef<[u8]> + Send + 'static,
    E: error::Error + Send + 'static,
    GB: Future<Item = Option<Vo>, Error = E> + Send + 'static,
    PB: Future<Item = (), Error = E> + Send + 'static,
{
    type Key = K;
    type ValueIn = Vi;
    type ValueOut = Vo;
    type Error = E;
    type GetBlob = GB;
    type PutBlob = PB;

    fn get(&self, key: &Self::Key) -> Self::GetBlob {
        self.as_ref().get(key)
    }

    fn put(&self, key: Self::Key, val: Self::ValueIn) -> Self::PutBlob {
        self.as_ref().put(key, val)
    }
}
