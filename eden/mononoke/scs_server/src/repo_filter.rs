/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::Parser;
use regex::Regex;

/// Command line argument to filter repositories
#[derive(Parser, Debug)]
pub struct RepoFilterArgs {
    /// Filter repositories using regex
    #[clap(long, value_parser = Regex::new)]
    pub filter_repos: Option<Regex>,
}
