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
    pub fn from_repo_id(repo_id: i32) -> Self {
        RepoArgs {
            repo_id: Some(repo_id),
            repo_name: None,
        }
    }

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

/// Command line arguments for specifying only a source  and a target repos,
/// Necessary for cross-repo operations
/// Only visible if the app was built with a call to `MononokeAppBuilder::with_source_and_target_repos`
#[derive(Args, Debug)]
#[clap(group(
    ArgGroup::new("source-repo")
        .required(true)
        .args(&["source-repo-id", "source-repo-name"]),
))]
#[clap(group(
            ArgGroup::new("target-repo")
        .required(true)
        .args(&["target-repo-id", "target-repo-name"]),
))]
pub struct SourceAndTargetRepoArgs {
    /// Numeric ID of source repository (used only for commands that operate on more than one repo)
    #[clap(long)]
    source_repo_id: Option<i32>,

    /// Name of source repository (used only for commands that operate on more than one repo)
    #[clap(long)]
    source_repo_name: Option<String>,

    /// Numeric ID of target repository (used only for commands that operate on more than one repo)
    #[clap(long)]
    target_repo_id: Option<i32>,

    /// Name of target repository (used only for commands that operate on more than one repo)
    #[clap(long)]
    target_repo_name: Option<String>,
}

impl SourceAndTargetRepoArgs {
    pub fn source_and_target_id_or_name(&self) -> Result<SourceAndTargetRepoArg> {
        let source_repo = match self {
            Self {
                source_repo_id: Some(source_repo_id),
                source_repo_name: None,
                ..
            } => Ok(RepoArg::Id(RepositoryId::new(*source_repo_id))),
            Self {
                source_repo_name: Some(source_repo_name),
                source_repo_id: None,
                ..
            } => Ok(RepoArg::Name(source_repo_name)),
            _ => Err(anyhow::anyhow!(
                "exactly one of source-repo-id and source-repo-name must be specified"
            )),
        }?;
        let target_repo = match self {
            Self {
                target_repo_id: Some(target_repo_id),
                target_repo_name: None,
                ..
            } => Ok(RepoArg::Id(RepositoryId::new(*target_repo_id))),
            Self {
                target_repo_name: Some(target_repo_name),
                target_repo_id: None,
                ..
            } => Ok(RepoArg::Name(target_repo_name)),
            _ => Err(anyhow::anyhow!(
                "exactly one of target-repo-id and target-repo-name must be specified"
            )),
        }?;
        Ok(SourceAndTargetRepoArg {
            source_repo,
            target_repo,
        })
    }
}

pub struct SourceAndTargetRepoArg<'name> {
    pub source_repo: RepoArg<'name>,
    pub target_repo: RepoArg<'name>,
}

pub enum RepoArg<'name> {
    Id(RepositoryId),
    Name(&'name str),
}
