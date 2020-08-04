/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobrepo_hg::BlobRepoHg;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use context::CoreContext;
use futures::compat::Future01CompatExt;
use slog::info;

use mercurial_types::{HgChangesetId, MPath};
use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime, FileChange};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ChangesetArgs {
    pub author: String,
    pub message: String,
    pub datetime: DateTime,
    pub bookmark: Option<BookmarkName>,
    pub mark_public: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StackPosition(pub usize);

/// For creating stacks of changesets
pub trait ChangesetArgsFactory = Fn(StackPosition) -> ChangesetArgs;

pub async fn create_save_and_generate_hg_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
    changeset_args: ChangesetArgs,
) -> Result<HgChangesetId, Error> {
    let bcs_id = create_and_save_bonsai(ctx, repo, parents, file_changes, changeset_args).await?;
    generate_hg_changeset(ctx, repo, bcs_id).await
}

pub async fn create_and_save_bonsai(
    ctx: &CoreContext,
    repo: &BlobRepo,
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
    changeset_args: ChangesetArgs,
) -> Result<ChangesetId, Error> {
    let ChangesetArgs {
        author,
        message,
        datetime,
        bookmark: maybe_bookmark,
        mark_public,
    } = changeset_args;
    let bcs = create_bonsai_changeset_only(parents, file_changes, author, message, datetime)?;
    let bcs_id = save_and_maybe_mark_public(&ctx, &repo, bcs, mark_public).await?;

    if let Some(bookmark) = maybe_bookmark {
        create_bookmark(ctx, repo, bookmark, bcs_id).await?;
    }

    Ok(bcs_id)
}

async fn save_and_maybe_mark_public(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs: BonsaiChangeset,
    mark_public: bool,
) -> Result<ChangesetId, Error> {
    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .compat()
        .await?;

    if mark_public {
        repo.get_phases()
            .add_reachable_as_public(ctx.clone(), vec![bcs_id])
            .compat()
            .await?;
        info!(ctx.logger(), "Marked as public {:?}", bcs_id);
    }
    Ok(bcs_id)
}

async fn generate_hg_changeset(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
) -> Result<HgChangesetId, Error> {
    info!(ctx.logger(), "Generating an HG equivalent of {:?}", bcs_id);
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
        .compat()
        .await?;

    info!(
        ctx.logger(),
        "Hg equivalent of {:?} is: {:?}", bcs_id, hg_cs_id
    );
    Ok(hg_cs_id)
}

async fn create_bookmark(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmark: BookmarkName,
    bcs_id: ChangesetId,
) -> Result<(), Error> {
    info!(
        ctx.logger(),
        "Setting bookmark {:?} to point to {:?}", bookmark, bcs_id
    );
    let mut transaction = repo.update_bookmark_transaction(ctx.clone());
    transaction.force_set(&bookmark, bcs_id, BookmarkUpdateReason::ManualMove, None)?;

    let commit_result = transaction.commit().await?;

    if !commit_result {
        Err(format_err!("Logical failure while setting {:?}", bookmark))
    } else {
        info!(ctx.logger(), "Setting bookmark {:?} finished", bookmark);
        Ok(())
    }
}

fn create_bonsai_changeset_only(
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
    author: String,
    message: String,
    datetime: DateTime,
) -> Result<BonsaiChangeset, Error> {
    BonsaiChangesetMut {
        parents,
        author: author.clone(),
        author_date: datetime,
        committer: Some(author),
        committer_date: Some(datetime),
        message,
        extra: BTreeMap::new(),
        file_changes,
    }
    .freeze()
}
