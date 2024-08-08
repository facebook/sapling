/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use blobstore::Blobstore;
use context::CoreContext;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use gix_hash::ObjectId;
use mononoke_types::hash;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::hash::Sha256;
use mononoke_types::BasicFileChange;

/// In line with https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md
fn format_lfs_pointer(sha256: Sha256, size: u64) -> String {
    format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{sha256}\nsize {size}\n",
        sha256 = sha256,
        size = size
    )
}

/// Given a file change generates a Git LFS pointer that points to acctual file contents
/// and stores it in the blobstore. Returns oid of the LFS pointer.
pub async fn generate_and_store_git_lfs_pointer<B: Blobstore + Clone + 'static>(
    blobstore: &B,
    filestore_config: FilestoreConfig,
    ctx: &CoreContext,
    basic_file_change: &BasicFileChange,
) -> Result<RichGitSha1> {
    let metadata = filestore::get_metadata(
        blobstore,
        ctx,
        &FetchKey::Canonical(basic_file_change.content_id()),
    )
    .await?
    .ok_or_else(|| anyhow!("Missing metadata for {}", basic_file_change.content_id()))?;
    let lfs_pointer = format_lfs_pointer(metadata.sha256, basic_file_change.size());
    let ((content_id, _size), fut) =
        filestore::store_bytes(blobstore, filestore_config, ctx, lfs_pointer.into());
    fut.await?;
    let oid = filestore::get_metadata(blobstore, ctx, &FetchKey::Canonical(content_id))
        .await?
        .ok_or_else(|| anyhow!("Missing metadata for {}", basic_file_change.content_id()))?
        .git_sha1;
    Ok(oid)
}

#[derive(Debug)]
pub struct LfsPointerData {
    pub version: String,
    pub sha256: hash::Sha256,
    pub size: u64,
    /// gitblob and gitid, where this metadata comes from. This is useful if we
    /// end up storing the metadata instead of the content (if the content cannot
    /// be found on the LFS server for example).
    pub gitblob: Vec<u8>,
    pub gitid: ObjectId,
}

/// We will not try to parse any file bigger then this.
/// Any valid gitlfs metadata file should be smaller then this.
const MAX_METADATA_LENGTH: usize = 511;

/// Layout of the metafiles:
/// | version https://git-lfs.github.com/spec/v1
/// | oid sha256:73e2200459562bb068f08e33210ed106014b877f878932b2147991e17a7c089b
/// | size 8423391
pub fn parse_lfs_pointer(gitblob: &[u8], gitid: ObjectId) -> Option<LfsPointerData> {
    if gitblob.len() > MAX_METADATA_LENGTH {
        return None;
    }

    let mut lines = std::str::from_utf8(gitblob).ok()?.lines();
    let version = lines.next()?.strip_prefix("version ")?;
    if version != "https://git-lfs.github.com/spec/v1" {
        return None;
    }
    let sha256 = lines
        .next()?
        .strip_prefix("oid sha256:")?
        .parse::<hash::Sha256>()
        .ok()?;
    let size = lines.next()?.strip_prefix("size ")?.parse::<u64>().ok()?;
    // As a precaution. If we have an additional line after this, then we assume its not a valid file.
    if lines.next().is_some() {
        return None;
    }
    Some(LfsPointerData {
        version: version.to_string(),
        sha256,
        size,
        gitblob: gitblob.to_vec(),
        gitid,
    })
}
