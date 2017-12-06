// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use bytes::Bytes;
use failure::Error;

use futures_ext::{BoxFuture, FutureExt};

use super::*;

pub type BoxBlobstore = Box<
    Blobstore<GetBlob = BoxFuture<Option<Bytes>, Error>, PutBlob = BoxFuture<(), Error>>,
>;

pub type ArcBlobstore = Arc<
    Blobstore<GetBlob = BoxFuture<Option<Bytes>, Error>, PutBlob = BoxFuture<(), Error>> + Sync,
>;

/// Take a concrete `BlobStore` implementation and box it up in a generic way.
/// In addition to boxing the blobstore type into trait object, it also conforms
/// the `Value*` and `Error` associated types to some type. This allows the caller
/// to conform multiple different blobstore implementations into a single boxed trait
/// object type with uniform errors and value types.
pub fn boxed<B>(blobstore: B) -> BoxBlobstore
where
    B: Blobstore + Send + 'static,
    B::GetBlob: Send + 'static,
    B::PutBlob: Send + 'static,
{
    let new = BlobstoreInner { blobstore };
    Box::new(new)
}

pub fn arced<B>(blobstore: B) -> ArcBlobstore
where
    B: Blobstore + Sync + Send + 'static,
    B::GetBlob: Send + 'static,
    B::PutBlob: Send + 'static,
{
    let new = BlobstoreInner { blobstore };
    Arc::new(new)
}

struct BlobstoreInner<B> {
    blobstore: B,
}

impl<B> Blobstore for BlobstoreInner<B>
where
    B: Blobstore + Send + 'static,
    B::GetBlob: Send + 'static,
    B::PutBlob: Send + 'static,
{
    type GetBlob = BoxFuture<Option<Bytes>, Error>;
    type PutBlob = BoxFuture<(), Error>;

    fn get(&self, key: String) -> Self::GetBlob {
        self.blobstore.get(key).boxify()
    }

    fn put(&self, key: String, value: Bytes) -> Self::PutBlob {
        self.blobstore.put(key, value).boxify()
    }
}
