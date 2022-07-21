/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use bookmarks::BookmarkName;
use clap::App;
use clap::Arg;
use clap::ArgGroup;
use clap::ArgMatches;
use clap::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;
use futures_ext::try_boxfuture;
use futures_ext::BoxFuture;
use futures_ext::FutureExt;
use futures_old::future::err;
use futures_old::future::ok;
use megarepolib::common::ChangesetArgs;
use megarepolib::common::ChangesetArgsFactory;
use megarepolib::common::StackPosition;
use mononoke_types::DateTime;

pub const BACKFILL_NOOP_MAPPING: &str = "backfill-noop-mapping";
pub const BASE_COMMIT_HASH: &str = "base-commit-hash";
pub const BONSAI_MERGE_P1: &str = "bonsai-merge-p1";
pub const BONSAI_MERGE_P2: &str = "bonsai-merge-p2";
pub const BONSAI_MERGE: &str = "bonsai-merge";
pub const CATCHUP_DELETE_HEAD: &str = "create-catchup-head-deletion-commits";
pub const CATCHUP_VALIDATE_COMMAND: &str = "catchup-validate";
pub const CHANGESET: &str = "commit";
pub const CHECK_PUSH_REDIRECTION_PREREQS: &str = "check-push-redirection-prereqs";
pub const CHUNKING_HINT_FILE: &str = "chunking-hint-file";
pub const COMMIT_AUTHOR: &str = "commit-author";
pub const COMMIT_BOOKMARK: &str = "bookmark";
pub const COMMIT_DATE_RFC3339: &str = "commit-date-rfc3339";
pub const COMMIT_HASH: &str = "commit-hash";
pub const COMMIT_HASH_CORRECT_HISTORY: &str = "commit-hash-correct-history";
pub const COMMIT_MESSAGE: &str = "commit-message";
pub const DELETE_NO_LONGER_BOUND_FILES_FROM_LARGE_REPO: &str =
    "delete-no-longer-bound-files-from-large-repo";
pub const DELETION_CHUNK_SIZE: &str = "deletion-chunk-size";
pub const DIFF_MAPPING_VERSIONS: &str = "diff-mapping-versions";
pub const DRY_RUN: &str = "dry-run";
pub const EVEN_CHUNK_SIZE: &str = "even-chunk-size";
pub const FIRST_PARENT: &str = "first-parent";
pub const GRADUAL_MERGE_PROGRESS: &str = "gradual-merge-progress";
pub const GRADUAL_MERGE: &str = "gradual-merge";
pub const GRADUAL_DELETE: &str = "gradual-delete";
pub const HEAD_BOOKMARK: &str = "head-bookmark";
pub const HISTORY_FIXUP_DELETE: &str = "history-fixup-deletes";
pub const INPUT_FILE: &str = "input-file";
pub const LAST_DELETION_COMMIT: &str = "last-deletion-commit";
pub const LIMIT: &str = "limit";
pub const MANUAL_COMMIT_SYNC: &str = "manual-commit-sync";
pub const MAPPING_VERSION_NAME: &str = "mapping-version-name";
pub const MARK_NOT_SYNCED_COMMAND: &str = "mark-not-synced";
pub const MARK_PUBLIC: &str = "mark-public";
pub const MAX_NUM_OF_MOVES_IN_COMMIT: &str = "max-num-of-moves-in-commit";
pub const MERGE: &str = "merge";
pub const MOVE: &str = "move";
pub const ORIGIN_REPO: &str = "origin-repo";
pub const OVERWRITE: &str = "overwrite";
pub const PARENTS: &str = "parents";
pub const PATH_REGEX: &str = "path-regex";
pub const PATH: &str = "path";
pub const PATH_PREFIX: &str = "path-prefix";
pub const PATHS_FILE: &str = "paths-file";
pub const PRE_DELETION_COMMIT: &str = "pre-deletion-commit";
pub const PRE_MERGE_DELETE: &str = "pre-merge-delete";
pub const RUN_MOVER: &str = "run-mover";
pub const SECOND_PARENT: &str = "second-parent";
pub const SELECT_PARENTS_AUTOMATICALLY: &str = "select-parents-automatically";
pub const SOURCE_CHANGESET: &str = "source-changeset";
pub const SYNC_COMMIT_AND_ANCESTORS: &str = "sync-commit-and-ancestors";
pub const SYNC_DIAMOND_MERGE: &str = "sync-diamond-merge";
pub const TARGET_CHANGESET: &str = "target-changeset";
pub const TO_MERGE_CS_ID: &str = "to-merge-cs-id";
pub const VERSION: &str = "version";
pub const WAIT_SECS: &str = "wait-secs";

pub fn cs_args_from_matches<'a>(sub_m: &ArgMatches<'a>) -> BoxFuture<ChangesetArgs, Error> {
    let message = try_boxfuture!(
        sub_m
            .value_of(COMMIT_MESSAGE)
            .ok_or_else(|| format_err!("missing argument {}", COMMIT_MESSAGE))
    )
    .to_string();
    let author = try_boxfuture!(
        sub_m
            .value_of(COMMIT_AUTHOR)
            .ok_or_else(|| format_err!("missing argument {}", COMMIT_AUTHOR))
    )
    .to_string();
    let datetime = try_boxfuture!(
        sub_m
            .value_of(COMMIT_DATE_RFC3339)
            .map_or_else(|| Ok(DateTime::now()), DateTime::from_rfc3339)
    );
    let bookmark = try_boxfuture!(
        sub_m
            .value_of(COMMIT_BOOKMARK)
            .map(BookmarkName::new)
            .transpose()
    );
    let mark_public = sub_m.is_present(MARK_PUBLIC);
    if !mark_public && bookmark.is_some() {
        return err(format_err!(
            "--mark-public is required if --bookmark is provided"
        ))
        .boxify();
    }

    ok(ChangesetArgs {
        author,
        message,
        datetime,
        bookmark,
        mark_public,
    })
    .boxify()
}

pub fn get_delete_commits_cs_args_factory<'a>(
    sub_m: &ArgMatches<'a>,
) -> Result<Box<dyn ChangesetArgsFactory>, Error> {
    get_commit_factory(sub_m, |s, num| -> String {
        format!("[MEGAREPO DELETE] {} ({})", s, num)
    })
}

pub fn get_catchup_head_delete_commits_cs_args_factory<'a>(
    sub_m: &ArgMatches<'a>,
) -> Result<Box<dyn ChangesetArgsFactory>, Error> {
    get_commit_factory(sub_m, |s, num| -> String {
        format!("[MEGAREPO CATCHUP DELETE] {} ({})", s, num)
    })
}

pub fn get_gradual_merge_commits_cs_args_factory<'a>(
    sub_m: &ArgMatches<'a>,
) -> Result<Box<dyn ChangesetArgsFactory>, Error> {
    get_commit_factory(sub_m, |s, num| -> String {
        format!("[MEGAREPO GRADUAL MERGE] {} ({})", s, num)
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

fn add_resulting_commit_args<'a, 'b>(subcommand: App<'a, 'b>) -> App<'a, 'b> {
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
            Arg::with_name(MARK_PUBLIC)
                .help("add the resulting commit to the public phase")
                .long(MARK_PUBLIC),
        )
        .arg(
            Arg::with_name(COMMIT_DATE_RFC3339)
                .help("commit date to use (default is now)")
                .long(COMMIT_DATE_RFC3339)
                .takes_value(true),
        )
        .arg(
            Arg::with_name(COMMIT_BOOKMARK)
                .help("bookmark to point to resulting commits (no sanity checks, will move existing bookmark, be careful)")
                .long(COMMIT_BOOKMARK)
                .takes_value(true)
        )
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
    let move_subcommand = SubCommand::with_name(MOVE)
        .about("create a move commit, using a provided spec")
        .arg(
            Arg::with_name(MAX_NUM_OF_MOVES_IN_COMMIT)
                .long(MAX_NUM_OF_MOVES_IN_COMMIT)
                .help("how many files a single commit moves (note - that might create a stack of move commits instead of just one)")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(MAPPING_VERSION_NAME)
                .long(MAPPING_VERSION_NAME)
                .help("which mapping version to use when remapping from small to large repo")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(ORIGIN_REPO)
                .help("use predefined mover for part of megarepo, coming from this repo")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(CHANGESET)
                .help("a changeset hash or bookmark of move commit's parent")
                .takes_value(true)
                .required(true),
        );

    let merge_subcommand = SubCommand::with_name(MERGE)
        .about("create a merge commit with given parents")
        .arg(
            Arg::with_name(FIRST_PARENT)
                .help("first parent of a produced merge commit")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(SECOND_PARENT)
                .help("second parent of a produced merge commit")
                .takes_value(true)
                .required(true),
        );

    let sync_diamond_subcommand = SubCommand::with_name(SYNC_DIAMOND_MERGE)
        .about("sync a diamond merge commit from a small repo into large repo")
        .arg(
            Arg::with_name(COMMIT_HASH)
                .help("diamond merge commit from small repo to sync")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(COMMIT_BOOKMARK)
                .help("bookmark to point to resulting commits (no sanity checks, will move existing bookmark, be careful)")
                .long(COMMIT_BOOKMARK)
                .takes_value(true)
        );

    let pre_merge_delete_subcommand = SubCommand::with_name(PRE_MERGE_DELETE)
        .about("create a set of pre-merge delete commits (which remove all of the files in working copy)")
        .arg(
            Arg::with_name(COMMIT_HASH)
                .help("commit from which to start deletion")
                .takes_value(true)
                .required(true)
        )
        .arg(
            Arg::with_name(CHUNKING_HINT_FILE)
                .help(r#"a path to working copy chunking hint. If not provided, working copy will
                        be chunked evenly into `--even-chunk-size` commits"#)
                .long(CHUNKING_HINT_FILE)
                .takes_value(true)
                .required(false)
        )
        .arg(
            Arg::with_name(EVEN_CHUNK_SIZE)
                .help("chunk size for even chunking when --chunking-hing-file is not provided")
                .long(EVEN_CHUNK_SIZE)
                .takes_value(true)
                .required(false)
        )
        .arg(
            Arg::with_name(BASE_COMMIT_HASH)
                .help("commit that will be diffed against to find what files needs to be deleted - \
                 only files that don't exist or differ from base commit will be deleted.")
                .long(BASE_COMMIT_HASH)
                .takes_value(true)
                .required(false)
        );

    let history_fixup_delete_subcommand =
        add_light_resulting_commit_args(SubCommand::with_name(HISTORY_FIXUP_DELETE))
            .about("create a set of delete commits before the path fixup.")
            .arg(
                Arg::with_name(COMMIT_HASH)
                    .help(
                        "commit which we want to fixup (the
                         files specified in paths file will be deleted there)",
                    )
                    .takes_value(true)
                    .required(true),
            )
            .arg(
                Arg::with_name(COMMIT_HASH_CORRECT_HISTORY)
                    .help(
                        "commit hash containing the files with correct
                         history (the files specified in path files will be
                         preserved there; all the other files will be deleted)",
                    )
                    .takes_value(true)
                    .required(true),
            )
            .arg(
                Arg::with_name(EVEN_CHUNK_SIZE)
                    .help("chunk size for even chunking")
                    .long(EVEN_CHUNK_SIZE)
                    .takes_value(true)
                    .required(true),
            )
            .arg(
                Arg::with_name(PATHS_FILE)
                    .long(PATHS_FILE)
                    .help("file containing paths to fixup separated by newlines")
                    .takes_value(true)
                    .required(true)
                    .multiple(true),
            );

    // PLease don't move `add_light_resulting_commit_args` to be applied
    // after `PATH` arg is added, as in that case `PATH` won't be the last
    // positional argument
    let gradual_delete_subcommand =
        add_light_resulting_commit_args(SubCommand::with_name(GRADUAL_DELETE))
            .about("create a set of delete commits for given paths")
            .arg(
                Arg::with_name(COMMIT_HASH)
                    .help("commit from which to start deletion")
                    .takes_value(true)
                    .required(true),
            )
            .arg(
                Arg::with_name(EVEN_CHUNK_SIZE)
                    .help("chunk size for even chunking")
                    .long(EVEN_CHUNK_SIZE)
                    .takes_value(true)
                    .required(true),
            )
            .arg(
                Arg::with_name(PATH)
                    .help("paths to delete")
                    .takes_value(true)
                    .required(true)
                    .multiple(true),
            );

    let bonsai_merge_subcommand = SubCommand::with_name(BONSAI_MERGE)
        .about("create a bonsai merge commit")
        .arg(
            Arg::with_name(BONSAI_MERGE_P1)
                .help("p1 of the merge")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(BONSAI_MERGE_P2)
                .help("p2 of the merge")
                .takes_value(true)
                .required(true),
        );

    let gradual_merge_subcommand = SubCommand::with_name(GRADUAL_MERGE)
        .about("Gradually merge a list of deletion commits")
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
        )
        .arg(
            Arg::with_name(DRY_RUN)
                .long(DRY_RUN)
                .help("Dry-run mode - doesn't do a merge, just validates")
                .takes_value(false)
                .required(false),
        )
        .arg(
            Arg::with_name(LIMIT)
                .long(LIMIT)
                .help("how many commits to merge")
                .takes_value(true)
                .required(false),
        );

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

    let mark_not_synced_candidate = SubCommand::with_name(MARK_NOT_SYNCED_COMMAND)
        .about("mark all commits that do not have any mapping as not synced candidate, but leave those that have the mapping alone")
        .arg(
            Arg::with_name(MAPPING_VERSION_NAME)
                .help("a version to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(INPUT_FILE)
                .long(INPUT_FILE)
                .help("list of large repo commit hashes that should be considered to be marked as not sync candidate")
                .takes_value(true)
                .required(true)
        )
        .arg(
            Arg::with_name(OVERWRITE)
                .long(OVERWRITE)
                .help("whether to overwrite existing values or not")
                .takes_value(false)
                .required(false),
        );

    let check_push_redirection_prereqs_subcommand = SubCommand::with_name(CHECK_PUSH_REDIRECTION_PREREQS)
        .about("check the prerequisites of enabling push-redirection at a given commit with a given CommitSyncConfig version")
        .arg(
            Arg::with_name(SOURCE_CHANGESET)
                .help("a source changeset hash or bookmark to check")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(TARGET_CHANGESET)
                .help("a target changeset hash or bookmark to check")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(VERSION)
                .help("a version to use")
                .takes_value(true)
                .required(true),
        );

    let run_mover_subcommand = SubCommand::with_name(RUN_MOVER)
        .about("run mover of a given version to remap paths between source and target repos")
        .arg(
            Arg::with_name(VERSION)
                .help("a version to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(PATH)
                .help("a path to remap")
                .takes_value(true)
                .required(true),
        );

    let backfill_noop_mapping = SubCommand::with_name(BACKFILL_NOOP_MAPPING)
        .about(
            "
            Given the list of commit identifiers resolve them to bonsai hashes in source \
            and target repo and insert a sync commit mapping with specified version name. \
            This is useful for initial backfill to mark commits that are identical between \
            repositories. \
            Input file can contain any commit identifier (e.g. bookmark name) \
            but the safest approach is to use commit hashes (bonsai or hg). \
            'source-repo' argument represents the small repo while 'target-repo' is the large repo.
        ",
        )
        .arg(
            Arg::with_name(INPUT_FILE)
                .long(INPUT_FILE)
                .help("list of commit hashes which are remapped with noop mapping")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name(MAPPING_VERSION_NAME)
                .long(MAPPING_VERSION_NAME)
                .help("name of the noop mapping that will be inserted")
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

    let diff_mapping_versions = SubCommand::with_name(DIFF_MAPPING_VERSIONS)
        .about("Show difference between two mapping versions.")
        .arg(
            Arg::with_name(MAPPING_VERSION_NAME)
                .help("list of mapping versions")
                .takes_value(true)
                .multiple(true)
                .required(true),
        );

    let delete_no_longer_bound_files_from_large_repo = SubCommand::with_name(DELETE_NO_LONGER_BOUND_FILES_FROM_LARGE_REPO)
        .about("
        Right after small and large are bound usually a majority of small repo files map to a single folder \
        in large repo (let's call it DIR). Later these files from small repo might be bound to a another files in large repo \
        however files in DIR might still exist in large repo. \
        This command allows us to delete these files from DIR. It does so by finding all files in DIR and its subfolders \
        that do not remap to a small repo and then deleting them. \
        Note: if there are files in DIR that were never part of a bind, they will be deleted.
        ")
        .arg(
            Arg::with_name(COMMIT_HASH)
                .long(COMMIT_HASH)
                .required(true)
                .takes_value(true)
                .help("hg/bonsai changeset id or bookmark"),
        )
        .arg(
            Arg::with_name(PATH_PREFIX)
                .long(PATH_PREFIX)
                .required(true)
                .takes_value(true)
                .help("path prefix where to search for files to delete from"),
        );

    args::MononokeAppBuilder::new("megarepo preparation tool")
        .with_advanced_args_hidden()
        .with_source_and_target_repos()
        .build()
        .subcommand(add_resulting_commit_args(move_subcommand))
        .subcommand(add_resulting_commit_args(merge_subcommand))
        .subcommand(sync_diamond_subcommand)
        .subcommand(add_light_resulting_commit_args(pre_merge_delete_subcommand))
        .subcommand(history_fixup_delete_subcommand)
        .subcommand(add_light_resulting_commit_args(bonsai_merge_subcommand))
        .subcommand(add_light_resulting_commit_args(gradual_merge_subcommand))
        .subcommand(gradual_merge_progress_subcommand)
        .subcommand(gradual_delete_subcommand)
        .subcommand(manual_commit_sync_subcommand)
        .subcommand(add_light_resulting_commit_args(
            catchup_delete_head_subcommand,
        ))
        .subcommand(catchup_validate_subcommand)
        .subcommand(mark_not_synced_candidate)
        .subcommand(check_push_redirection_prereqs_subcommand)
        .subcommand(run_mover_subcommand)
        .subcommand(backfill_noop_mapping)
        .subcommand(sync_commit_and_ancestors)
        .subcommand(diff_mapping_versions)
        .subcommand(add_light_resulting_commit_args(
            delete_no_longer_bound_files_from_large_repo,
        ))
}
