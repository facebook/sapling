/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::App;
use clap::AppSettings;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use futures::stream;
use futures::stream::StreamExt;
use source_control::types as thrift;

use crate::args::commit_id::add_commit_id_args;
use crate::args::commit_id::get_commit_id;
use crate::args::commit_id::resolve_commit_id;
use crate::args::pushvars::add_pushvar_args;
use crate::args::pushvars::get_pushvars;
use crate::args::repo::add_repo_args;
use crate::args::repo::get_repo_specifier;
use crate::args::service_id::add_service_id_args;
use crate::args::service_id::get_service_id;
use crate::connection::Connection;
use crate::render::RenderStream;

pub(super) const NAME: &str = "create-bookmark";

const ARG_NAME: &str = "BOOKMARK_NAME";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Create a bookmark")
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_commit_id_args(cmd);
    let cmd = add_service_id_args(cmd);
    let cmd = add_pushvar_args(cmd);
    cmd.arg(
        Arg::with_name(ARG_NAME)
            .short("n")
            .long("name")
            .takes_value(true)
            .help("Name of the bookmark to create")
            .required(true),
    )
}

pub(super) async fn run(matches: &ArgMatches<'_>, connection: Connection) -> Result<RenderStream> {
    let repo = get_repo_specifier(matches).expect("repository is required");
    let commit_id = get_commit_id(matches)?;
    let id = resolve_commit_id(&connection, &repo, &commit_id).await?;
    let bookmark = matches.value_of(ARG_NAME).expect("name is required").into();
    let service_identity = get_service_id(matches).map(String::from);
    let pushvars = get_pushvars(&matches)?;

    let params = thrift::RepoCreateBookmarkParams {
        bookmark,
        target: id,
        service_identity,
        pushvars,
        ..Default::default()
    };
    connection.repo_create_bookmark(&repo, &params).await?;
    Ok(stream::empty().boxed())
}
