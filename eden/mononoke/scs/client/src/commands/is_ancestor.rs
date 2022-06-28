/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Find common base of two commits

use std::io::Write;

use anyhow::bail;
use anyhow::Error;
use clap::App;
use clap::AppSettings;
use clap::ArgMatches;
use clap::SubCommand;
use futures::stream;
use futures_util::stream::StreamExt;
use serde_derive::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::add_multiple_commit_id_args;
use crate::args::commit_id::add_scheme_args;
use crate::args::commit_id::get_commit_ids;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::repo::add_repo_args;
use crate::args::repo::get_repo_specifier;
use crate::connection::Connection;
use crate::render::Render;
use crate::render::RenderStream;

pub(super) const NAME: &str = "is-ancestor";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Finds whether the first provided commit is an ancestor of the second one.")
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_scheme_args(cmd);
    add_multiple_commit_id_args(cmd)
}

#[derive(Serialize)]
pub struct IsAncestorOutput {
    #[serde(skip)]
    pub requested: (String, String),
    pub result: bool,
}

impl Render for IsAncestorOutput {
    fn render(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        writeln!(w, "{:?}", self.result)?;
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

pub(super) async fn run(
    matches: &ArgMatches<'_>,
    connection: Connection,
) -> Result<RenderStream, Error> {
    let repo = get_repo_specifier(matches).expect("repository is required");
    let commit_ids = get_commit_ids(matches)?;
    let ids = resolve_commit_ids(&connection, &repo, &commit_ids).await?;
    if ids.len() != 2 || ids.is_empty() {
        bail!("expected 1 or 2 commit_ids (got {})", commit_ids.len())
    }
    let commit = thrift::CommitSpecifier {
        repo,
        id: ids[0].clone(),
        ..Default::default()
    };
    let params = thrift::CommitIsAncestorOfParams {
        descendant_commit_id: ids[1].clone(),
        ..Default::default()
    };
    let response = connection.commit_is_ancestor_of(&commit, &params).await?;
    let output = Box::new(IsAncestorOutput {
        requested: (commit_ids[0].to_string(), commit_ids[1].to_string()),
        result: response,
    });
    Ok(stream::once(async move { Ok(output as Box<dyn Render>) }).boxed())
}
