// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::error;
use std::marker::PhantomData;
use std::sync::Arc;

use futures::Future;

use futures_ext::{BoxFuture, FutureExt};

use super::*;

pub type BoxBlobstore<K, Vi, Vo, E> = Box<
    Blobstore<
        Key = K,
        ValueIn = Vi,
        ValueOut = Vo,
        Error = E,
        GetBlob = BoxFuture<Option<Vo>, E>,
        PutBlob = BoxFuture<(), E>,
    >,
>;

pub type ArcBlobstore<K, Vi, Vo, E> = Arc<
    Blobstore<
        Key = K,
        ValueIn = Vi,
        ValueOut = Vo,
        Error = E,
        GetBlob = BoxFuture<Option<Vo>, E>,
        PutBlob = BoxFuture<(), E>,
    >
        + Sync,
>;

/// Take a concrete `BlobStore` implementation and box it up in a generic way.
/// In addition to boxing the blobstore type into trait object, it also conforms
/// the `Value*` and `Error` associated types to some type. This allows the caller
/// to conform multiple different blobstore implementations into a single boxed trait
/// object type with uniform errors and value types.
pub fn boxed<B, Vi, Vo, E>(blobstore: B) -> BoxBlobstore<B::Key, Vi, Vo, E>
where
    B: Blobstore + Send + 'static,
    B::Key: Sized + 'static,
    B::GetBlob: Send + 'static,
    B::PutBlob: Send + 'static,
    B::ValueIn: From<Vi>,
    Vi: Send + 'static,
    Vo: From<B::ValueOut> + AsRef<[u8]> + Send + 'static,
    E: error::Error + From<B::Error> + Send + 'static,
{
    let new = BlobstoreInner {
        blobstore,
        _phantom: PhantomData,
    };
    Box::new(new)
}

pub fn arced<B, Vi, Vo, E>(blobstore: B) -> ArcBlobstore<B::Key, Vi, Vo, E>
where
    B: Blobstore + Sync + Send + 'static,
    B::Key: Sized + 'static,
    B::GetBlob: Send + 'static,
    B::PutBlob: Send + 'static,
    B::ValueIn: From<Vi>,
    Vi: Send + 'static,
    Vo: From<B::ValueOut> + AsRef<[u8]> + Send + 'static,
    E: error::Error + From<B::Error> + Send + 'static,
{
    let new = BlobstoreInner {
        blobstore,
        _phantom: PhantomData,
    };
    Arc::new(new)
}

struct BlobstoreInner<B, Vi, Vo, E> {
    blobstore: B,
    _phantom: PhantomData<(Vi, Vo, E)>,
}

// Set Sync marker iff B is Sync - the rest don't matter in a PhantomData.
unsafe impl<B, Vi, Vo, E> Sync for BlobstoreInner<B, Vi, Vo, E>
where
    B: Sync,
{
}

impl<B, Vi, Vo, E> Blobstore for BlobstoreInner<B, Vi, Vo, E>
where
    B: Blobstore + Send + 'static,
    B::Key: Sized + 'static,
    B::GetBlob: Send + 'static,
    B::PutBlob: Send + 'static,
    B::ValueIn: From<Vi>,
    Vi: Send + 'static,
    Vo: From<B::ValueOut> + AsRef<[u8]> + Send + 'static,
    E: error::Error + From<B::Error> + Send + 'static,
{
    type Error = E;
    type Key = B::Key;
    type ValueIn = Vi;
    type ValueOut = Vo;

    type GetBlob = BoxFuture<Option<Self::ValueOut>, Self::Error>;
    type PutBlob = BoxFuture<(), Self::Error>;

    fn get(&self, key: &Self::Key) -> Self::GetBlob {
        self.blobstore
            .get(key)
            .map(|v| v.map(Vo::from))
            .map_err(E::from)
            .boxify()
    }

    fn put(&self, key: Self::Key, value: Self::ValueIn) -> Self::PutBlob {
        let value = B::ValueIn::from(value);
        self.blobstore.put(key, value).map_err(E::from).boxify()
    }
}
