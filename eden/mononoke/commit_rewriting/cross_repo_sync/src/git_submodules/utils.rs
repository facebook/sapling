/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use context::CoreContext;
use fsnodes::RootFsnodeId;
use futures::future;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use git_types::ObjectKind;
use manifest::bonsai_diff;
use manifest::BonsaiDiffFileChange;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileType;
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

/// Returns true if the given path is a git submodule.
pub(crate) async fn is_path_git_submodule(
    ctx: &CoreContext,
    repo: &impl Repo,
    changeset: ChangesetId,
    path: &NonRootMPath,
) -> Result<bool, Error> {
    Ok(get_submodule_file_content_id(ctx, repo, changeset, path)
        .await?
        .is_some())
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

// Returns the differences between a submodule commit and its parents.
pub(crate) async fn submodule_diff(
    ctx: &CoreContext,
    sm_repo: &impl Repo,
    cs_id: ChangesetId,
    parents: Vec<ChangesetId>,
) -> Result<impl Stream<Item = Result<BonsaiDiffFileChange<(ContentId, u64)>>>> {
    let fsnode_id = sm_repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await
        .with_context(|| format!("Failed to get fsnode id form changeset id {}", cs_id))?
        .into_fsnode_id();

    let parent_fsnode_ids = stream::iter(parents)
        .then(|parent_cs_id| async move {
            anyhow::Ok(
                sm_repo
                    .repo_derived_data()
                    .derive::<RootFsnodeId>(ctx, parent_cs_id)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to get parent's fsnode id from its changeset id: {}",
                            parent_cs_id
                        )
                    })?
                    .into_fsnode_id(),
            )
        })
        .try_collect::<HashSet<_>>()
        .await?;

    Ok(bonsai_diff(
        ctx.clone(),
        sm_repo.repo_blobstore_arc().clone(),
        fsnode_id,
        parent_fsnode_ids,
    ))
}

/// Returns the content id of the given path if it is a submodule file.
pub(crate) async fn get_submodule_file_content_id(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
    path: &NonRootMPath,
) -> Result<Option<ContentId>> {
    let fsnode_id = repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await?
        .into_fsnode_id();

    let entry = fsnode_id
        .find_entry(ctx.clone(), repo.repo_blobstore_arc(), path.clone().into())
        .await?;

    match entry {
        Some(Entry::Leaf(file)) if *file.file_type() == FileType::GitSubmodule => {
            Ok(Some(file.content_id().clone()))
        }
        _ => Ok(None),
    }
}

pub(crate) async fn list_all_paths(
    ctx: &CoreContext,
    repo: &impl Repo,
    changeset: ChangesetId,
) -> Result<impl Stream<Item = Result<NonRootMPath>>> {
    let fsnode_id = repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, changeset)
        .await?
        .into_fsnode_id();
    Ok(fsnode_id
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore_arc())
        .map_ok(|(path, _)| path))
}

pub(crate) async fn list_non_submodule_files_under(
    ctx: &CoreContext,
    repo: &impl Repo,
    cs_id: ChangesetId,
    submodule_path: SubmodulePath,
) -> Result<impl Stream<Item = Result<NonRootMPath>>> {
    let fsnode_id = repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await?
        .into_fsnode_id();

    Ok(fsnode_id
        .list_leaf_entries_under(
            ctx.clone(),
            repo.repo_blobstore_arc(),
            vec![submodule_path.0],
        )
        .try_filter_map(|(path, fsnode_file)| {
            future::ready(Ok(
                (*fsnode_file.file_type() != FileType::GitSubmodule).then_some(path)
            ))
        }))
}
