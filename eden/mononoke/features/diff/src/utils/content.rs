/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bytes::Bytes;
use context::CoreContext;
use filestore::FetchKey;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::NonRootMPath;

use crate::types::DiffSingleInput;

pub async fn load_content<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    input: DiffSingleInput,
) -> Result<Option<Bytes>, Error> {
    let content_id = match input {
        DiffSingleInput::Content(content_input) => Some(content_input.content_id),
        DiffSingleInput::ChangesetPath(changeset_input) => {
            get_content_id_from_changeset_path(
                repo,
                changeset_input.changeset_id,
                changeset_input.path,
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
                Err(Error::msg(format!("Content not found: {}", content_id)))
            }
            Err(e) => {
                // Other errors (blobstore issues, etc.) are internal errors
                Err(e.context("Failed to load content"))
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
) -> Result<Option<ContentId>, Error> {
    let changeset_ctx = repo
        .changeset(changeset_id)
        .await?
        .ok_or_else(|| Error::msg(format!("changeset not found: {}", changeset_id)))?;

    let path_content_ctx = changeset_ctx.path_with_content(path).await?;

    let file = path_content_ctx.file().await?;

    if let Some(file) = file {
        let content_id = file.id().await?;
        Ok(Some(content_id))
    } else {
        // The file is not present, so it may be new or deleted
        Ok(None)
    }
}
