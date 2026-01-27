/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Check if specific paths are restricted and if the caller has access.

use std::io::Write;

use anyhow::Result;
use clap::Parser;
use commit_id_types::CommitIdArgs;
use scs_client_raw::thrift;
use serde::Serialize;

use super::PathRestrictionInfo;
use super::convert_restriction_roots;
use super::path_coverage_to_string;
use crate::ScscApp;
use crate::args::commit_id::resolve_commit_id;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::render::Render;

/// Query paths for restriction status and access permissions.
/// Returns which paths are restricted, the ACLs protecting them,
/// and which paths the caller has authorization to access if `--check-permissions` enabled.
#[derive(Parser)]
pub(super) struct AccessArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(long)]
    /// Check access permissions (populates authorized_paths field)
    check_permissions: bool,
    /// Paths to check for restrictions
    #[clap(short, long, required = true)]
    paths: Vec<String>,
}

#[derive(Serialize)]
struct AccessOutput {
    are_restricted: String,
    has_access: String,
    restriction_roots: Vec<PathRestrictionInfo>,
    authorized_paths: Vec<String>,
}

impl Render for AccessOutput {
    type Args = ();

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        writeln!(w, "Restricted: {}", self.are_restricted)?;
        writeln!(w, "Has access: {}", self.has_access)?;

        if !self.restriction_roots.is_empty() {
            writeln!(w, "\nRestriction roots:")?;
            for info in &self.restriction_roots {
                writeln!(w, "  {}:", info.path)?;
                for root in &info.roots {
                    writeln!(w, "    {} (ACLs: {})", root.root_path, root.acls.join(", "))?;
                }
            }
        }

        if !self.authorized_paths.is_empty() {
            writeln!(w, "\nAuthorized paths:")?;
            for path in &self.authorized_paths {
                writeln!(w, "  {}", path)?;
            }
        }

        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: AccessArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let conn = app.get_connection(Some(&repo.name)).await?;
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;

    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    };

    let params = thrift::CommitRestrictedPathsAccessParams {
        paths: args.paths.into_iter().collect(),
        check_permissions: args.check_permissions,
        ..Default::default()
    };

    let response = conn
        .commit_restricted_paths_access(&commit, &params)
        .await
        .map_err(|e| e.handle_selection_error(&repo))?;

    let output = AccessOutput {
        are_restricted: path_coverage_to_string(response.are_restricted),
        has_access: path_coverage_to_string(response.has_access),
        restriction_roots: convert_restriction_roots(response.restriction_roots),
        authorized_paths: response.authorized_paths,
    };

    app.target.render_one(&(), output).await
}
