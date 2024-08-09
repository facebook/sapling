/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Display information about a commit, directory, or file.

use std::io::Write;

use anyhow::Error;
use anyhow::Result;
use cloned::cloned;
use scs_client_raw::thrift;
use serde::Serialize;

use crate::args::repo::RepoArgs;
use crate::library::stress_test::StressArgs;
use crate::library::summary::summary_output;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]
/// Fetch info about a commit, directory, file or bookmark
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    /// Enable stress test mode
    #[clap(flatten)]
    stress: Option<StressArgs>,
}

#[derive(Serialize)]
pub(crate) struct RepoInfo {
    pub r#type: String, // For JSON output, always "repo".
    pub name: String,
    pub default_commit_identity_scheme: String,
}

impl TryFrom<&thrift::RepoInfo> for RepoInfo {
    type Error = Error;

    fn try_from(repo: &thrift::RepoInfo) -> Result<RepoInfo, Error> {
        Ok(RepoInfo {
            r#type: "repo".to_string(),
            name: repo.name.clone(),
            default_commit_identity_scheme: repo.default_commit_identity_scheme.to_string(),
        })
    }
}

struct RepoInfoOutput {
    repo: RepoInfo,
}

impl Render for RepoInfoOutput {
    type Args = CommandArgs;

    fn render(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        write!(w, "Repo: {}\n", self.repo.name)?;
        write!(
            w,
            "Default commit identity scheme: {}\n",
            self.repo.default_commit_identity_scheme
        )?;
        Ok(())
    }

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, &self.repo)?)
    }
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();
    let conn = app.get_connection(Some(&repo.name))?;
    let params = thrift::RepoInfoParams {
        ..Default::default()
    };

    if let Some(stress) = args.stress {
        let runner = stress.new_runner(conn.get_client_corrrelator());
        let results = runner
            .run(Box::new(move || {
                cloned!(conn, repo, params);
                Box::pin(async move {
                    conn.repo_info(&repo, &params).await?;
                    Ok(())
                })
            }))
            .await;

        let output = summary_output(results);
        app.target.render(&(), output).await
    } else {
        let response = conn.repo_info(&repo, &params).await?;
        let repo_info = RepoInfo::try_from(&response)?;
        let output = RepoInfoOutput { repo: repo_info };
        app.target.render_one(&args, output).await
    }
}
