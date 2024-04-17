/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore::Blobstore;
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
use mononoke_types::FsnodeId;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;

use crate::git_submodules::expand::SubmodulePath;
use crate::types::Repo;

/// Get the git hash from a submodule file, which represents the commit from the
/// given submodule that the source repo depends on at that revision.
pub(crate) async fn get_git_hash_from_submodule_file<'a, R: Repo>(
    ctx: &'a CoreContext,
    repo: &'a R,
    submodule_file_content_id: ContentId,
) -> Result<GitSha1> {
    let blobstore = repo.repo_blobstore_arc();

    let bytes = filestore::fetch_concat_exact(&blobstore, ctx, submodule_file_content_id, 20)
        .await
        .with_context(
            || "Failed to fetch content of file containing the submodule's git commit hash",
        )?;

    let git_submodule_hash = RichGitSha1::from_bytes(&bytes, ObjectKind::Commit.as_str(), 0)?;
    let git_submodule_sha1 = git_submodule_hash.sha1();

    anyhow::Ok(git_submodule_sha1)
}

/// Get the git hash from a submodule file, which represents the commit from the
/// given submodule that the source repo depends on at that revision.
pub(crate) async fn git_hash_from_submodule_metadata_file<'a, B: Blobstore>(
    ctx: &'a CoreContext,
    blobstore: &'a B,
    submodule_file_content_id: ContentId,
) -> Result<GitSha1> {
    let bytes = filestore::fetch_concat_exact(&blobstore, ctx, submodule_file_content_id, 40)
      .await
      .with_context(|| {
          format!(
              "Failed to fetch content from content id {} file containing the submodule's git commit hash",
              &submodule_file_content_id
          )
      })?;

    let git_hash_string = std::str::from_utf8(bytes.as_ref())?;
    let git_sha1 = GitSha1::from_str(git_hash_string)?;

    anyhow::Ok(git_sha1)
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

pub(crate) fn x_repo_submodule_metadata_file_basename<S: std::fmt::Display>(
    submodule_basename: &S,
    x_repo_submodule_metadata_file_prefix: &str,
) -> Result<MPathElement> {
    MPathElement::new(
        format!(".{x_repo_submodule_metadata_file_prefix}-{submodule_basename}")
            .to_string()
            .into_bytes(),
    )
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

    let x_repo_sm_metadata_file = x_repo_submodule_metadata_file_basename(
        &sm_basename,
        x_repo_submodule_metadata_file_prefix,
    )?;

    let x_repo_sm_metadata_path = match mb_sm_parent_dir {
        Some(sm_parent_dir) => sm_parent_dir.join(&x_repo_sm_metadata_file),
        None => x_repo_sm_metadata_file.into(),
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

pub(crate) async fn list_non_submodule_files_under<R>(
    ctx: &CoreContext,
    repo: &R,
    cs_id: ChangesetId,
    submodule_path: SubmodulePath,
) -> Result<impl Stream<Item = Result<NonRootMPath>>>
where
    R: RepoDerivedDataRef + RepoBlobstoreArc,
{
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

/// Gets the root directory's fsnode id from a submodule commit provided as
/// as a git hash. This is used for working copy validation of submodule
/// expansion.
pub(crate) async fn root_fsnode_id_from_submodule_git_commit(
    ctx: &CoreContext,
    repo: &impl Repo,
    git_hash: GitSha1,
) -> Result<FsnodeId> {
    let cs_id: ChangesetId = repo
        .bonsai_git_mapping()
        .get_bonsai_from_git_sha1(ctx, git_hash)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "Failed to get changeset id from git submodule commit hash {} in repo {}",
                &git_hash,
                &repo.repo_identity().name()
            )
        })?;
    let submodule_root_fsnode_id: RootFsnodeId = repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await?;

    Ok(submodule_root_fsnode_id.into_fsnode_id())
}
