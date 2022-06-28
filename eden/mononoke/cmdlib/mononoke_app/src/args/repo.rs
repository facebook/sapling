/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::ArgGroup;
use clap::Args;
use mononoke_types::RepositoryId;

/// Command line arguments for specifying a single repo.
#[derive(Args, Debug)]
#[clap(group(
    ArgGroup::new("repo")
        .required(true)
        .args(&["repo-id", "repo-name"]),
))]
pub struct RepoArgs {
    /// Numeric repository ID
    #[clap(long)]
    repo_id: Option<i32>,

    /// Repository name
    #[clap(short = 'R', long)]
    repo_name: Option<String>,
}

impl RepoArgs {
    pub fn id_or_name(&self) -> Result<RepoArg> {
        match self {
            RepoArgs {
                repo_id: Some(repo_id),
                repo_name: None,
            } => Ok(RepoArg::Id(RepositoryId::new(*repo_id))),
            RepoArgs {
                repo_name: Some(repo_name),
                repo_id: None,
            } => Ok(RepoArg::Name(repo_name)),
            _ => Err(anyhow::anyhow!(
                "exactly one of repo-id and repo-name must be specified"
            )),
        }
    }
}

/// Command line arguments for specifying multiple repos.
#[derive(Args, Debug)]
#[clap(group(
    ArgGroup::new("multirepos")
        .multiple(true)
        .conflicts_with("repo")
        .args(&["repo-id", "repo-name"]),
))]
pub struct MultiRepoArgs {
    /// Numeric repository ID
    #[clap(long)]
    pub repo_id: Vec<i32>,

    /// Repository name
    #[clap(short = 'R', long)]
    pub repo_name: Vec<String>,
}

impl MultiRepoArgs {
    pub fn ids_or_names(&self) -> Result<Vec<RepoArg>> {
        let mut l = vec![];
        for id in &self.repo_id {
            l.push(RepoArg::Id(RepositoryId::new(*id)));
        }
        for name in &self.repo_name {
            l.push(RepoArg::Name(name));
        }

        Ok(l)
    }
}

pub enum RepoArg<'name> {
    Id(RepositoryId),
    Name(&'name str),
}
