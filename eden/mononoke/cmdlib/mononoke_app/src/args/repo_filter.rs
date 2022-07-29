/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use regex::Regex;

use crate::AppExtension;
use environment::MononokeEnvironment;

/// Command line argument to filter repositories
#[derive(Parser, Debug)]
pub struct RepoFilterArgs {
    /// Filter repositories using regex
    #[clap(long, value_parser = Regex::new)]
    pub filter_repos: Option<Regex>,
}

pub struct RepoFilterAppExtension;

impl AppExtension for RepoFilterAppExtension {
    type Args = RepoFilterArgs;

    fn environment_hook(&self, args: &Self::Args, env: &mut MononokeEnvironment) -> Result<()> {
        env.filter_repos = args.filter_repos.clone();

        Ok(())
    }
}
