// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::error;
use std::marker::PhantomData;
use std::sync::Arc;

use bytes::Bytes;
use futures::Future;

use futures_ext::{BoxFuture, FutureExt};

use super::*;

pub type BoxBlobstore<E> = Box<
    Blobstore<Error = E, GetBlob = BoxFuture<Option<Bytes>, E>, PutBlob = BoxFuture<(), E>>,
>;

pub type ArcBlobstore<E> = Arc<
    Blobstore<Error = E, GetBlob = BoxFuture<Option<Bytes>, E>, PutBlob = BoxFuture<(), E>> + Sync,
>;

/// Take a concrete `BlobStore` implementation and box it up in a generic way.
/// In addition to boxing the blobstore type into trait object, it also conforms
/// the `Value*` and `Error` associated types to some type. This allows the caller
/// to conform multiple different blobstore implementations into a single boxed trait
/// object type with uniform errors and value types.
pub fn boxed<B, E>(blobstore: B) -> BoxBlobstore<E>
where
    B: Blobstore + Send + 'static,
    B::GetBlob: Send + 'static,
    B::PutBlob: Send + 'static,
    E: error::Error + From<B::Error> + Send + 'static,
{
    let new = BlobstoreInner {
        blobstore,
        _phantom: PhantomData,
    };
    Box::new(new)
}

pub fn arced<B, E>(blobstore: B) -> ArcBlobstore<E>
where
    B: Blobstore + Sync + Send + 'static,
    B::GetBlob: Send + 'static,
    B::PutBlob: Send + 'static,
    E: error::Error + From<B::Error> + Send + 'static,
{
    let new = BlobstoreInner {
        blobstore,
        _phantom: PhantomData,
    };
    Arc::new(new)
}

struct BlobstoreInner<B, E> {
    blobstore: B,
    _phantom: PhantomData<E>,
}

// Set Sync marker iff B is Sync - the rest don't matter in a PhantomData.
unsafe impl<B, E> Sync for BlobstoreInner<B, E>
where
    B: Sync,
{
}

impl<B, E> Blobstore for BlobstoreInner<B, E>
where
    B: Blobstore + Send + 'static,
    B::GetBlob: Send + 'static,
    B::PutBlob: Send + 'static,
    E: error::Error + From<B::Error> + Send + 'static,
{
    type Error = E;

    type GetBlob = BoxFuture<Option<Bytes>, Self::Error>;
    type PutBlob = BoxFuture<(), Self::Error>;

    fn get(&self, key: String) -> Self::GetBlob {
        self.blobstore.get(key).map_err(E::from).boxify()
    }

    fn put(&self, key: String, value: Bytes) -> Self::PutBlob {
        self.blobstore.put(key, value).map_err(E::from).boxify()
    }
}
