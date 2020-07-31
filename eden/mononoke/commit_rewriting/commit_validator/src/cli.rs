/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::{App, Arg, SubCommand};
use cmdlib::args;

pub const ARG_ONCE: &'static str = "once";
pub const ARG_ENTRY_ID: &'static str = "entry-id";
pub const ARG_MASTER_BOOKMARK: &'static str = "master-bookmark";
pub const ARG_TAIL: &'static str = "tail";
pub const ARG_START_ID: &'static str = "start-id";

pub fn create_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp::new("Mononoke cross-repo sync validator job")
        .with_scuba_logging_args()
        .with_fb303_args()
        .with_test_args()
        .build();

    let app = app.arg(
        Arg::with_name(ARG_MASTER_BOOKMARK)
            .long(ARG_MASTER_BOOKMARK)
            .takes_value(true)
            .default_value("master")
            .help("Name of the master bookmark in this repo"),
    );

    let once = SubCommand::with_name(ARG_ONCE)
        .about("Validate a single entry (mainly for integration tests)")
        .arg(
            Arg::with_name(ARG_ENTRY_ID)
                .long(ARG_ENTRY_ID)
                .takes_value(true)
                .required(true)
                .help("A commit to validate"),
        );

    let tail = SubCommand::with_name(ARG_TAIL)
        .about("Validates entries in a loop, tailing bookmarks_update_log")
        .arg(
            Arg::with_name(ARG_START_ID)
                .long(ARG_START_ID)
                .takes_value(true)
                .help("Starting BookmarksUpdateLog entry id to use (ignores the mutable_counters)"),
        );

    let app = app.subcommand(once).subcommand(tail);
    app
}
