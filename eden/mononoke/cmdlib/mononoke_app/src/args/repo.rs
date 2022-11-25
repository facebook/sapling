/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::value_parser;
use clap::Arg;
use clap::ArgGroup;
use clap::ArgMatches;
use clap::Args;
use clap::Command;
use clap::Error;
use clap::FromArgMatches;
use mononoke_types::RepositoryId;

/// Command line arguments for specifying a single repo.

// For convenience first we define macro for generating appropiate RepoArgs
// structure that can be used with clap derive structs. Each struct adds args
// for providing repo using id or name.
fn augment_args<'a>(
    cmd: Command<'a>,
    ident: &'static str,
    required: bool,
    name_arg: &'static str,
    name_arg_short: Option<char>,
    name_help: &'static str,
    id_arg: &'static str,
    id_help: &'static str,
) -> Command<'a> {
    cmd.arg(
        Arg::new(id_arg)
            .long(id_arg)
            .value_parser(value_parser!(i32))
            .value_name("REPO_ID")
            .help(id_help),
    )
    .arg({
        let mut arg = Arg::new(name_arg)
            .long(name_arg)
            .value_name("REPO_NAME")
            .help(name_help);
        if let Some(short) = name_arg_short {
            arg = arg.short(short);
        }
        arg
    })
    .group(
        ArgGroup::new(ident)
            .required(required)
            .args(&[id_arg, name_arg]),
    )
}

macro_rules! repo_args {
    ($ident:ident, $name_arg:literal, $maybe_short_name_arg:expr, $name_help:literal, $id_arg:literal, $id_help:literal) => {
        #[derive(Debug)]
        pub struct $ident(RepoArg);

        impl Args for $ident {
            fn augment_args(cmd: Command) -> Command {
                augment_args(
                    cmd,
                    stringify!($ident),
                    true,
                    $name_arg,
                    $maybe_short_name_arg,
                    $name_help,
                    $id_arg,
                    $id_help,
                )
            }
            fn augment_args_for_update(cmd: Command) -> Command {
                Self::augment_args(cmd)
            }
        }

        impl FromArgMatches for $ident {
            fn from_arg_matches(matches: &ArgMatches) -> Result<Self, Error> {
                let repo_id = matches.get_one($id_arg);
                let repo_name: Option<&String> = matches.get_one($name_arg);
                match (repo_id, repo_name) {
                    (Some(repo_id), None) => Ok(Self(RepoArg::Id(RepositoryId::new(*repo_id)))),
                    (None, Some(repo_name)) => Ok(Self(RepoArg::Name(repo_name.clone()))),
                    // This case should never happen - arg grouping in clap will error first.
                    _ => {
                        unreachable!("exactly one of repo-id and repo-name must be specified");
                    }
                }
            }

            fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), Error> {
                *self = Self::from_arg_matches(matches)?;
                Ok(())
            }
        }

        impl $ident {
            pub fn id_or_name(&self) -> &RepoArg {
                &self.0
            }
        }
    };
}

repo_args!(
    RepoArgs,
    "repo-name",
    Some('R'),
    "Repository name",
    "repo-id",
    "Numeric repository ID"
);

impl RepoArgs {
    pub fn from_repo_id(repo_id: i32) -> Self {
        Self(RepoArg::Id(RepositoryId::new(repo_id)))
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
            l.push(RepoArg::Name(name.to_owned()));
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
            } => Ok(RepoArg::Name(source_repo_name.clone())),
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
            } => Ok(RepoArg::Name(target_repo_name.clone())),
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

pub struct SourceAndTargetRepoArg {
    pub source_repo: RepoArg,
    pub target_repo: RepoArg,
}

#[derive(Debug)]
pub enum RepoArg {
    Id(RepositoryId),
    Name(String),
}
