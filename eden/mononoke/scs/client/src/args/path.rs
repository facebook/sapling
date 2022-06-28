/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for path selection.

use clap::App;
use clap::Arg;
use clap::ArgMatches;

const ARG_PATH: &str = "PATH";

/// Add arguments for specifying a path.
fn add_path_args_impl<'a, 'b>(app: App<'a, 'b>, required: bool, multiple: bool) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ARG_PATH)
            .short("p")
            .long("path")
            .help("Path")
            .takes_value(true)
            .required(required)
            .multiple(multiple),
    )
}

/// Add arguments for specifying a path.
pub(crate) fn add_path_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    add_path_args_impl(app, true, false)
}

/// Add arguments for optionally specifying a path.
pub(crate) fn add_optional_path_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    add_path_args_impl(app, false, false)
}

/// Add arguments for optionally specifying multiple path.
pub(crate) fn add_optional_multiple_path_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    add_path_args_impl(app, false, true)
}

/// Get the specified path.
pub(crate) fn get_path(matches: &ArgMatches) -> Option<String> {
    matches.value_of(ARG_PATH).map(String::from)
}

/// Get the specified paths.
pub(crate) fn get_paths(matches: &ArgMatches) -> Option<Vec<String>> {
    matches
        .values_of(ARG_PATH)
        .map(|i| i.map(String::from).collect())
}
