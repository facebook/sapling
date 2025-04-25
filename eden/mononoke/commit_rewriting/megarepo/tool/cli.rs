/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::format_err;
use clap::App;
use clap::Arg;
use clap::ArgGroup;
use clap::ArgMatches;
use clap::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;
use megarepolib::common::ChangesetArgs;
use megarepolib::common::ChangesetArgsFactory;
use megarepolib::common::StackPosition;
use mononoke_types::DateTime;

pub const CATCHUP_DELETE_HEAD: &str = "create-catchup-head-deletion-commits";
pub const CATCHUP_VALIDATE_COMMAND: &str = "catchup-validate";
pub const CHANGESET: &str = "commit";
pub const COMMIT_AUTHOR: &str = "commit-author";
pub const COMMIT_BOOKMARK: &str = "bookmark";
pub const COMMIT_DATE_RFC3339: &str = "commit-date-rfc3339";
pub const COMMIT_HASH: &str = "commit-hash";
pub const COMMIT_MESSAGE: &str = "commit-message";
pub const DELETION_CHUNK_SIZE: &str = "deletion-chunk-size";
pub const DRY_RUN: &str = "dry-run";
pub const GRADUAL_MERGE_PROGRESS: &str = "gradual-merge-progress";
pub const HEAD_BOOKMARK: &str = "head-bookmark";
pub const LAST_DELETION_COMMIT: &str = "last-deletion-commit";
pub const MANUAL_COMMIT_SYNC: &str = "manual-commit-sync";
pub const MAPPING_VERSION_NAME: &str = "mapping-version-name";
pub const PARENTS: &str = "parents";
pub const PATH_REGEX: &str = "path-regex";
pub const PRE_DELETION_COMMIT: &str = "pre-deletion-commit";
pub const SELECT_PARENTS_AUTOMATICALLY: &str = "select-parents-automatically";

pub const SYNC_COMMIT_AND_ANCESTORS: &str = "sync-commit-and-ancestors";

pub const TO_MERGE_CS_ID: &str = "to-merge-cs-id";

pub const WAIT_SECS: &str = "wait-secs";

pub fn get_catchup_head_delete_commits_cs_args_factory<'a>(
    sub_m: &ArgMatches<'a>,
) -> Result<Box<dyn ChangesetArgsFactory>, Error> {
    get_commit_factory(sub_m, |s, num| -> String {
        format!("[MEGAREPO CATCHUP DELETE] {} ({})", s, num)
    })
}

fn get_commit_factory<'a>(
    sub_m: &ArgMatches<'a>,
    msg_factory: impl Fn(&String, usize) -> String + Send + Sync + 'static,
) -> Result<Box<dyn ChangesetArgsFactory>, Error> {
    let message = sub_m
        .value_of(COMMIT_MESSAGE)
        .ok_or_else(|| format_err!("missing argument {}", COMMIT_MESSAGE))?
        .to_string();

    let author = sub_m
        .value_of(COMMIT_AUTHOR)
        .ok_or_else(|| format_err!("missing argument {}", COMMIT_AUTHOR))?
        .to_string();

    let datetime = sub_m
        .value_of(COMMIT_DATE_RFC3339)
        .map(DateTime::from_rfc3339)
        .transpose()?
        .unwrap_or_else(DateTime::now);

    Ok(Box::new(move |num: StackPosition| ChangesetArgs {
        author: author.clone(),
        message: msg_factory(&message, num.0),
        datetime: datetime.clone(),
        bookmark: None,
        mark_public: false,
    }))
}

fn add_light_resulting_commit_args<'a, 'b>(subcommand: App<'a, 'b>) -> App<'a, 'b> {
    subcommand
        .arg(
            Arg::with_name(COMMIT_AUTHOR)
                .help("commit author to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(COMMIT_MESSAGE)
                .help("commit message to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(COMMIT_DATE_RFC3339)
                .help("commit date to use (default is now)")
                .long(COMMIT_DATE_RFC3339)
                .takes_value(true),
        )
}

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

    let catchup_delete_head_subcommand = SubCommand::with_name(CATCHUP_DELETE_HEAD)
        .about("Create delete commits for 'catchup strategy. \
        This is normally used after invisible merge is done, but small repo got a few new commits
        that needs merging.

        O         <-  head bookmark
        |
        O   O <-  new commits (we want to merge them in master)
        |  ...
        IM  |       <- invisible merge commit
        |\\ /
        O O

        This command create deletion commits on top of master bookmark for files that were changed in new commits,
        and pushrebases them.

        After all of the commits are pushrebased paths that match --path-regex in head bookmark should be a subset
        of all paths that match --path-regex in the latest new commit we want to merge.
        ")
        .arg(
            Arg::with_name(HEAD_BOOKMARK)
                .long(HEAD_BOOKMARK)
                .help("commit to merge into")
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
        )
        .arg(
            Arg::with_name(DELETION_CHUNK_SIZE)
                .long(DELETION_CHUNK_SIZE)
                .help("how many files to delete in a single commit")
                .default_value("10000")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(WAIT_SECS)
                .long(WAIT_SECS)
                .help("how many seconds to wait after each push")
                .default_value("0")
                .takes_value(true)
                .required(false),
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
        .subcommand(add_light_resulting_commit_args(
            catchup_delete_head_subcommand,
        ))
        .subcommand(catchup_validate_subcommand)
        .subcommand(sync_commit_and_ancestors)
}
