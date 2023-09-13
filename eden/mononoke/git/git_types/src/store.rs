/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use blobstore::impl_loadable_storable;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use filestore::hash_bytes;
use filestore::Sha1IncrementalHasher;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BlobstoreKey;

use crate::delta::DeltaInstructionChunk;
use crate::delta::DeltaInstructionChunkId;
use crate::errors::GitError;
use crate::thrift::Tree as ThriftTree;
use crate::thrift::TreeHandle as ThriftTreeHandle;
use crate::Tree;
use crate::TreeHandle;

impl_loadable_storable! {
    handle_type => TreeHandle,
    handle_thrift_type => ThriftTreeHandle,
    value_type => Tree,
    value_thrift_type => ThriftTree,
}

const GIT_OBJECT_PREFIX: &str = "git_object";
const SEPARATOR: &str = ".";

/// Free function for uploading serialized git objects
pub async fn upload_git_object<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
    raw_content: Vec<u8>,
) -> anyhow::Result<(), GitError>
where
    B: Blobstore + Clone,
{
    // Check if the provided Sha1 hash (i.e. ObjectId) of the bytes actually corresponds to the hash of the bytes
    let bytes = bytes::Bytes::from(raw_content);
    let sha1_hash = hash_bytes(Sha1IncrementalHasher::new(), &bytes);
    if sha1_hash.as_ref() != git_hash.as_bytes() {
        return Err(GitError::HashMismatch(
            git_hash.to_hex().to_string(),
            sha1_hash.to_hex().to_string(),
        ));
    };
    // Check if the bytes actually correspond to a valid Git object
    let blobstore_bytes = BlobstoreBytes::from_bytes(bytes.clone());
    let git_obj = gix_object::ObjectRef::from_loose(bytes.as_ref()).map_err(|e| {
        GitError::InvalidContent(
            git_hash.to_hex().to_string(),
            anyhow::anyhow!(e.to_string()).into(),
        )
    })?;
    // Check if the git object is not a raw content blob. Raw content blobs are uploaded directly through
    // LFS. This method supports git commits, trees, tags, notes and similar pointer objects.
    if let gix_object::ObjectRef::Blob(_) = git_obj {
        return Err(GitError::DisallowedBlobObject(
            git_hash.to_hex().to_string(),
        ));
    }
    // The bytes are valid, upload to blobstore with the key:
    // git_object_{hex-value-of-hash}
    let blobstore_key = format!("{}{}{}", GIT_OBJECT_PREFIX, SEPARATOR, git_hash.to_hex());
    blobstore
        .put(ctx, blobstore_key, blobstore_bytes)
        .await
        .map_err(|e| GitError::StorageFailure(git_hash.to_hex().to_string(), e.into()))
}

/// Free function for fetching stored git objects
pub async fn fetch_git_object<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
) -> anyhow::Result<gix_object::Object, GitError>
where
    B: Blobstore + Clone,
{
    let blobstore_key = format!("{}{}{}", GIT_OBJECT_PREFIX, SEPARATOR, git_hash.to_hex());
    let object_bytes = blobstore
        .get(ctx, &blobstore_key)
        .await
        .map_err(|e| GitError::StorageFailure(git_hash.to_hex().to_string(), e.into()))?
        .ok_or_else(|| GitError::NonExistentObject(git_hash.to_hex().to_string()))?;
    let object =
        gix_object::ObjectRef::from_loose(object_bytes.as_raw_bytes().as_ref()).map_err(|e| {
            GitError::InvalidContent(
                git_hash.to_hex().to_string(),
                anyhow::anyhow!(e.to_string()).into(),
            )
        })?;
    Ok(object.into())
}

#[async_trait]
impl Loadable for DeltaInstructionChunkId {
    type Value = DeltaInstructionChunk;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let id = *self;
        let blobstore_key = id.blobstore_key();
        let get = blobstore.get(ctx, &blobstore_key);

        let bytes = get.await?.ok_or(LoadableError::Missing(blobstore_key))?;
        DeltaInstructionChunk::from_encoded_bytes(bytes.into_raw_bytes())
            .map_err(LoadableError::Error)
    }
}
