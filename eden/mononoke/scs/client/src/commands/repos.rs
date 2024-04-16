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
use cloned::cloned;
use serde::Serialize;
use source_control as thrift;

use crate::library::summary::run_stress;
use crate::library::summary::summary_output;
use crate::library::summary::StressArgs;
use crate::render::Render;
use crate::ScscApp;

#[derive(Parser)]
/// List repositories
pub(super) struct CommandArgs {
    /// Enable stress test mode
    #[clap(flatten)]
    stress: Option<StressArgs>,
}

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
    let conn = app.get_connection(None)?;

    if let Some(stress) = args.stress {
        let results = run_stress(stress.count, stress.parallel, || {
            cloned!(conn, params);
            Box::pin(async move {
                conn.list_repos(&params).await?;
                Ok(())
            })
        })
        .await;

        let output = summary_output(results);
        app.target.render(&(), output).await
    } else {
        let repos = conn.list_repos(&params).await?;
        app.target
            .render_one(
                &args,
                ReposOutput {
                    repos: repos.into_iter().map(|repo| repo.name).collect(),
                },
            )
            .await
    }
}
