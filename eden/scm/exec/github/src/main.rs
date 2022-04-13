/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use github::get_parent_repo;
use github::get_pull_request;
use github::GitHubRepo;
use github::PullRequest;
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

    PullRequest {
        /// name of GitHub organization
        owner: String,

        /// name of repository within organization
        name: String,

        /// number of the pull request
        number: u32,
    },
}

fn main() -> Result<()> {
    let github_api_token =
        std::env::var("GITHUB_API_TOKEN").expect("Missing GITHUB_API_TOKEN env var");

    match Subcommand::from_args() {
        Subcommand::CheckParent { owner, name } => is_fork(&github_api_token, &owner, &name),
        Subcommand::PullRequest {
            owner,
            name,
            number,
        } => get_pr(&github_api_token, &owner, &name, number),
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
        None => eprintln!("{}/{} does not appear to be a fork", owner, name),
    }
    Ok(())
}

fn get_pr(github_api_token: &str, owner: &str, name: &str, number: u32) -> Result<()> {
    let repo = GitHubRepo {
        owner: owner.to_string(),
        name: name.to_string(),
    };
    match get_pull_request(github_api_token, &repo, number)? {
        Some(pull_request) => {
            let PullRequest {
                url, title, body, ..
            } = pull_request;
            println!("{}", url);
            println!("Title: {}", title);
            println!("Body:\n{}", body);
            println!("base: {:?}", pull_request.base);
            println!("head: {:?}", pull_request.head);
            println!("is_draft? {}", pull_request.is_draft);
            println!("state: {:?}", pull_request.state);
            println!("closed? {}", pull_request.closed);
            println!("merged? {}", pull_request.merged);
            println!("review_decision: {:?}", pull_request.review_decision);
        }
        None => eprintln!("{}/{}/pull/{} did not resolve to a PR", owner, name, number),
    }
    Ok(())
}
