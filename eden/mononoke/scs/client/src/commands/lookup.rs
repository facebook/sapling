/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Look up a bookmark or commit id.

use std::collections::BTreeMap;
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

use crate::args::commit_id::add_commit_id_args;
use crate::args::commit_id::add_scheme_args;
use crate::args::commit_id::get_commit_id;
use crate::args::commit_id::get_request_schemes;
use crate::args::commit_id::get_schemes;
use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::repo::add_repo_args;
use crate::args::repo::get_repo_specifier;
use crate::connection::Connection;
use crate::lib::commit_id::render_commit_id;
use crate::render::Render;
use crate::render::RenderStream;

pub(super) const NAME: &str = "lookup";

#[allow(clippy::let_and_return)]
pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Look up a bookmark or commit id")
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_scheme_args(cmd);
    let cmd = add_commit_id_args(cmd);

    cmd
}

#[derive(Serialize)]
pub struct LookupOutput {
    #[serde(skip)]
    pub requested: String,
    pub exists: bool,
    pub ids: BTreeMap<String, String>,
}

impl Render for LookupOutput {
    fn render(&self, matches: &ArgMatches, w: &mut dyn Write) -> Result<(), Error> {
        if self.exists {
            let schemes = get_schemes(matches);
            render_commit_id(None, "\n", &self.requested, &self.ids, &schemes, w)?;
            write!(w, "\n")?;
        } else {
            bail!("{} does not exist\n", self.requested);
        }
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
    let commit_id = get_commit_id(matches)?;
    let id = resolve_commit_id(&connection, &repo, &commit_id).await?;
    let commit = thrift::CommitSpecifier {
        repo,
        id,
        ..Default::default()
    };
    let params = thrift::CommitLookupParams {
        identity_schemes: get_request_schemes(matches),
        ..Default::default()
    };
    let response = connection.commit_lookup(&commit, &params).await?;
    let ids = match &response.ids {
        Some(ids) => map_commit_ids(ids.values()),
        None => BTreeMap::new(),
    };
    let output = Box::new(LookupOutput {
        requested: commit_id.to_string(),
        exists: response.exists,
        ids,
    });
    Ok(stream::once(async move { Ok(output as Box<dyn Render>) }).boxed())
}
