/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use context::CoreContext;
use fsnodes::RootFsnodeId;
use git_types::ObjectKind;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FsnodeId;
use mononoke_types::NonRootMPath;

use crate::git_submodules::expand::SubmodulePath;
use crate::types::Repo;

/// Get the git hash from a submodule file, which represents the commit from the
/// given submodule that the source repo depends on at that revision.
pub(crate) async fn get_git_hash_from_submodule_file<'a, R: Repo>(
    ctx: &'a CoreContext,
    repo: &'a R,
    submodule_file_content_id: ContentId,
    submodule_path: &'a SubmodulePath,
) -> Result<GitSha1> {
    let blobstore = repo.repo_blobstore_arc();

    let bytes = filestore::fetch_concat_exact(&blobstore, ctx, submodule_file_content_id, 20)
      .await
      .with_context(|| {
          format!(
              "Failed to fetch content of submodule {} file containing the submodule's git commit hash",
              &submodule_path
          )
      })?;

    let git_submodule_hash = RichGitSha1::from_bytes(&bytes, ObjectKind::Commit.as_str(), 0)?;
    let git_submodule_sha1 = git_submodule_hash.sha1();

    anyhow::Ok(git_submodule_sha1)
}

pub(crate) fn get_submodule_repo<'a, 'b, R: Repo>(
    sm_path: &'a SubmodulePath,
    submodule_deps: &'b HashMap<NonRootMPath, R>,
) -> Result<&'b R> {
    submodule_deps
        .get(&sm_path.0)
        .ok_or_else(|| anyhow!("Mononoke repo from submodule {} not available", sm_path.0))
}

pub(crate) async fn id_to_fsnode_manifest_id(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
) -> Result<FsnodeId, Error> {
    let repo_derived_data = repo.repo_derived_data();

    let root_fsnode_id = repo_derived_data
        .derive::<RootFsnodeId>(ctx, bcs_id)
        .await?;

    Ok(root_fsnode_id.into_fsnode_id())
}

/// Builds the full path of the x-repo submodule metadata file for a given
/// submodule.
pub(crate) fn get_x_repo_submodule_metadata_file_path(
    submodule_file_path: &SubmodulePath,
    // Prefix used to generate the metadata file basename. Obtained from
    // the small repo sync config.
    x_repo_submodule_metadata_file_prefix: &str,
) -> Result<NonRootMPath> {
    let (mb_sm_parent_dir, sm_basename) = submodule_file_path.0.split_dirname();

    let x_repo_sm_metadata_file: NonRootMPath = NonRootMPath::new(
        format!(".{x_repo_submodule_metadata_file_prefix}-{sm_basename}")
            .to_string()
            .into_bytes(),
    )?;

    let x_repo_sm_metadata_path = match mb_sm_parent_dir {
        Some(sm_parent_dir) => sm_parent_dir.join(&x_repo_sm_metadata_file),
        None => x_repo_sm_metadata_file,
    };
    Ok(x_repo_sm_metadata_path)
}
