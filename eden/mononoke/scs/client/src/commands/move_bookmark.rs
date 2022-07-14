/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use clap::App;
use clap::AppSettings;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use futures::stream;
use futures::stream::StreamExt;
use source_control::types as thrift;

use crate::args::commit_id::add_multiple_commit_id_args;
use crate::args::commit_id::get_commit_ids;
use crate::args::commit_id::resolve_commit_ids;
use crate::args::pushvars::add_pushvar_args;
use crate::args::pushvars::get_pushvars;
use crate::args::repo::add_repo_args;
use crate::args::repo::get_repo_specifier;
use crate::args::service_id::add_service_id_args;
use crate::args::service_id::get_service_id;
use crate::connection::Connection;
use crate::render::RenderStream;

pub(super) const NAME: &str = "move-bookmark";

const ARG_NAME: &str = "BOOKMARK_NAME";
const ARG_NON_FAST_FORWARD: &str = "NON_FAST_FORWARD";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("Move a bookmark")
        .long_about(concat!(
            "Move a bookmark\n\n",
            "If two commits are provided, then move the bookmark from the first commit ",
            "to the second commit, failing if the bookmark didn't previously point at ",
            "the first commit.",
        ))
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_multiple_commit_id_args(cmd);
    let cmd = add_service_id_args(cmd);
    let cmd = add_pushvar_args(cmd);
    cmd.arg(
        Arg::with_name(ARG_NAME)
            .short("n")
            .long("name")
            .takes_value(true)
            .help("Name of the bookmark to move")
            .required(true),
    )
    .arg(
        Arg::with_name(ARG_NON_FAST_FORWARD)
            .long("allow-non-fast-forward-move")
            .help("Allow non-fast-forward moves (if permitted for this bookmark)"),
    )
}

pub(super) async fn run(matches: &ArgMatches<'_>, connection: Connection) -> Result<RenderStream> {
    let repo = get_repo_specifier(matches).expect("repository is required");
    let commit_ids = get_commit_ids(matches)?;
    if commit_ids.len() != 1 && commit_ids.len() != 2 {
        bail!("expected 1 or 2 commit_ids (got {})", commit_ids.len())
    }
    let ids = resolve_commit_ids(&connection, &repo, &commit_ids).await?;
    let bookmark = matches.value_of(ARG_NAME).expect("name is required").into();
    let service_identity = get_service_id(matches).map(String::from);

    let (old_target, target) = match ids.as_slice() {
        [id] => (None, id.clone()),
        [old_id, new_id] => (Some(old_id.clone()), new_id.clone()),
        _ => bail!("expected 1 or 2 commit_ids (got {})", ids.len()),
    };
    let allow_non_fast_forward_move = matches.is_present(ARG_NON_FAST_FORWARD);
    let pushvars = get_pushvars(matches)?;

    let params = thrift::RepoMoveBookmarkParams {
        bookmark,
        target,
        old_target,
        service_identity,
        allow_non_fast_forward_move,
        pushvars,
        ..Default::default()
    };
    connection.repo_move_bookmark(&repo, &params).await?;
    Ok(stream::empty().boxed())
}
