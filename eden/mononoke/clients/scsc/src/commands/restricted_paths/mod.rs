/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Commands for querying restricted paths in a repository.

mod access;
mod changes;
mod find;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;

#[derive(Parser)]
/// Query restricted paths in a repository
pub(super) struct CommandArgs {
    #[command(subcommand)]
    subcommand: RestrictedPathsSubcommand,
}

#[derive(Subcommand)]
enum RestrictedPathsSubcommand {
    /// Check if specific paths are restricted and if the caller has access
    Access(access::AccessArgs),
    /// Find all restriction roots under specified paths
    Find(find::FindArgs),
    /// Check restrictions on all paths changed in a commit
    Changes(changes::ChangesArgs),
}

// Shared output types

#[derive(Serialize)]
pub(super) struct PathRestrictionInfo {
    pub path: String,
    pub roots: Vec<RestrictionRootInfo>,
}

#[derive(Serialize)]
pub(super) struct RestrictionRootInfo {
    pub root_path: String,
    pub acls: Vec<String>,
}

// Helper functions

pub(super) fn path_coverage_to_string(coverage: thrift::PathCoverage) -> String {
    match coverage {
        thrift::PathCoverage::NONE => "none".to_string(),
        thrift::PathCoverage::SOME => "some".to_string(),
        thrift::PathCoverage::ALL => "all".to_string(),
        _ => "unknown".to_string(),
    }
}

pub(super) fn convert_restriction_roots(
    roots_map: std::collections::BTreeMap<String, Vec<thrift::PathRestrictionRoot>>,
) -> Vec<PathRestrictionInfo> {
    roots_map
        .into_iter()
        .map(|(path, roots)| PathRestrictionInfo {
            path,
            roots: roots
                .into_iter()
                .map(|root| RestrictionRootInfo {
                    root_path: root.path,
                    acls: root.acls,
                })
                .collect(),
        })
        .collect()
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    match args.subcommand {
        RestrictedPathsSubcommand::Access(args) => access::run(app, args).await,
        RestrictedPathsSubcommand::Find(args) => find::run(app, args).await,
        RestrictedPathsSubcommand::Changes(args) => changes::run(app, args).await,
    }
}
