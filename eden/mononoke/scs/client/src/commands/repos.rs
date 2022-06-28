/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! List repositories.

use std::io::Write;

use anyhow::Error;
use clap::App;
use clap::AppSettings;
use clap::ArgMatches;
use clap::SubCommand;
use futures::stream;
use futures_util::stream::StreamExt;
use serde_derive::Serialize;
use source_control::types as thrift;

use crate::connection::Connection;
use crate::render::Render;
use crate::render::RenderStream;

pub(super) const NAME: &str = "repos";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(NAME)
        .about("List repositories")
        .setting(AppSettings::ColoredHelp)
}

#[derive(Serialize)]
struct ReposOutput {
    repos: Vec<String>,
}

impl Render for ReposOutput {
    fn render(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        for repo in self.repos.iter() {
            write!(w, "{}\n", repo)?;
        }
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(
    _matches: &ArgMatches<'_>,
    connection: Connection,
) -> Result<RenderStream, Error> {
    let params = thrift::ListReposParams {
        ..Default::default()
    };
    let repos = connection.list_repos(&params).await?;
    let output = Box::new(ReposOutput {
        repos: repos.into_iter().map(|repo| repo.name).collect(),
    });
    Ok(stream::once(async move { Ok(output as Box<dyn Render>) }).boxed())
}
