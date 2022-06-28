/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for repository selection.

use clap::App;
use clap::Arg;
use clap::ArgMatches;
use source_control::types as thrift;

const ARG_REPO: &str = "REPO";

/// Add arguments for specifying a repository.
pub(crate) fn add_repo_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ARG_REPO)
            .short("R")
            .long("repo")
            .help("Repository name")
            .takes_value(true)
            .required(true),
    )
}

/// Get the specified repository as a thrift specifier.
pub(crate) fn get_repo_specifier(matches: &ArgMatches) -> Option<thrift::RepoSpecifier> {
    matches
        .value_of(ARG_REPO)
        .map(|name| thrift::RepoSpecifier {
            name: name.to_string(),
            ..Default::default()
        })
}
