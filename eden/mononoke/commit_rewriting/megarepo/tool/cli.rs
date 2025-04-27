/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Arg;
use clap::ArgGroup;
use clap::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;

pub const CATCHUP_VALIDATE_COMMAND: &str = "catchup-validate";
pub const CHANGESET: &str = "commit";
pub const COMMIT_BOOKMARK: &str = "bookmark";
pub const COMMIT_HASH: &str = "commit-hash";
pub const DRY_RUN: &str = "dry-run";
pub const GRADUAL_MERGE_PROGRESS: &str = "gradual-merge-progress";
pub const LAST_DELETION_COMMIT: &str = "last-deletion-commit";
pub const MANUAL_COMMIT_SYNC: &str = "manual-commit-sync";
pub const MAPPING_VERSION_NAME: &str = "mapping-version-name";
pub const PARENTS: &str = "parents";
pub const PATH_REGEX: &str = "path-regex";
pub const PRE_DELETION_COMMIT: &str = "pre-deletion-commit";
pub const SELECT_PARENTS_AUTOMATICALLY: &str = "select-parents-automatically";

pub const SYNC_COMMIT_AND_ANCESTORS: &str = "sync-commit-and-ancestors";

pub const TO_MERGE_CS_ID: &str = "to-merge-cs-id";

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

    let manual_commit_sync_subcommand = SubCommand::with_name(MANUAL_COMMIT_SYNC)
        .about("Manually sync a commit from source repo to a target repo. It's usually used right after a big merge")
        .arg(
            Arg::with_name(CHANGESET)
                .long(CHANGESET)
                .help("Source repo changeset that will synced to target repo")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(DRY_RUN)
                .long(DRY_RUN)
                .help("Dry-run mode - doesn't do a merge, just validates")
                .takes_value(false)
                .required(false),
        )
        .arg(
            Arg::with_name(PARENTS)
                .long(PARENTS)
                .help("Parents of the new commit")
                .takes_value(true)
                .multiple(true)
        )
        .arg(
            Arg::with_name(SELECT_PARENTS_AUTOMATICALLY)
                .long(SELECT_PARENTS_AUTOMATICALLY)
                .help("Finds parents automatically: takes parents in the source repo and finds equivalent commits in target repo. \
                If parents are not remapped yet then this command will fail")
                .takes_value(false)
        )
        .arg(
            Arg::with_name(MAPPING_VERSION_NAME)
                .long(MAPPING_VERSION_NAME)
                .help("name of the noop mapping that will be inserted")
                .takes_value(true)
                .required(true),
        )
        .group(
            ArgGroup::with_name("parents_group")
                .args(&[SELECT_PARENTS_AUTOMATICALLY, PARENTS])
                .required(true)
        );

    let catchup_validate_subcommand = SubCommand::with_name(CATCHUP_VALIDATE_COMMAND)
        .about("validate invariants about the catchup")
        .arg(
            Arg::with_name(COMMIT_HASH)
                .long(COMMIT_HASH)
                .help("merge commit i.e. commit where all catchup commits were merged into")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(TO_MERGE_CS_ID)
                .long(TO_MERGE_CS_ID)
                .help("commit to merge")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(PATH_REGEX)
                .long(PATH_REGEX)
                .help("regex that matches all paths that should be merged in head commit")
                .takes_value(true)
                .required(true),
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
        .subcommand(manual_commit_sync_subcommand)
        .subcommand(catchup_validate_subcommand)
        .subcommand(sync_commit_and_ancestors)
}
