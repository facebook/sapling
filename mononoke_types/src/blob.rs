// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Support for converting Mononoke data structures into in-memory blobs.

use blobstore::{Blobstore, BlobstoreBytes};
use bytes::Bytes;
use context::CoreContext;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};

use crate::{
    errors::*,
    typed_hash::{
        ChangesetId, ContentChunkId, ContentId, ContentMetadataId, FileUnodeId, ManifestUnodeId,
        MononokeId, RawBundle2Id,
    },
};

/// A serialized blob in memory.
pub struct Blob<Id> {
    id: Id,
    data: Bytes,
}

impl<Id> Blob<Id> {
    pub fn new(id: Id, data: Bytes) -> Self {
        Self { id, data }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn data(&self) -> &Bytes {
        &self.data
    }
}

pub type ChangesetBlob = Blob<ChangesetId>;
pub type ContentBlob = Blob<ContentId>;
pub type ContentChunkBlob = Blob<ContentChunkId>;
pub type RawBundle2Blob = Blob<RawBundle2Id>;
pub type FileUnodeBlob = Blob<FileUnodeId>;
pub type ManifestUnodeBlob = Blob<ManifestUnodeId>;
pub type ContentMetadataBlob = Blob<ContentMetadataId>;

impl<Id> From<Blob<Id>> for BlobstoreBytes {
    #[inline]
    fn from(blob: Blob<Id>) -> BlobstoreBytes {
        BlobstoreBytes::from_bytes(blob.data)
    }
}

pub trait BlobstoreValue: Sized + Send {
    type Key;
    fn into_blob(self) -> Blob<Self::Key>;
    fn from_blob(blob: Blob<Self::Key>) -> Result<Self>;
}

pub trait Loadable: Sized + 'static {
    type Value;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, Error>;
}

pub trait Storable: Sized + 'static {
    type Key;

    fn store<B: Blobstore + Clone>(
        self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Key, Error>;
}

impl<T> Loadable for T
where
    T: MononokeId,
{
    type Value = Option<T::Value>;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, Error> {
        let id = *self;
        let blobstore_key = id.blobstore_key();

        blobstore
            .get(ctx, blobstore_key.clone())
            .and_then(move |bytes| {
                bytes
                    .map(move |bytes| {
                        let blob: Blob<T> = Blob::new(id, bytes.into_bytes());
                        <T::Value>::from_blob(blob)
                    })
                    .transpose()
            })
            .boxify()
    }
}

impl<T> Storable for Blob<T>
where
    T: MononokeId,
{
    type Key = T;

    fn store<B: Blobstore + Clone>(
        self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Key, Error> {
        let id = *self.id();
        blobstore
            .put(ctx, id.blobstore_key(), self.into())
            .map(move |_| id)
            .boxify()
    }
}
