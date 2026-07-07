/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Find the first (oldest) commit that introduced a path.

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use commit_id_types::CommitIdArgs;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::commit_id::SchemeArgs;
use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::path::PathArgs;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::library::commit_id::render_commit_id;
use crate::render::Render;

#[derive(Parser)]
/// Find the first (oldest) commit that introduced a path
///
/// This is the temporal mirror of the commit returned at the end of `scsc log
/// --path`: it returns the oldest commit in the path's history (for example, to
/// find the original author of a file) without having to fetch the entire
/// history.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,
    #[clap(flatten)]
    path_args: PathArgs,
    #[clap(long)]
    /// Track history across deletion i.e. if a path was deleted then added back,
    /// report the commit that originally introduced it rather than the one that
    /// re-added it
    history_across_deletions: bool,
}

#[derive(Serialize)]
struct PathFirstCommitOutput {
    #[serde(skip)]
    requested: String,
    exists: bool,
    ids: BTreeMap<String, String>,
}

impl Render for PathFirstCommitOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        if self.exists {
            let schemes = args.scheme_args.scheme_string_set();
            render_commit_id(None, "\n", &self.requested, &self.ids, &schemes, w)?;
            write!(w, "\n")?;
        } else {
            bail!("{} has no first commit", self.requested);
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let conn = app.get_connection(Some(&repo.name)).await?;
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id,
        ..Default::default()
    };
    let path = args.path_args.path.clone();
    let commit_and_path = thrift::CommitPathSpecifier {
        commit,
        path: path.clone(),
        ..Default::default()
    };
    let params = thrift::CommitPathFirstChangedParams {
        identity_schemes: args.scheme_args.clone().into_request_schemes(),
        follow_history_across_deletions: args.history_across_deletions,
        ..Default::default()
    };
    let response = conn
        .commit_path_first_changed(&commit_and_path, &params)
        .await
        .map_err(|e| e.handle_selection_error(&repo))?;

    let (exists, ids) = match response.first_commit {
        Some(first_commit) => (true, map_commit_ids(first_commit.values())),
        None => (false, BTreeMap::new()),
    };
    let output = PathFirstCommitOutput {
        requested: path,
        exists,
        ids,
    };
    app.target.render_one(&args, output).await
}
