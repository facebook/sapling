/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use serde::Serialize;
use source_control::types as thrift;
use std::io::Write;

use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdArgs;
use crate::args::repo::RepoArgs;
use crate::render::Render;
use crate::ScscApp;

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
    #[clap(long)]
    /// If provided (even empty), response is sorted and starts from the given name
    after: Option<String>,
    #[clap(long, default_value_t = 100)]
    /// Maximum number of paths to return
    limit: u64,
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
    let id = resolve_commit_id(&app.connection, &repo, &commit_id).await?;
    let prefixes = args.prefix.clone();
    let basenames = args.filename.clone();
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
        prefixes,
        ..Default::default()
    };
    let response = app
        .connection
        .commit_find_files(&commit_specifier, &params)
        .await?;
    app.target
        .render_one(&args, FileListOutput(response.files))
        .await
}
