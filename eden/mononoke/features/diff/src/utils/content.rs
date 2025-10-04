/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Context;
use anyhow::Error;
use bytes::Bytes;
use context::CoreContext;
use filestore::FetchKey;
use git_types::git_lfs::format_lfs_pointer;
use mononoke_api::ChangesetPathContentContext;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::NonRootMPath;
use mononoke_types::hash::GitSha1;

use crate::error::DiffError;
use crate::types::DiffFileType;
use crate::types::DiffSingleInput;
use crate::types::LfsPointer;

pub async fn load_content<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    input: &DiffSingleInput,
) -> Result<Option<Bytes>, DiffError> {
    let content_id = match input {
        DiffSingleInput::Content(content_input) => Some(content_input.content_id),
        DiffSingleInput::ChangesetPath(changeset_input) => {
            get_content_id_from_changeset_path(
                repo,
                changeset_input.changeset_id,
                changeset_input.path.clone(),
            )
            .await?
        }
    };

    if let Some(content_id) = content_id {
        let blobstore = repo.repo_blobstore();
        let fetch_key = FetchKey::Canonical(content_id);

        // We need to store the full file in memory, so there is no reason
        // to use the streaming version.
        // Use fetch_concat_opt which returns Option<Bytes> to properly handle missing content
        // TODO: Add size limit to avoid overloading the service
        match filestore::fetch_concat_opt(&blobstore, ctx, &fetch_key).await {
            Ok(Some(bytes)) => Ok(Some(bytes)),
            Ok(None) => {
                // Content not found - this is a client error
                Err(DiffError::content_not_found(content_id))
            }
            Err(e) => {
                // Other errors (blobstore issues, etc.) are internal errors
                Err(DiffError::internal(e.context("Failed to load content")))
            }
        }
    } else {
        Ok(None)
    }
}

async fn get_content_id_from_changeset_path<R: MononokeRepo>(
    repo: &RepoContext<R>,
    changeset_id: ChangesetId,
    path: NonRootMPath,
) -> Result<Option<ContentId>, DiffError> {
    let changeset_ctx = repo
        .changeset(changeset_id)
        .await
        .map_err(DiffError::internal)?
        .ok_or_else(|| DiffError::changeset_not_found(changeset_id))?;

    let path_content_ctx = changeset_ctx
        .path_with_content(path)
        .await
        .map_err(DiffError::internal)?;

    let file = path_content_ctx.file().await.map_err(DiffError::internal)?;

    if let Some(file) = file {
        let content_id = file.id().await.map_err(DiffError::internal)?;
        Ok(Some(content_id))
    } else {
        // The file is not present, so it may be new or deleted
        Ok(None)
    }
}

/// Extract content ID, changeset ID, default path, and LFS pointer from a DiffSingleInput
async fn extract_input_data<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    input: &DiffSingleInput,
    default_path: NonRootMPath,
) -> Result<
    (
        Option<ContentId>,
        Option<ChangesetId>,
        NonRootMPath,
        Option<LfsPointer>,
    ),
    DiffError,
> {
    match input {
        DiffSingleInput::ChangesetPath(changeset_input) => {
            let content_id = get_content_id_from_changeset_path(
                repo,
                changeset_input.changeset_id,
                changeset_input.path.clone(),
            )
            .await?;

            // It's mandatory to provide a path with Changeset inputs, so we don't consider the
            // default path.
            let path = changeset_input
                .replacement_path
                .as_ref()
                .unwrap_or(&changeset_input.path)
                .clone();

            let changeset_ctx = repo
                .changeset(changeset_input.changeset_id)
                .await
                .map_err(DiffError::internal)?
                .ok_or_else(|| DiffError::changeset_not_found(changeset_input.changeset_id))?;

            let path_content_ctx = changeset_ctx
                .path_with_content(changeset_input.path.clone())
                .await
                .map_err(DiffError::internal)?;

            let lfs_pointer = if let Some(content_id) = &content_id {
                get_lfs_pointer(ctx, repo, content_id, path.clone(), path_content_ctx).await?
            } else {
                None
            };

            Ok((
                content_id,
                Some(changeset_input.changeset_id),
                path,
                lfs_pointer,
            ))
        }
        DiffSingleInput::Content(content_input) => {
            let path = match &content_input.path {
                None => default_path,
                Some(path) => path.clone(),
            };
            Ok((
                Some(content_input.content_id),
                None,
                path,
                content_input.lfs_pointer.clone(),
            ))
        }
    }
}

pub async fn get_lfs_pointer<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    content_id: &ContentId,
    path: NonRootMPath,
    path_content_ctx: ChangesetPathContentContext<R>,
) -> Result<Option<LfsPointer>, DiffError> {
    // Check if LFS pointer interpretation is enabled
    if !repo.config().git_configs.git_lfs_interpret_pointers {
        return Ok(None);
    }

    let file_change = match path_content_ctx
        .file_change()
        .await
        .map_err(DiffError::internal)?
    {
        Some(file_change) => Some(file_change),
        None => {
            // If the file is not touched in the current changeset,
            // try checking the last changeset that touched the file
            let last_modified_cs = path_content_ctx
                .changeset()
                .path_with_history(path.clone())
                .await
                .map_err(DiffError::internal)?
                .last_modified()
                .await
                .map_err(DiffError::internal)?;

            match last_modified_cs {
                Some(last_modified_cs) => {
                    let file_changes = last_modified_cs
                        .file_changes()
                        .await
                        .map_err(DiffError::internal)?;
                    file_changes.get(&path).cloned()
                }
                None => None,
            }
        }
    };

    if let Some(fc) = file_change {
        if let Some(git_lfs) = fc.git_lfs() {
            if git_lfs.is_lfs_pointer() {
                let metadata = get_content_metadata(ctx, repo, content_id).await?;
                let size = fc.size().unwrap_or_default();
                return Ok(Some(LfsPointer {
                    sha256: metadata.sha256.to_string(),
                    size: size as i64,
                }));
            }
        }
    }

    Ok(None)
}

pub async fn get_content_metadata<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    content_id: &ContentId,
) -> Result<ContentMetadataV2, DiffError> {
    let blobstore = repo.repo_blobstore();
    let metadata = filestore::get_metadata(&blobstore, ctx, &FetchKey::Canonical(*content_id))
        .await
        .map_err(DiffError::internal)?
        .ok_or_else(|| DiffError::content_not_found(*content_id))?;
    Ok(metadata)
}

pub struct DiffFileOpts {
    pub file_type: DiffFileType,
    pub inspect_lfs_pointers: bool,
    pub omit_content: bool,
}

pub async fn load_diff_file<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    input: &DiffSingleInput,
    default_path: NonRootMPath,
    options: &DiffFileOpts,
) -> Result<Option<xdiff::DiffFile<String, Vec<u8>>>, Error> {
    let (content_id, _changeset_id, path, lfs_pointer) =
        extract_input_data(ctx, repo, input, default_path).await?;

    if let Some(id) = content_id {
        let contents = if options.file_type == DiffFileType::GitSubmodule {
            // Handle Git submodule: load commit hash regardless of omit_content
            let commit_hash_bytes = load_content(ctx, repo, input).await?.ok_or_else(|| {
                Error::msg(format!(
                    "Failed to load submodule content for content_id: {:?}",
                    id
                ))
            })?;

            let commit_hash = GitSha1::from_bytes(commit_hash_bytes)
                .with_context(|| format!("Invalid commit hash for submodule at {}", path))?
                .to_string();
            xdiff::FileContent::Submodule { commit_hash }
        } else if options.omit_content || (!options.inspect_lfs_pointers && lfs_pointer.is_some()) {
            // Omit content if selected, or if there is an LFS pointer that should not be
            // inspected.
            xdiff::FileContent::Omitted {
                content_hash: format!("{:?}", id),
                git_lfs_pointer: lfs_pointer.and_then(|lfs| {
                    // Parse string sha256 to Sha256 type and convert i64 to u64
                    let sha256 = mononoke_types::hash::Sha256::from_str(&lfs.sha256).ok()?;
                    let size = lfs.size as u64;
                    Some(format_lfs_pointer(sha256, size))
                }),
            }
        } else {
            // Otherwise load the full content
            let bytes = load_content(ctx, repo, input).await?.ok_or_else(|| {
                Error::msg(format!("Failed to load content for content_id: {:?}", id))
            })?;
            xdiff::FileContent::Inline(bytes.to_vec())
        };

        Ok(Some(xdiff::DiffFile {
            path: path.to_string(),
            contents,
            file_type: options.file_type.into(),
        }))
    } else {
        // If there was no contentId that's not necessarily an error, the file may be new, or
        // deleted
        Ok(None)
    }
}
