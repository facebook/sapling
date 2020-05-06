/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use cloned::cloned;
use context::CoreContext;
use futures_old::future::{err, ok, Future};
use futures_old::IntoFuture;
use slog::info;

use futures_ext::FutureExt;
use mercurial_types::{HgChangesetId, MPath};
use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime, FileChange};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct ChangesetArgs {
    pub author: String,
    pub message: String,
    pub datetime: DateTime,
    pub bookmark: Option<BookmarkName>,
    pub mark_public: bool,
}

pub fn create_and_save_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
    changeset_args: ChangesetArgs,
) -> impl Future<Item = HgChangesetId, Error = Error> {
    let ChangesetArgs {
        author,
        message,
        datetime,
        bookmark: maybe_bookmark,
        mark_public,
    } = changeset_args;
    create_bonsai_changeset_only(parents, file_changes, author, message, datetime)
        .and_then({
            cloned!(ctx, repo);
            move |bcs| save_and_maybe_mark_public(ctx, repo, bcs, mark_public)
        })
        .and_then({
            cloned!(ctx, repo);
            move |bcs_id| match maybe_bookmark {
                Some(bookmark) => create_bookmark(ctx, repo, bookmark, bcs_id.clone())
                    .map(move |_| bcs_id)
                    .left_future(),
                None => ok(bcs_id).right_future(),
            }
        })
        .and_then({
            cloned!(ctx, repo);
            move |bcs_id| generate_hg_changeset(ctx, repo, bcs_id)
        })
}

fn save_and_maybe_mark_public(
    ctx: CoreContext,
    repo: BlobRepo,
    bcs: BonsaiChangeset,
    mark_public: bool,
) -> impl Future<Item = ChangesetId, Error = Error> {
    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs.clone()], ctx.clone(), repo.clone()).and_then({
        cloned!(ctx, repo);
        move |_| {
            if mark_public {
                repo.get_phases()
                    .add_reachable_as_public(ctx.clone(), vec![bcs_id.clone()])
                    .map(move |_| {
                        info!(ctx.logger(), "Marked as public {:?}", bcs_id);
                        bcs_id
                    })
                    .left_future()
            } else {
                ok(bcs_id).right_future()
            }
        }
    })
}

fn generate_hg_changeset(
    ctx: CoreContext,
    repo: BlobRepo,
    bcs_id: ChangesetId,
) -> impl Future<Item = HgChangesetId, Error = Error> {
    info!(ctx.logger(), "Generating an HG equivalent of {:?}", bcs_id);
    repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id.clone())
        .map({
            cloned!(ctx);
            move |hg_cs_id| {
                info!(
                    ctx.logger(),
                    "Hg equivalent of {:?} is: {:?}", bcs_id, hg_cs_id
                );
                hg_cs_id
            }
        })
}

fn create_bookmark(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark: BookmarkName,
    bcs_id: ChangesetId,
) -> impl Future<Item = (), Error = Error> {
    info!(
        ctx.logger(),
        "Setting bookmark {:?} to point to {:?}", bookmark, bcs_id
    );
    let mut transaction = repo.clone().update_bookmark_transaction(ctx.clone());
    if let Err(e) =
        transaction.force_set(&bookmark, bcs_id.clone(), BookmarkUpdateReason::ManualMove)
    {
        return err(e).left_future();
    }
    transaction
        .commit()
        .and_then({
            cloned!(ctx);
            move |commit_result| {
                if !commit_result {
                    err(format_err!("Logical failure while setting {:?}", bookmark))
                } else {
                    info!(ctx.logger(), "Setting bookmark {:?} finished", bookmark);
                    ok(bcs_id)
                }
            }
        })
        .map(|_| ())
        .right_future()
}

fn create_bonsai_changeset_only(
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
    author: String,
    message: String,
    datetime: DateTime,
) -> impl Future<Item = BonsaiChangeset, Error = Error> {
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
    .into_future()
}
