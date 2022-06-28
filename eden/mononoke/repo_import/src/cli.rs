/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ImportStage;
use crate::RecoveryFields;
use anyhow::Error;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;
use mononoke_types::DateTime;
use std::num::NonZeroUsize;
use std::time::Duration;

pub const IMPORT: &str = "import";
pub const ARG_GIT_REPOSITORY_PATH: &str = "git-repository-path";
pub const ARG_GIT_MERGE_REV_ID: &str = "git-merge-rev-id";
pub const ARG_DEST_PATH: &str = "dest-path";
pub const ARG_BATCH_SIZE: &str = "batch-size";
pub const ARG_BOOKMARK_SUFFIX: &str = "bookmark-suffix";
pub const ARG_PHAB_CHECK_DISABLED: &str = "disable-phabricator-check";
pub const ARG_X_REPO_CHECK_DISABLED: &str = "disable-x-repo-check";
pub const ARG_HG_SYNC_CHECK_DISABLED: &str = "disable-hg-sync-check";
pub const ARG_SLEEP_TIME: &str = "sleep-time-secs";
pub const ARG_DEST_BOOKMARK: &str = "dest-bookmark";
pub const ARG_COMMIT_MESSAGE: &str = "commit-message";
pub const ARG_COMMIT_AUTHOR: &str = "commit-author";
pub const ARG_COMMIT_DATE_RFC3339: &str = "commit-date-rfc3339";
pub const ARG_RECOVERY_FILE_PATH: &str = "recovery-file-path";
pub const RECOVER_PROCESS: &str = "recover-process";
pub const SAVED_RECOVERY_FILE_PATH: &str = "saved-recovery-file-path";
pub const CHECK_ADDITIONAL_SETUP_STEPS: &str = "check-additional-setup-steps";

pub fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("Import Repository")
        .with_advanced_args_hidden()
        .build()
        .about("Automating repository imports")
        .subcommand(
            SubCommand::with_name(CHECK_ADDITIONAL_SETUP_STEPS)
            .about("Check for additional setup steps before running the repo_import tool")
            .arg(
                Arg::with_name(ARG_PHAB_CHECK_DISABLED)
                    .long(ARG_PHAB_CHECK_DISABLED)
                    .takes_value(false)
                    .help("Disable waiting for Phabricator to parse commits."),
            )
            .arg(
                Arg::with_name(ARG_BOOKMARK_SUFFIX)
                    .long(ARG_BOOKMARK_SUFFIX)
                    .required(true)
                    .takes_value(true)
                    .help("Suffix of the bookmark (repo_import_<suffix>). \
                    This bookmark is used to publish the imported commits and to track the parsing of commits on Phabricator."),
            )
            .arg(
                Arg::with_name(ARG_DEST_BOOKMARK)
                    .long(ARG_DEST_BOOKMARK)
                    .takes_value(true)
                    .required(true)
                    .help("The bookmark branch we want to merge our repo into (e.g. master)"),
            )
        )
        .subcommand(
            SubCommand::with_name(IMPORT)
                .about("Run the whole repo_import process")
                .arg(
                    Arg::with_name(ARG_GIT_REPOSITORY_PATH)
                        .required(true)
                        .help("Path to a git repository to import"),
                )
                .arg(
                    Arg::with_name(ARG_GIT_MERGE_REV_ID)
                        .long(ARG_GIT_MERGE_REV_ID)
                        .takes_value(true)
                        .required(true)
                        .help("Revision in a git repo which should be merged"),
                )
                .arg(
                    Arg::with_name(ARG_DEST_PATH)
                        .long(ARG_DEST_PATH)
                        .required(true)
                        .takes_value(true)
                        .help("Path to the destination folder we import to"),
                )
                .arg(
                    Arg::with_name(ARG_BATCH_SIZE)
                        .long(ARG_BATCH_SIZE)
                        .takes_value(true)
                        .default_value("100")
                        .help("Number of commits we make visible when moving the bookmark"),
                )
                .arg(
                    Arg::with_name(ARG_BOOKMARK_SUFFIX)
                        .long(ARG_BOOKMARK_SUFFIX)
                        .required(true)
                        .takes_value(true)
                        .help("Suffix of the bookmark (repo_import_<suffix>). \
                        This bookmark is used to publish the imported commits and to track the parsing of commits on Phabricator."),
                )
                .arg(
                    Arg::with_name(ARG_PHAB_CHECK_DISABLED)
                        .long(ARG_PHAB_CHECK_DISABLED)
                        .takes_value(false)
                        .help("Disable waiting for Phabricator to parse commits."),
                )
                .arg(
                    Arg::with_name(ARG_X_REPO_CHECK_DISABLED)
                        .long(ARG_X_REPO_CHECK_DISABLED)
                        .takes_value(false)
                        .help("Disable x_repo sync check after moving the bookmark"),
                )
                .arg(
                    Arg::with_name(ARG_HG_SYNC_CHECK_DISABLED)
                        .long(ARG_HG_SYNC_CHECK_DISABLED)
                        .takes_value(false)
                        .help("Disable hg sync check after moving the bookmark"),
                )
                .arg(
                    Arg::with_name(ARG_SLEEP_TIME)
                        .long(ARG_SLEEP_TIME)
                        .takes_value(true)
                        .default_value("5")
                        .help(
                            "Sleep time in seconds, if we fail dependent system (phabricator, hg_sync ...) checkers",
                        ),
                )
                .arg(
                    Arg::with_name(ARG_DEST_BOOKMARK)
                        .long(ARG_DEST_BOOKMARK)
                        .takes_value(true)
                        .required(true)
                        .help("The bookmark branch we want to merge our repo into (e.g. master)"),
                )
                .arg(
                    Arg::with_name(ARG_COMMIT_AUTHOR)
                        .help("commit author to use")
                        .long(ARG_COMMIT_AUTHOR)
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_COMMIT_MESSAGE)
                        .help("commit message to use")
                        .long(ARG_COMMIT_MESSAGE)
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_COMMIT_DATE_RFC3339)
                        .help("commit date to use (default is now)")
                        .long(ARG_COMMIT_DATE_RFC3339)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name(ARG_RECOVERY_FILE_PATH)
                        .long(ARG_RECOVERY_FILE_PATH)
                        .required(true)
                        .takes_value(true)
                        .help("File path to store the importing state for recovery in case the tool breaks"),
                )

        )
        .subcommand(
            SubCommand::with_name(RECOVER_PROCESS)
                .about("Repo_import tool process recovery in case of import failure")
                .arg(
                    Arg::with_name(SAVED_RECOVERY_FILE_PATH)
                        .help("File path to fetch the recovery state for repo_import tool")
                        .required(true)
                        .takes_value(true),
                )
        )
}

pub fn setup_import_args(matches: &ArgMatches<'_>) -> Result<RecoveryFields, Error> {
    let import_stage = ImportStage::GitImport;
    let recovery_file_path = matches.value_of(ARG_RECOVERY_FILE_PATH).unwrap();
    let git_repo_path = matches.value_of(ARG_GIT_REPOSITORY_PATH).unwrap();
    let git_merge_rev_id = matches.value_of(ARG_GIT_MERGE_REV_ID).unwrap();
    let dest_path = matches.value_of(ARG_DEST_PATH).unwrap();
    let bookmark_suffix = matches.value_of(ARG_BOOKMARK_SUFFIX).unwrap();
    let batch_size = matches.value_of(ARG_BATCH_SIZE).unwrap();
    let batch_size = batch_size.parse::<NonZeroUsize>()?.get();
    let phab_check_disabled = matches.is_present(ARG_PHAB_CHECK_DISABLED);
    let x_repo_check_disabled = matches.is_present(ARG_X_REPO_CHECK_DISABLED);
    let hg_sync_check_disabled = matches.is_present(ARG_HG_SYNC_CHECK_DISABLED);
    let sleep_time = matches.value_of(ARG_SLEEP_TIME).unwrap();
    let sleep_time = Duration::from_secs(sleep_time.parse::<u64>()?);
    let dest_bookmark_name = matches.value_of(ARG_DEST_BOOKMARK).unwrap();
    let commit_author = matches.value_of(ARG_COMMIT_AUTHOR).unwrap();
    let commit_message = matches.value_of(ARG_COMMIT_MESSAGE).unwrap();
    let datetime = match matches.value_of(ARG_COMMIT_DATE_RFC3339) {
        Some(date) => DateTime::from_rfc3339(date)?,
        None => DateTime::now(),
    };
    Ok(RecoveryFields {
        import_stage,
        recovery_file_path: recovery_file_path.to_string(),
        git_merge_rev_id: git_merge_rev_id.to_string(),
        git_repo_path: git_repo_path.to_string(),
        dest_path: dest_path.to_string(),
        bookmark_suffix: bookmark_suffix.to_string(),
        batch_size,
        move_bookmark_commits_done: 0,
        phab_check_disabled,
        x_repo_check_disabled,
        hg_sync_check_disabled,
        sleep_time,
        dest_bookmark_name: dest_bookmark_name.to_string(),
        commit_author: commit_author.to_string(),
        commit_message: commit_message.to_string(),
        datetime,
        imported_cs_id: None,
        shifted_bcs_ids: None,
        gitimport_bcs_ids: None,
        merged_cs_id: None,
        git_merge_bcs_id: None,
    })
}
