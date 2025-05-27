/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use anyhow::Error;
use anyhow::format_err;
use bonsai_git_mapping::BonsaiGitMappingRef;
use context::CoreContext;
use gix_hash::ObjectId;
use import_tools::GitRepoReader;
use import_tools::GitimportTarget;
use import_tools::oid_to_sha1;
use mononoke_types::typed_hash::ChangesetId;
use slog::debug;

mod uploader;

pub use uploader::DirectUploader;

/// Import starting at from (known to be in Mononoke) and ending with to
pub async fn range(
    from: ObjectId,
    to: ObjectId,
    ctx: &CoreContext,
    repo: &impl BonsaiGitMappingRef,
) -> Result<GitimportTarget, Error> {
    let from_csid = repo
        .bonsai_git_mapping()
        .get_bonsai_from_git_sha1(ctx, oid_to_sha1(&from)?)
        .await?
        .ok_or_else(|| {
            format_err!(
                "Cannot start import from root {}: commit does not exist in repo",
                from
            )
        })?;
    let known = [(from, from_csid)].into();
    GitimportTarget::new(to, known)
}

/// Import commit and all its history that's not yet been imported
/// Makes a pass over the repo on construction to find missing history
pub async fn missing_for_commit(
    commit: ObjectId,
    ctx: &CoreContext,
    hg_repo: &impl BonsaiGitMappingRef,
    git_command_path: &Path,
    repo_path: &Path,
) -> Result<GitimportTarget, Error> {
    let reader = GitRepoReader::new(git_command_path, repo_path).await?;
    let ta = Instant::now();

    // Starting from the specified commit. We need to get the boundaries of what already is imported into Mononoke.
    // We do this by doing a bfs search from the specified commit.
    let mut known = HashMap::<ObjectId, ChangesetId>::new();
    let mut visited = HashSet::new();
    let mut q = vec![commit];
    while let Some(id) = q.pop() {
        if visited.insert(id) {
            if let Some(changeset) = commit_in_mononoke(ctx, hg_repo, &id).await? {
                known.insert(id, changeset);
            } else {
                let object = reader.get_object(&id).await?;
                let commit = object
                    .parsed
                    .try_into_commit()
                    .map_err(|_| format_err!("oid {} is not a commit", id))?;
                q.extend(commit.parents);
            }
        }
    }

    let tb = Instant::now();
    debug!(
        ctx.logger(),
        "Time to find missing commits {:?}",
        tb.duration_since(ta)
    );

    GitimportTarget::new(commit, known)
}

async fn commit_in_mononoke(
    ctx: &CoreContext,
    hg_repo: &impl BonsaiGitMappingRef,
    commit_id: &gix_hash::oid,
) -> Result<Option<ChangesetId>, Error> {
    let changeset = hg_repo
        .bonsai_git_mapping()
        .get_bonsai_from_git_sha1(ctx, oid_to_sha1(commit_id)?)
        .await?;
    if let Some(existing_changeset) = changeset {
        debug!(
            ctx.logger(),
            "Commit found in Mononoke Oid:{} -> ChangesetId:{}",
            oid_to_sha1(commit_id)?.to_brief(),
            existing_changeset.to_brief()
        );
    }
    Ok(changeset)
}
