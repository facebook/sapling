/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use github::get_parent_repo;
use github::GitHubRepo;
use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(
    about = "test github crate; GITHUB_API_TOKEN env var must be set",
    rename_all = "kebab-case"
)]
enum Subcommand {
    CheckParent {
        /// name of GitHub organization
        owner: String,

        /// name of repository within organization
        name: String,
    },
}

fn main() -> Result<()> {
    let github_api_token =
        std::env::var("GITHUB_API_TOKEN").expect("Missing GITHUB_API_TOKEN env var");

    match Subcommand::from_args() {
        Subcommand::CheckParent { owner, name } => is_fork(&github_api_token, &owner, &name),
    }
}

fn is_fork(github_api_token: &str, owner: &str, name: &str) -> Result<()> {
    let repo = GitHubRepo {
        owner: owner.to_string(),
        name: name.to_string(),
    };
    match get_parent_repo(github_api_token, &repo)? {
        Some(parent) => {
            println!(
                "{}/{} is a fork of {}/{}",
                owner, name, parent.owner, parent.name
            );
        }
        None => println!("{}/{} does not appear to be a fork", owner, name),
    }
    Ok(())
}
