/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap_old::Arg;
use clap_old::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;

pub const ARG_ONCE: &str = "once";
pub const ARG_COMMIT: &str = "commit";
pub const ARG_TAIL: &str = "tail";
pub const ARG_TARGET_BOOKMARK: &str = "target-bookmark";
pub const ARG_CATCH_UP_ONCE: &str = "catch-up-once";
pub const ARG_LOG_TO_SCUBA: &str = "log-to-scuba";
pub const ARG_BACKSYNC_BACKPRESSURE_REPOS_IDS: &str = "backsync-backpressure-repo-ids";
pub const ARG_HG_SYNC_BACKPRESSURE: &str = "hg-sync-backpressure";
pub const ARG_DERIVED_DATA_TYPES: &str = "derived-data-types";
pub const ARG_SLEEP_SECS: &str = "sleep-secs";
pub const ARG_BOOKMARK_REGEX: &str = "bookmark-regex";

pub fn create_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    let app = args::MononokeAppBuilder::new("Mononoke cross-repo sync job")
        .with_fb303_args()
        .with_source_and_target_repos()
        .build();

    let app = app.arg(
        Arg::with_name(ARG_LOG_TO_SCUBA)
            .long(ARG_LOG_TO_SCUBA)
            .takes_value(false)
            .help("Whether the progress of the tailer should be logged to scuba"),
    );

    let once = SubCommand::with_name(ARG_ONCE)
        .about("Syncs a single commit")
        .arg(
            Arg::with_name(ARG_TARGET_BOOKMARK)
                .long(ARG_TARGET_BOOKMARK)
                .takes_value(true)
                .required(false)
                .help("A bookmark in the target repo to sync to"),
        )
        .arg(
            Arg::with_name(ARG_COMMIT)
                .long(ARG_COMMIT)
                .takes_value(true)
                .required(true)
                .help("A commit to sync"),
        );

    let tail = SubCommand::with_name(ARG_TAIL)
        .about("Syncs commits in a loop")
        .arg(
            Arg::with_name(ARG_SLEEP_SECS)
                .long(ARG_SLEEP_SECS)
                .takes_value(true)
                .help("Sleep this many seconds on the no-op iteration/while applying backpressure"),
        )
        .arg(
            Arg::with_name(ARG_CATCH_UP_ONCE)
                .long(ARG_CATCH_UP_ONCE)
                .takes_value(false)
                .help(
                    "Only catch up until the current state of \
                     bookmarks_update_log and do not wait indefinitely",
                ),
        )
        .arg(
            Arg::with_name(ARG_BACKSYNC_BACKPRESSURE_REPOS_IDS)
                .long(ARG_BACKSYNC_BACKPRESSURE_REPOS_IDS)
                .takes_value(true)
                .multiple(true)
                .required(false)
                .help(
                    "Monitors how many entries to backsync in the queue for other repos and pauses syncing if queue is too large",
                ),
        )
        .arg(
            Arg::with_name(ARG_HG_SYNC_BACKPRESSURE)
                .long(ARG_HG_SYNC_BACKPRESSURE)
                .required(false)
                .help("Waits until new commits created in the target repo are synced to hg"),
        )
        .arg(
            Arg::with_name(ARG_DERIVED_DATA_TYPES)
                .long(ARG_DERIVED_DATA_TYPES)
                .takes_value(true)
                .multiple(true)
                .required(false)
                .help(
                    "derived data to derive in target repo after sync",
                ),
        )
        .arg(
            Arg::with_name(ARG_BOOKMARK_REGEX)
                .long(ARG_BOOKMARK_REGEX)
                .takes_value(true)
                .required(false)
                .help(
                    "sync only bookmarks that match the regex",
                ),
        );

    let app = app.subcommand(once).subcommand(tail);
    app
}
