/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use environment::MononokeEnvironment;
use regex::Regex;

use crate::AppExtension;

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
        if let Some(filter_repos) = args.filter_repos.clone() {
            if let Some(current_filter_fn) = env.filter_repos.clone() {
                env.filter_repos = Some(Arc::new(move |name: &str| {
                    filter_repos.is_match(name) & current_filter_fn(name)
                }));
            } else {
                env.filter_repos = Some(Arc::new(move |name: &str| filter_repos.is_match(name)));
            }
        }

        Ok(())
    }
}
