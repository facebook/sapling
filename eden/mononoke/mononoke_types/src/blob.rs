/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Support for converting Mononoke data structures into in-memory blobs.

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::Storable;
use bytes::Bytes;
use context::CoreContext;

use crate::typed_hash::BlobstoreKey;
use crate::typed_hash::ChangesetId;
use crate::typed_hash::ContentChunkId;
use crate::typed_hash::ContentId;
use crate::typed_hash::ContentMetadataId;
use crate::typed_hash::ContentMetadataV2Id;
use crate::typed_hash::DeletedManifestV2Id;
use crate::typed_hash::FastlogBatchId;
use crate::typed_hash::FileUnodeId;
use crate::typed_hash::FsnodeId;
use crate::typed_hash::ManifestUnodeId;
use crate::typed_hash::RawBundle2Id;
use crate::typed_hash::RedactionKeyListId;
use crate::typed_hash::SkeletonManifestId;

/// A serialized blob in memory.
#[derive(Clone)]
pub struct Blob<Id: BlobstoreKey> {
    id: Id,
    data: Bytes,
}

impl<Id: BlobstoreKey> Blob<Id> {
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

#[async_trait]
impl<K: BlobstoreKey + Copy + Send + Sync + 'static> Storable for Blob<K> {
    type Key = K;

    async fn store<'a, B: Blobstore>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Key> {
        let id = *self.id();
        let bytes = self.into();
        blobstore.put(ctx, id.blobstore_key(), bytes).await?;
        Ok(id)
    }
}

pub type ChangesetBlob = Blob<ChangesetId>;
pub type ContentBlob = Blob<ContentId>;
pub type ContentChunkBlob = Blob<ContentChunkId>;
pub type RawBundle2Blob = Blob<RawBundle2Id>;
pub type FileUnodeBlob = Blob<FileUnodeId>;
pub type ManifestUnodeBlob = Blob<ManifestUnodeId>;
pub type DeletedManifestV2Blob = Blob<DeletedManifestV2Id>;
pub type FsnodeBlob = Blob<FsnodeId>;
pub type SkeletonManifestBlob = Blob<SkeletonManifestId>;
pub type ContentMetadataBlob = Blob<ContentMetadataId>;
pub type ContentMetadataV2Blob = Blob<ContentMetadataV2Id>;
pub type FastlogBatchBlob = Blob<FastlogBatchId>;
pub type RedactionKeyListBlob = Blob<RedactionKeyListId>;

impl<Id: BlobstoreKey> From<Blob<Id>> for BlobstoreBytes {
    #[inline]
    fn from(blob: Blob<Id>) -> BlobstoreBytes {
        BlobstoreBytes::from_bytes(blob.data)
    }
}

pub trait BlobstoreValue: Sized + Send {
    type Key: BlobstoreKey;
    fn into_blob(self) -> Blob<Self::Key>;
    fn from_blob(blob: Blob<Self::Key>) -> Result<Self>;
}
