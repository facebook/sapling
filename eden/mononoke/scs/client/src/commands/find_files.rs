/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use commit_id_types::CommitIdArgs;
use futures::TryStreamExt;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::args::commit_id::resolve_commit_id;
use crate::args::repo::RepoArgs;
use crate::errors::SelectionErrorExt;
use crate::render::Render;

#[derive(Parser)]
/// Find all files inside a certain dir or with certain filename
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    commit_id_args: CommitIdArgs,

    #[clap(long, short)]
    /// Subdir to look at
    prefix: Option<Vec<String>>,
    #[clap(long, short)]
    /// Filename to filter on
    filename: Option<Vec<String>>,
    #[clap(long, short)]
    /// Suffix of the basenames to filter on,
    suffix: Option<Vec<String>>,
    #[clap(long)]
    /// If provided (even empty), response is sorted and starts from the given name
    after: Option<String>,
    #[clap(long, default_value_t = 100)]
    /// Maximum number of paths to return
    limit: u64,
    #[clap(long)]
    /// EXPERIMENTAL: stream the output from the server rather than obtaining it in one go
    stream: bool,
}

#[derive(Serialize)]
struct FileListOutput(Vec<String>);

impl Render for FileListOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        for file in &self.0 {
            write!(w, "{}\n", file)?;
        }
        Ok(())
    }
    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let commit_id = args.commit_id_args.clone().into_commit_id();
    let conn = app.get_connection(Some(&repo.name))?;
    let id = resolve_commit_id(&conn, &repo, &commit_id).await?;
    let prefixes = args.prefix.clone();
    let basenames = args.filename.clone();
    let basename_suffixes = args.suffix.clone();
    let after = args.after.clone();
    let limit: i64 = args.limit.try_into().context("limit too large")?;

    let commit_specifier = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let params = thrift::CommitFindFilesParams {
        limit,
        after,
        basenames,
        basename_suffixes,
        prefixes,
        ..Default::default()
    };

    if args.stream {
        let (_initial_response, response_stream) = conn
            .commit_find_files_stream(&commit_specifier, &params)
            .await
            .map_err(|e| e.handle_selection_error(&commit_specifier.repo))?;

        let response = response_stream
            .map_ok(|entry| FileListOutput(entry.files))
            .map_err(Into::into);
        app.target.render(&args, response).await
    } else {
        let response = conn
            .commit_find_files(&commit_specifier, &params)
            .await
            .map_err(|e| e.handle_selection_error(&commit_specifier.repo))?;
        app.target
            .render_one(&args, FileListOutput(response.files))
            .await
    }
}
