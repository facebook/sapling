/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bookmarks::Bookmarks;
use bookmarks::BookmarksRef;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;

/// Modify a bookmark
///
/// Like `mononoke-admin bookmarks`, except performs fewer checks and allows
/// overriding of the bookmark update reason, so only suitable for use in
/// tests.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    /// Bookmark modification to perform.
    #[clap(subcommand)]
    op: BookmarkOperation,
}

#[derive(Subcommand)]
pub enum BookmarkOperation {
    Create {
        /// Bookmark to create
        bookmark: BookmarkName,

        /// Bookmark update reason
        #[clap(long, arg_enum, default_value = "test-move")]
        reason: BookmarkUpdateReason,

        /// Changeset bookmark should be created at
        #[clap(long, short = 't')]
        to: ChangesetId,
    },
    Update {
        /// Bookmark to update
        bookmark: BookmarkName,

        /// Bookmark update reason
        #[clap(long, arg_enum, default_value = "test-move")]
        reason: BookmarkUpdateReason,

        /// Changeset bookmark is being moved from
        ///
        /// If specified, the bookmark update will only succeed if the
        /// bookmark currently points at this changeset.  If omitted, the
        /// bookmark is force-moved to the new location, no matter where it
        /// currently points.
        #[clap(long, short = 'f')]
        from: Option<ChangesetId>,

        /// Changeset bookmark should be moved to
        #[clap(long, short = 't')]
        to: ChangesetId,
    },
    Delete {
        /// Bookmark to delete
        bookmark: BookmarkName,

        /// Bookmark update reason
        #[clap(long, arg_enum, default_value = "test-move")]
        reason: BookmarkUpdateReason,

        /// Changeset bookmark is being deleted from
        ///
        /// If specified, the bookmark delete will only succeed if the
        /// bookmarks currently points at this changeset.  If omitted, the
        /// bookmark is force-deleted no matter where it points to.
        #[clap(long, short = 'f')]
        from: Option<ChangesetId>,
    },
}

#[facet::container]
pub struct Repo {
    #[facet]
    bookmarks: dyn Bookmarks,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: Repo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    let mut txn = repo.bookmarks().create_transaction(ctx.clone());

    match args.op {
        BookmarkOperation::Create {
            bookmark,
            reason,
            to,
        } => {
            txn.create(&bookmark, to, reason)?;
        }
        BookmarkOperation::Update {
            bookmark,
            reason,
            from: Some(from),
            to,
        } => {
            txn.update(&bookmark, to, from, reason)?;
        }
        BookmarkOperation::Update {
            bookmark,
            reason,
            from: None,
            to,
        } => {
            txn.force_set(&bookmark, to, reason)?;
        }
        BookmarkOperation::Delete {
            bookmark,
            reason,
            from: Some(from),
        } => {
            txn.delete(&bookmark, from, reason)?;
        }
        BookmarkOperation::Delete {
            bookmark,
            reason,
            from: None,
        } => {
            txn.force_delete(&bookmark, reason)?;
        }
    }

    let success = txn
        .commit()
        .await
        .context("Failed to commit bookmark transaction")?;

    if !success {
        bail!("Bookmark transaction failed");
    }

    Ok(())
}
