/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::{App, Arg, SubCommand};
use cmdlib::args;

pub const ARG_ONCE: &'static str = "once";
pub const ARG_COMMIT: &'static str = "commit";
pub const ARG_TAIL: &'static str = "tail";
pub const ARG_TARGET_BOOKMARK: &'static str = "target-bookmark";
pub const ARG_CATCH_UP_ONCE: &'static str = "catch-up-once";
pub const ARG_LOG_TO_SCUBA: &'static str = "log-to-scuba";
pub const ARG_BACKPRESSURE_REPOS_IDS: &'static str = "backpressure-repo-ids";
pub const ARG_DERIVED_DATA_TYPES: &'static str = "derived-data-types";
pub const ARG_SLEEP_SECS: &'static str = "sleep-secs";

pub fn create_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp::new("Mononoke cross-repo sync job")
        .with_fb303_args()
        .with_source_and_target_repos()
        .with_test_args()
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
                .required(true)
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
            Arg::with_name(ARG_BACKPRESSURE_REPOS_IDS)
                .long(ARG_BACKPRESSURE_REPOS_IDS)
                .takes_value(true)
                .multiple(true)
                .required(false)
                .help(
                    "Monitors how many entries to backsync in the queue for other repos and pauses syncing if queue is too large",
                ),
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
        );

    let app = app.subcommand(once).subcommand(tail);
    app
}
