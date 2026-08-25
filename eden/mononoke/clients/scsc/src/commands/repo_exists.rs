/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Check whether a repository exists.

use std::io::Write;

use anyhow::Result;
use clap::Parser;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::ScscApp;
use crate::render::Render;

#[derive(Parser)]
/// Check whether a repository exists
pub(super) struct CommandArgs {
    /// Name of the repository to check
    #[clap(long, short)]
    repo: String,
}

#[derive(Serialize)]
struct RepoExistsOutput {
    exists: bool,
}

impl Render for RepoExistsOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        writeln!(w, "{}", self.exists)?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let params = thrift::RepoExistsParams {
        repo_name: args.repo.clone(),
        ..Default::default()
    };
    let conn = app.get_connection(None).await?;
    let response = conn.repo_exists(&params).await?;
    app.target
        .render_one(
            &args,
            RepoExistsOutput {
                exists: response.exists,
            },
        )
        .await
}
