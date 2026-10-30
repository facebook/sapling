/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Imports a Perforce depot into a repo.

use std::collections::HashMap;
use std::pin::pin;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use bytes::Bytes;
use context::CoreContext;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::Stream;
use futures::TryStreamExt;
use futures::stream;
use mononoke_types::ContentMetadataV2;
use mononoke_types::FileType;
use p4_types::ChangeSource;
use p4_types::Changelist;
use repo_blobstore::RepoBlobstoreRef;

/// Stores a file revision's content in the repo's filestore. `size` is the size
/// Perforce reports for the revision: content of any other length is rejected.
pub async fn upload_content(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    size: u64,
    content: impl Stream<Item = Result<Bytes>> + Send,
) -> Result<ContentMetadataV2> {
    filestore::store(
        repo.repo_blobstore(),
        *repo.filestore_config(),
        ctx,
        &StoreRequest::new(size),
        content,
    )
    .await
}

// Only bounds memory: Perforce's default filesys.maxsymlink is 1K, OS limits are 4-8K.
const MAX_SYMLINK_SIZE: u64 = 64 * 1024;

/// Like `upload_content`, but stores a symlink as its bare target.
pub async fn upload_file_content(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    file_type: FileType,
    size: u64,
    content: impl Stream<Item = Result<Bytes>> + Send,
) -> Result<ContentMetadataV2> {
    if file_type != FileType::Symlink {
        return upload_content(ctx, repo, size, content).await;
    }
    ensure!(
        size <= MAX_SYMLINK_SIZE,
        "symlink of {size} bytes is larger than we support (64 KiB)"
    );
    let printed = read_symlink(size, content).await?;
    let target = symlink_target(size, printed)?;
    upload_content(
        ctx,
        repo,
        target.len() as u64,
        stream::once(async { Ok(target) }),
    )
    .await
}

async fn read_symlink(size: u64, content: impl Stream<Item = Result<Bytes>>) -> Result<Vec<u8>> {
    let mut content = pin!(content);
    let mut printed = Vec::new();
    while let Some(chunk) = content.try_next().await? {
        ensure!(
            (printed.len() + chunk.len()) as u64 <= size,
            "symlink content is longer than Perforce reports ({size} bytes)"
        );
        printed.extend_from_slice(&chunk);
    }
    Ok(printed)
}

// p4 print ends a symlink's target with a newline, and Perforce's size counts it.
fn symlink_target(size: u64, mut printed: Vec<u8>) -> Result<Bytes> {
    ensure!(
        printed.len() as u64 == size,
        "symlink content is {} bytes but Perforce reports {size}",
        printed.len()
    );
    printed
        .pop_if(|last| *last == b'\n')
        .context("symlink content does not end with a newline")?;
    ensure!(
        !printed.ends_with(b"\n"),
        "symlink content ends with more than one newline"
    );
    Ok(Bytes::from(printed))
}

/// Stores the content of every revision in `changelist` that leaves content in the depot
/// at `depot_root`, keyed by depot path as `p4_transform::changeset` takes it.
pub async fn upload_changelist_contents(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    source: &impl ChangeSource,
    depot_root: &str,
    changelist: &Changelist,
) -> Result<HashMap<String, ContentMetadataV2>> {
    let mut contents = HashMap::new();
    // One file at a time for now.
    for revision in &changelist.files {
        if !revision.action.has_content()
            || p4_transform::repo_path(depot_root, &revision.depot_path)?.is_none()
        {
            continue;
        }
        let name = || format!("{}#{}", revision.depot_path, revision.revision);
        let size = revision
            .size
            .with_context(|| format!("describe gave no size for {}", name()))?;
        let content = source.print(revision).await?;
        let file_type = p4_transform::file_type(&revision.file_type);
        let metadata = upload_file_content(ctx, repo, file_type, size, content)
            .await
            .with_context(|| format!("storing {}", name()))?;
        contents.insert(revision.depot_path.clone(), metadata);
    }
    Ok(contents)
}

#[cfg(test)]
mod tests;
