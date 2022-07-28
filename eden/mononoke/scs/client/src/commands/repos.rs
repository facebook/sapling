/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! List repositories.

use std::io::Write;

use anyhow::Result;
use clap::Parser;
use serde::Serialize;
use source_control::types as thrift;

use crate::render::Render;
use crate::ScscApp;

#[derive(Parser)]
/// List repositories
pub(super) struct CommandArgs {}

#[derive(Serialize)]
struct ReposOutput {
    repos: Vec<String>,
}

impl Render for ReposOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        for repo in self.repos.iter() {
            write!(w, "{}\n", repo)?;
        }
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let params = thrift::ListReposParams {
        ..Default::default()
    };
    let repos = app.connection.list_repos(&params).await?;
    app.target
        .render_one(
            &args,
            ReposOutput {
                repos: repos.into_iter().map(|repo| repo.name).collect(),
            },
        )
        .await
}
