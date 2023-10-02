/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use async_trait::async_trait;
use blobstore::impl_loadable_storable;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use bytes::Bytes;
use context::CoreContext;
use filestore::hash_bytes;
use filestore::ExpectedSize;
use filestore::Sha1IncrementalHasher;
use futures::future;
use futures::stream;
use futures::stream::BoxStream;
use futures::StreamExt;
use futures::TryStreamExt;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BlobstoreKey;

use crate::delta::DeltaInstructionChunk;
use crate::delta::DeltaInstructionChunkId;
use crate::delta::DeltaInstructionChunkIdPrefix;
use crate::delta::DeltaInstructions;
use crate::errors::GitError;
use crate::thrift::Tree as ThriftTree;
use crate::thrift::TreeHandle as ThriftTreeHandle;
use crate::zlib_writer::AsyncZlibEncoder;
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

/// Free function for fetching the raw bytes of stored git objects
pub(crate) async fn fetch_git_object_bytes<B>(
    ctx: &CoreContext,
    blobstore: &B,
    git_hash: &gix_hash::oid,
) -> anyhow::Result<Bytes, GitError>
where
    B: Blobstore + Clone,
{
    let blobstore_key = format!("{}{}{}", GIT_OBJECT_PREFIX, SEPARATOR, git_hash.to_hex());
    let object_bytes = blobstore
        .get(ctx, &blobstore_key)
        .await
        .map_err(|e| GitError::StorageFailure(git_hash.to_hex().to_string(), e.into()))?
        .ok_or_else(|| GitError::NonExistentObject(git_hash.to_hex().to_string()))?;
    Ok(object_bytes.into_raw_bytes())
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
    let raw_bytes = fetch_git_object_bytes(ctx, blobstore, git_hash).await?;
    let object = gix_object::ObjectRef::from_loose(raw_bytes.as_ref()).map_err(|e| {
        GitError::InvalidContent(
            git_hash.to_hex().to_string(),
            anyhow::anyhow!(e.to_string()).into(),
        )
    })?;
    Ok(object.into())
}

/// Struct containing the information pertaining to stored chunks of raw instructions
pub struct StoredInstructionsMetadata {
    /// The total size of the raw delta instructions without Zlib encoding/compression
    pub uncompressed_bytes: u64,
    /// The compressed size of the raw delta instructions with Zlib encoding/compression
    pub compressed_bytes: u64,
    /// The total number of chunks used to store the raw delta instructions
    pub chunks: u64,
}

/// Store delta instructions in blobstore by chunking the incoming byte stream and returning the metadata of
/// the written delta instructions stored as chunks in the blobstore. This method can partially fail
/// and store a subset of the chunks. However, it is perfectly safe to retry until all the chunks are stored
/// successfully
#[allow(dead_code)]
pub async fn store_delta_instructions<B>(
    ctx: &CoreContext,
    blobstore: &B,
    instructions: DeltaInstructions,
    chunk_prefix: DeltaInstructionChunkIdPrefix,
    chunk_size: Option<u64>,
) -> anyhow::Result<StoredInstructionsMetadata>
where
    B: Blobstore + Clone,
{
    // Zlib encode the instructions when writing to buffer
    let mut raw_instruction_bytes = AsyncZlibEncoder::new(Vec::new());
    instructions
        .write(&mut raw_instruction_bytes)
        .await
        .context("Error in converting DeltaInstructions to raw bytes")?;
    let uncompressed_bytes = raw_instruction_bytes.bytes_written();
    let raw_instruction_bytes = raw_instruction_bytes.into_inner();
    let compressed_bytes = raw_instruction_bytes.len() as u64;
    let size = ExpectedSize::new(compressed_bytes);
    let raw_instructions_stream = stream::once(future::ok(Bytes::from(raw_instruction_bytes)));
    let chunk_stream = filestore::make_chunks(raw_instructions_stream, size, chunk_size);
    let chunks = match chunk_stream {
        filestore::Chunks::Inline(fallible_bytes) => {
            let instruction_bytes = fallible_bytes
                .await
                .context("Error in getting inlined bytes from chunk stream")?;
            store_delta_instruction_chunk(ctx, blobstore, chunk_prefix.as_id(0), instruction_bytes)
                .await
                .context("Failure in storing inlined instruction chunk to blobstore")?;
            Ok(1)
        }
        filestore::Chunks::Chunked(_, bytes_stream) => bytes_stream
            .enumerate()
            .map(|(idx, fallible_bytes)| {
                let chunk_prefix = &chunk_prefix;
                async move {
                    let instruction_bytes = fallible_bytes.with_context(|| {
                        format!(
                            "Error in getting bytes from chunk {} in chunked stream",
                            idx
                        )
                    })?;
                    store_delta_instruction_chunk(
                        ctx,
                        blobstore,
                        chunk_prefix.as_id(idx),
                        instruction_bytes,
                    )
                    .await
                    .with_context(|| {
                        format!("Failure in storing instruction chunk {} to blobstore", idx)
                    })?;
                    anyhow::Ok(())
                }
            })
            .buffer_unordered(24) // Same as the concurrency used for filestore
            .try_collect::<Vec<_>>()
            .await
            .map(|result| result.len() as u64),
    };
    chunks.map(|chunks| StoredInstructionsMetadata {
        uncompressed_bytes,
        compressed_bytes,
        chunks,
    })
}

/// Fetch all the delta instruction chunks corresponding to the given prefix and return the result
/// as a boxed stream of bytes in order
#[allow(dead_code)]
pub fn fetch_delta_instructions<'a, B>(
    ctx: &'a CoreContext,
    blobstore: &'a B,
    chunk_prefix: &'a DeltaInstructionChunkIdPrefix,
    chunk_count: u64,
) -> BoxStream<'a, anyhow::Result<Bytes>>
where
    B: Blobstore + Clone,
{
    stream::iter(0..chunk_count)
        .map(move |chunk_idx| async move {
            let chunk_id = chunk_prefix.as_id(chunk_idx as usize);
            let chunk = chunk_id.load(ctx, blobstore).await.with_context(|| {
                format!("Error while fetching instructions chunk #{}", chunk_idx)
            })?;
            anyhow::Ok(chunk.into_bytes())
        })
        .buffered(24) // Same as the concurrency used for filestore
        .boxed()
}

async fn store_delta_instruction_chunk<B>(
    ctx: &CoreContext,
    blobstore: &B,
    id: DeltaInstructionChunkId,
    instruction_bytes: Bytes,
) -> anyhow::Result<()>
where
    B: Blobstore + Clone,
{
    let blobstore_key = id.blobstore_key();
    blobstore
        .put(
            ctx,
            blobstore_key,
            DeltaInstructionChunk::new_bytes(instruction_bytes).into_blobstore_bytes(),
        )
        .await
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
