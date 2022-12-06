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

#[macro_export]
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

        impl AsRepoArg for $ident {
            fn as_repo_arg(&self) -> &RepoArg {
                &self.0
            }
        }
    };
}

#[macro_export]
macro_rules! repo_args_optional {
    ($ident:ident, $name_arg:literal, $maybe_short_name_arg:expr, $name_help:literal, $id_arg:literal, $id_help:literal) => {
        #[derive(Debug)]
        pub struct $ident(Option<RepoArg>);

        impl Args for $ident {
            fn augment_args(cmd: Command) -> Command {
                augment_args(
                    cmd,
                    stringify!($ident),
                    false,
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
                    (Some(repo_id), None) => {
                        Ok(Self(Some(RepoArg::Id(RepositoryId::new(*repo_id)))))
                    }
                    (None, Some(repo_name)) => Ok(Self(Some(RepoArg::Name(repo_name.clone())))),
                    (Some(_), Some(_)) => unreachable!(),
                    (None, None) => Ok(Self(None)),
                }
            }
            fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), Error> {
                *self = Self::from_arg_matches(matches)?;
                Ok(())
            }
        }

        impl $ident {
            pub fn as_repo_arg(&self) -> &Option<RepoArg> {
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
    pub fn from_repo_name(repo_name: String) -> Self {
        Self(RepoArg::Name(repo_name))
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

repo_args!(
    SourceRepoArgs,
    "source-repo-name",
    None,
    "Source repository name",
    "source-repo-id",
    "Numeric source repository ID"
);

repo_args!(
    TargetRepoArgs,
    "target-repo-name",
    None,
    "Target repository name",
    "target-repo-id",
    "Numeric target repository ID"
);

/// Command line arguments for specifying only a source  and a target repos,
/// Necessary for cross-repo operations
/// Only visible if the app was built with a call to `MononokeAppBuilder::with_source_and_target_repos`
#[derive(Args, Debug)]
pub struct SourceAndTargetRepoArgs {
    #[clap(flatten)]
    pub source_repo: SourceRepoArgs,

    #[clap(flatten)]
    pub target_repo: TargetRepoArgs,
}

#[derive(Debug)]
pub enum RepoArg {
    Id(RepositoryId),
    Name(String),
}

pub trait AsRepoArg {
    fn as_repo_arg(&self) -> &RepoArg;
}

impl AsRepoArg for RepoArg {
    fn as_repo_arg(&self) -> &RepoArg {
        self
    }
}
