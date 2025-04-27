/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::CoreContext;
use futures::TryStreamExt;
use futures::future::try_join;
use manifest::ManifestOps;
use mononoke_types::ChangesetId;
use regex::Regex;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use slog::error;
use slog::info;
use unodes::RootUnodeManifestId;

use crate::Repo;

pub async fn validate(
    ctx: &CoreContext,
    repo: &Repo,
    head_commit: ChangesetId,
    to_merge_commit: ChangesetId,
    path_regex: Regex,
) -> Result<(), Error> {
    let head_root_unode = repo
        .repo_derived_data()
        .derive::<RootUnodeManifestId>(ctx, head_commit);
    let to_merge_commit_root_unode = repo
        .repo_derived_data()
        .derive::<RootUnodeManifestId>(ctx, to_merge_commit);

    let (head_root_unode, to_merge_commit_root_unode) =
        try_join(head_root_unode, to_merge_commit_root_unode).await?;

    let head_leaves = head_root_unode
        .manifest_unode_id()
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
        .try_collect::<Vec<_>>();
    let to_merge_commit_leaves = to_merge_commit_root_unode
        .manifest_unode_id()
        .list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
        .try_collect::<Vec<_>>();

    let (head_leaves, mut to_merge_commit_leaves) =
        try_join(head_leaves, to_merge_commit_leaves).await?;

    info!(
        ctx.logger(),
        "total unodes in head commit: {}",
        head_leaves.len()
    );
    info!(
        ctx.logger(),
        "total unodes in to merge commit: {}",
        to_merge_commit_leaves.len()
    );
    let mut head_leaves = head_leaves
        .into_iter()
        .filter(|(path, _)| path.matches_regex(&path_regex))
        .collect::<Vec<_>>();
    info!(
        ctx.logger(),
        "unodes in to head commit after filtering: {}",
        head_leaves.len()
    );

    head_leaves.sort();
    to_merge_commit_leaves.sort();

    if head_leaves == to_merge_commit_leaves {
        info!(ctx.logger(), "all is well");
    } else {
        error!(ctx.logger(), "validation failed!");
        for (path, unode) in head_leaves {
            println!("{}\t{}\t{}", head_commit, path, unode);
        }

        for (path, unode) in to_merge_commit_leaves {
            println!("{}\t{}\t{}", to_merge_commit, path, unode);
        }
    }
    Ok(())
}
