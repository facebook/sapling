/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks::BookmarkUpdateLogId;
use clap::Parser;
use mononoke_app::args::RepoArgs;

#[derive(clap::Subcommand, Debug)]
pub(crate) enum SubcommandValidator {
    /// Validate a single entry (mainly for integration tests)
    Once {
        /// A commit to validate
        #[clap(long)]
        entry_id: BookmarkUpdateLogId,
    },
    /// Validates entries in a loop, tailing bookmarks_update_log
    Tail {
        /// Starting BookmarksUpdateLog entry id to use (ignores the mutable_counters)
        #[clap(long)]
        start_id: Option<BookmarkUpdateLogId>,
    },
}

/// Mirroring hg commits
#[derive(Parser, Debug)]
#[clap(about = "Mononoke cross-repo sync validator job ")]
pub(crate) struct MononokeCommitValidatorArgs {
    #[clap(flatten)]
    pub(crate) repo: RepoArgs,
    /// Name of the master bookmark in this repo
    #[clap(long, default_value = "master")]
    pub(crate) master_bookmark: String,
    #[clap(subcommand)]
    pub(crate) subcommand: SubcommandValidator,
}
