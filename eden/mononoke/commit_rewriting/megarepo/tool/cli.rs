/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Arg;
use clap::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;

pub const COMMIT_BOOKMARK: &str = "bookmark";
pub const COMMIT_HASH: &str = "commit-hash";
pub const GRADUAL_MERGE_PROGRESS: &str = "gradual-merge-progress";
pub const LAST_DELETION_COMMIT: &str = "last-deletion-commit";
pub const PRE_DELETION_COMMIT: &str = "pre-deletion-commit";

pub const SYNC_COMMIT_AND_ANCESTORS: &str = "sync-commit-and-ancestors";

pub fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    let gradual_merge_progress_subcommand = SubCommand::with_name(GRADUAL_MERGE_PROGRESS)
        .about("Display progress of the gradual merge as #MERGED_COMMITS/#TOTAL_COMMITS_TO_MERGE")
        .arg(
            Arg::with_name(LAST_DELETION_COMMIT)
                .long(LAST_DELETION_COMMIT)
                .help("Last deletion commit")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(PRE_DELETION_COMMIT)
                .long(PRE_DELETION_COMMIT)
                .help("Commit right before the first deletion commit")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(COMMIT_BOOKMARK)
                .help("bookmark to point to resulting commits (no sanity checks, will move existing bookmark, be careful)")
                .long(COMMIT_BOOKMARK)
                .takes_value(true)
        );

    let sync_commit_and_ancestors = SubCommand::with_name(SYNC_COMMIT_AND_ANCESTORS)
        .about(
            "
            Command that syncs a commit and all of its unsynced ancestors from source repo \
            to target repo. This is similar to SCS commit_lookup_xrepo() method except that it \
            doesn't do all the safety checks that commit_lookup_xrepo(). In particular, it allows \
            to sync public small repo commits.
        ",
        )
        .arg(
            Arg::with_name(COMMIT_HASH)
                .long(COMMIT_HASH)
                .help("commit (and its ancestors) to sync")
                .takes_value(true)
                .required(true),
        );

    args::MononokeAppBuilder::new("megarepo preparation tool")
        .with_advanced_args_hidden()
        .with_source_and_target_repos()
        .build()
        .subcommand(gradual_merge_progress_subcommand)
        .subcommand(sync_commit_and_ancestors)
}
