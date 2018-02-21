// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate bytes;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate tokio_core;

use std::sync::Arc;

use bytes::Bytes;

use failure::Error;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Blob {} not found in blobstore", _0)] NotFound(String),
}

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
pub trait Blobstore: Send + Sync + 'static {
    fn get(&self, key: String) -> BoxFuture<Option<Bytes>, Error>;
    // The underlying implementation is allowed to assume that the value for a given key is always
    // the same. Thus, it's legitimate for an implementation to do
    // "self.assert_present(key).or_else()" and never upload the same key twice.
    fn put(&self, key: String, value: Bytes) -> BoxFuture<(), Error>;
    // Allows the underlying Blobstore to skip the download phase
    fn is_present(&self, key: String) -> BoxFuture<bool, Error> {
        self.get(key).map(|opt| opt.is_some()).boxify()
    }
    fn assert_present(&self, key: String) -> BoxFuture<(), Error> {
        self.is_present(key.clone())
            .and_then(|present| {
                if present {
                    future::ok(())
                } else {
                    future::err(ErrorKind::NotFound(key).into())
                }
            })
            .boxify()
    }
}

impl Blobstore for Arc<Blobstore> {
    fn get(&self, key: String) -> BoxFuture<Option<Bytes>, Error> {
        self.as_ref().get(key)
    }
    fn put(&self, key: String, value: Bytes) -> BoxFuture<(), Error> {
        self.as_ref().put(key, value)
    }
    fn is_present(&self, key: String) -> BoxFuture<bool, Error> {
        self.as_ref().is_present(key)
    }
    fn assert_present(&self, key: String) -> BoxFuture<(), Error> {
        self.as_ref().assert_present(key)
    }
}

impl Blobstore for Box<Blobstore> {
    fn get(&self, key: String) -> BoxFuture<Option<Bytes>, Error> {
        self.as_ref().get(key)
    }
    fn put(&self, key: String, value: Bytes) -> BoxFuture<(), Error> {
        self.as_ref().put(key, value)
    }
    fn is_present(&self, key: String) -> BoxFuture<bool, Error> {
        self.as_ref().is_present(key)
    }
    fn assert_present(&self, key: String) -> BoxFuture<(), Error> {
        self.as_ref().assert_present(key)
    }
}
