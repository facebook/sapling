/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::{App, Arg};
use cmdlib::args;

pub const ARG_GIT_REPOSITORY_PATH: &str = "git-repository-path";
pub const ARG_DEST_PATH: &str = "dest-path";
pub const ARG_BATCH_SIZE: &str = "batch-size";
pub const ARG_BOOKMARK_SUFFIX: &str = "bookmark-suffix";
pub const ARG_CALL_SIGN: &str = "call-sign";
pub const ARG_PHAB_CHECK_DISABLED: &str = "disable-phabricator-check";
pub const ARG_X_REPO_CHECK_DISABLED: &str = "disable-x-repo-check";
pub const ARG_HG_SYNC_CHECK_DISABLED: &str = "disable-hg-sync-check";
pub const ARG_SLEEP_TIME: &str = "sleep-time";
pub const ARG_BACKUP_HASHES_FILE_PATH: &str = "backup-hashes-file-path";
pub const ARG_DEST_BOOKMARK: &str = "dest-bookmark";
pub const ARG_COMMIT_MESSAGE: &'static str = "commit-message";
pub const ARG_COMMIT_AUTHOR: &'static str = "commit-author";
pub const ARG_COMMIT_DATE_RFC3339: &'static str = "commit-date-rfc3339";

pub fn setup_app<'a, 'b>() -> App<'a, 'b> {
    args::MononokeApp::new("Import Repository")
        .with_advanced_args_hidden()
        .with_test_args()
        .build()
        .version("0.0.0")
        .about("Automating repository imports")
        .arg(
            Arg::with_name(ARG_GIT_REPOSITORY_PATH)
                .required(true)
                .help("Path to a git repository to import"),
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
                .help("Suffix of the bookmark (repo_import_<suffix>)"),
        )
        .arg(
            Arg::with_name(ARG_CALL_SIGN)
                .long(ARG_CALL_SIGN)
                .takes_value(true)
                .help("Call sign to get commit info from Phabricator. e.g. FBS for fbsource"),
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
                .default_value("1")
                .help(
                    "Sleep time, if we fail dependent system (phabricator, hg_sync ...) checkers",
                ),
        )
        .arg(
            Arg::with_name(ARG_BACKUP_HASHES_FILE_PATH)
                .long(ARG_BACKUP_HASHES_FILE_PATH)
                .takes_value(true)
                .required(true)
                .help("Backup file path to save bonsai hashes if deriving data types fail"),
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
}
