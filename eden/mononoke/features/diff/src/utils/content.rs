/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Context;
use blobstore::Loadable;
use bytes::Bytes;
use context::CoreContext;
use filestore::FetchKey;
use fsnodes::RootFsnodeId;
use git_types::git_lfs::format_lfs_pointer;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::FileChange::Change;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::hash::GitSha1;
use mononoke_types::path::MPath;
use unodes::RootUnodeManifestId;

use crate::error::DiffError;
use crate::types::DiffFileType;
use crate::types::DiffSingleInput;
use crate::types::LfsPointer;
use crate::types::Repo;

pub async fn load_content(
    ctx: &CoreContext,
    repo: &impl Repo,
    input: &DiffSingleInput,
) -> Result<Option<Bytes>, DiffError> {
    let content_id = match input {
        DiffSingleInput::Content(content_input) => Some(content_input.content_id),
        DiffSingleInput::ChangesetPath(changeset_input) => {
            get_content_id_from_changeset_path(
                ctx,
                repo,
                changeset_input.changeset_id,
                changeset_input.path.clone(),
            )
            .await?
        }
        DiffSingleInput::String(string_input) => {
            // For string inputs, convert the string directly to Bytes and return early
            return Ok(Some(Bytes::copy_from_slice(
                string_input.content.as_bytes(),
            )));
        }
    };

    if let Some(content_id) = content_id {
        let blobstore = repo.repo_blobstore();
        let fetch_key = FetchKey::Canonical(content_id);

        // We need to store the full file in memory, so there is no reason
        // to use the streaming version.
        // Use fetch_concat_opt which returns Option<Bytes> to properly handle missing content
        // TODO: Add size limit to avoid overloading the service
        match filestore::fetch_concat_opt(blobstore, ctx, &fetch_key).await {
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

async fn get_content_id_from_changeset_path(
    ctx: &CoreContext,
    repo: &impl Repo,
    changeset_id: ChangesetId,
    path: NonRootMPath,
) -> Result<Option<ContentId>, DiffError> {
    // If the file was changed in this changeset we already have the content_id
    let changeset = changeset_id
        .load(ctx, repo.repo_blobstore())
        .await
        .map_err(DiffError::internal)?;
    let change = &changeset.file_changes_map().get(&path);
    if let Some(Change(tracked)) = change {
        Ok(Some(tracked.content_id().clone()))
    } else {
        // If the file was not changed here we will have to retrieve it and trigger derivation
        let content_info = get_file_info_from_changeset_path(ctx, repo, changeset_id, path).await?;
        Ok(content_info.map(|(content_id, _)| content_id))
    }
}

pub async fn get_file_info_from_changeset_path(
    ctx: &CoreContext,
    repo: &impl Repo,
    changeset_id: ChangesetId,
    path: NonRootMPath,
) -> Result<Option<(ContentId, FileType)>, DiffError> {
    let root_fsnode_id = repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, changeset_id)
        .await
        .map_err(DiffError::internal)?;

    let blobstore = repo.repo_blobstore();
    let mpath = MPath::from(path);

    match root_fsnode_id
        .fsnode_id()
        .find_entry(ctx.clone(), blobstore.clone(), mpath)
        .await
        .map_err(DiffError::internal)?
    {
        Some(Entry::Leaf(fsnode_file)) => {
            Ok(Some((*fsnode_file.content_id(), *fsnode_file.file_type())))
        }
        Some(Entry::Tree(_)) => Ok(None), // Path exists but is a directory, not a file
        None => Ok(None),                 // Path does not exist
    }
}

/// Get file change information from changeset for LFS metadata
async fn get_file_change_from_changeset_path(
    ctx: &CoreContext,
    repo: &impl Repo,
    changeset_id: ChangesetId,
    path: NonRootMPath,
) -> Result<Option<mononoke_types::FileChange>, DiffError> {
    // Load the changeset to get FileChange with LFS metadata
    let changeset = changeset_id
        .load(ctx, repo.repo_blobstore())
        .await
        .map_err(DiffError::internal)?;

    // First, try to find the file change in the current changeset
    if let Some((_, file_change)) = changeset.file_changes().find(|(p, _)| p == &&path) {
        return Ok(Some(file_change.clone()));
    }

    // If not found in current changeset, look back through history
    let root_unode_manifest_id = repo
        .repo_derived_data()
        .derive::<RootUnodeManifestId>(ctx, changeset_id)
        .await
        .map_err(DiffError::internal)?;

    let blobstore = repo.repo_blobstore();
    let mpath = MPath::from(path.clone());

    if let Some(Entry::Leaf(file_unode_id)) = root_unode_manifest_id
        .manifest_unode_id()
        .find_entry(ctx.clone(), blobstore.clone(), mpath)
        .await
        .map_err(DiffError::internal)?
    {
        let file_unode = file_unode_id
            .load(ctx, blobstore)
            .await
            .map_err(DiffError::internal)?;

        let last_modified_cs_id = file_unode.linknode().clone();

        // Load the last modified changeset and check for file changes
        let last_modified_changeset = last_modified_cs_id
            .load(ctx, blobstore)
            .await
            .map_err(DiffError::internal)?;

        if let Some((_, file_change)) = last_modified_changeset
            .file_changes()
            .find(|(p, _)| p == &&path)
        {
            return Ok(Some(file_change.clone()));
        }
    }

    Ok(None)
}

/// Extract content ID, changeset ID, default path, and LFS pointer from a DiffSingleInput
async fn extract_input_data(
    ctx: &CoreContext,
    repo: &impl Repo,
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
                ctx,
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

            // Try to detect LFS pointer if content exists
            let lfs_pointer = if let Some(content_id) = content_id {
                get_lfs_pointer(
                    ctx,
                    repo,
                    changeset_input.changeset_id,
                    path.clone(),
                    &content_id,
                )
                .await?
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
        DiffSingleInput::String(_string_input) => {
            // For string inputs, there's no content ID, changeset ID, or LFS pointer
            // Use the default path provided
            Ok((None, None, default_path, None))
        }
    }
}

pub async fn get_lfs_pointer(
    ctx: &CoreContext,
    repo: &impl Repo,
    changeset_id: ChangesetId,
    path: NonRootMPath,
    content_id: &ContentId,
) -> Result<Option<LfsPointer>, DiffError> {
    // Check if LFS pointer interpretation is enabled
    if !repo.repo_config().git_configs.git_lfs_interpret_pointers {
        return Ok(None);
    }

    // Get the file change to check LFS metadata
    if let Some(Change(tracked_change)) =
        get_file_change_from_changeset_path(ctx, repo, changeset_id, path).await?
    {
        if tracked_change.git_lfs().is_lfs_pointer() {
            let metadata = get_content_metadata(ctx, repo, content_id).await?;
            return Ok(Some(LfsPointer {
                sha256: metadata.sha256.to_string(),
                size: tracked_change.size() as i64,
            }));
        }
    }

    Ok(None)
}

pub async fn get_content_metadata(
    ctx: &CoreContext,
    repo: &impl Repo,
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

pub async fn load_diff_file(
    ctx: &CoreContext,
    repo: &impl Repo,
    input: &DiffSingleInput,
    default_path: NonRootMPath,
    options: &DiffFileOpts,
) -> Result<Option<xdiff::DiffFile<String, Vec<u8>>>, DiffError> {
    // Handle String input specially since it doesn't have a content_id
    if let DiffSingleInput::String(string_input) = input {
        // Validate string input size
        let bytes = Bytes::copy_from_slice(string_input.content.as_bytes());
        return Ok(Some(xdiff::DiffFile {
            path: default_path.to_string(),
            contents: xdiff::FileContent::Inline(bytes.to_vec()),
            file_type: options.file_type.into(),
        }));
    }

    let (content_id, _changeset_id, path, lfs_pointer) =
        extract_input_data(ctx, repo, input, default_path).await?;

    if let Some(id) = content_id {
        let contents = if options.file_type == DiffFileType::GitSubmodule {
            // Handle Git submodule: load commit hash regardless of omit_content
            let commit_hash_bytes = load_content(ctx, repo, input).await?.ok_or_else(|| {
                DiffError::Internal(anyhow::anyhow!(
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
                DiffError::Internal(anyhow::anyhow!(
                    "Failed to load content for content_id: {:?}",
                    id
                ))
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
