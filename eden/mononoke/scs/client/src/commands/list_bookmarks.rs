/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::Write;

use anyhow::Error;
use anyhow::Result;
use clap::App;
use clap::AppSettings;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use serde_derive::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::add_optional_commit_id_args;
use crate::args::commit_id::add_scheme_args;
use crate::args::commit_id::get_commit_ids;
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

pub(super) const NAME: &str = "list-bookmarks";

const ARG_LIMIT: &str = "LIMIT";
const ARG_AFTER: &str = "AFTER";
const ARG_PREFIX: &str = "PREFIX";
const ARG_NAME_ONLY: &str = "NAME_ONLY";
const ARG_INCLUDE_SCRATCH: &str = "INCLUDE_SCRATCH";

const ARG_LIMIT_DEFAULT: &str = "100";

pub(super) fn make_subcommand<'a, 'b>() -> App<'a, 'b> {
    let cmd = SubCommand::with_name(NAME)
        .about("List bookmarks and their current commits")
        .long_about(concat!(
            "List bookmarks and their current commits\n\n",
            "If a commit id is provided, the results are limited to descendants ",
            "of that commit."
        ))
        .setting(AppSettings::ColoredHelp);
    let cmd = add_repo_args(cmd);
    let cmd = add_scheme_args(cmd);
    let cmd = add_optional_commit_id_args(cmd);
    let cmd = cmd
        .arg(
            Arg::with_name(ARG_PREFIX)
                .long("prefix")
                .takes_value(true)
                .help("Limit bookmarks to those starting with a certain prefix"),
        )
        .arg(
            Arg::with_name(ARG_LIMIT)
                .short("l")
                .long("limit")
                .takes_value(true)
                .default_value(ARG_LIMIT_DEFAULT)
                .help("Limit the number of bookmarks"),
        )
        .arg(
            Arg::with_name(ARG_AFTER)
                .long("after")
                .takes_value(true)
                .help("Only show bookmarks after the provided name"),
        )
        .arg(
            Arg::with_name(ARG_NAME_ONLY)
                .short("n")
                .long("name-only")
                .help("Only show the bookmark names"),
        )
        .arg(
            Arg::with_name(ARG_INCLUDE_SCRATCH)
                .long("include-scratch")
                .help("Include scratch bookmarks in results"),
        );
    cmd
}

#[derive(Serialize)]
struct BookmarkOutput {
    name: String,
    ids: BTreeMap<String, String>,
}

impl Render for BookmarkOutput {
    fn render(&self, matches: &ArgMatches, w: &mut dyn Write) -> Result<()> {
        let name_only = matches.is_present(ARG_NAME_ONLY);
        if name_only {
            write!(w, "{}\n", self.name)?;
        } else {
            let schemes = get_schemes(matches);
            if schemes.len() == 1 {
                write!(w, "{:<40} ", self.name)?;
                render_commit_id(None, "\n", &self.name, &self.ids, &schemes, w)?;
            } else {
                write!(w, "{}", self.name)?;
                render_commit_id(Some(("", "    ")), "\n", &self.name, &self.ids, &schemes, w)?;
            }
            write!(w, "\n")?;
        }
        Ok(())
    }

    fn render_json(&self, _matches: &ArgMatches, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

fn repo_list_bookmarks(
    connection: &Connection,
    repo: thrift::RepoSpecifier,
    limit: Option<i64>,
    after: Option<String>,
    prefix: Option<String>,
    include_scratch: bool,
    identity_schemes: BTreeSet<thrift::CommitIdentityScheme>,
) -> impl Stream<Item = Result<(String, BTreeMap<String, String>)>> {
    let connection = connection.clone();
    stream::try_unfold(Some((after, limit)), move |state| {
        let connection = connection.clone();
        let repo = repo.clone();
        let identity_schemes = identity_schemes.clone();
        let prefix = prefix.clone();
        async move {
            if let Some((after, limit)) = state {
                let (limit, remaining) = limit.map_or((0, None), |limit| {
                    let size = limit.min(source_control::consts::REPO_LIST_BOOKMARKS_MAX_LIMIT);
                    (size, Some(limit.saturating_sub(size)))
                });

                let params = thrift::RepoListBookmarksParams {
                    include_scratch,
                    bookmark_prefix: prefix.unwrap_or_default(),
                    limit,
                    after,
                    identity_schemes: identity_schemes.clone(),
                    ..Default::default()
                };
                let response = connection.repo_list_bookmarks(&repo, &params).await?;
                let bookmarks = response
                    .bookmarks
                    .into_iter()
                    .map(|(name, ids)| Ok((name, map_commit_ids(ids.values()))));
                let next_state = response
                    .continue_after
                    .map(|after| (Some(after), remaining))
                    .filter(|_| remaining.map_or(true, |r| r > 0));

                Ok(Some((stream::iter(bookmarks), next_state)))
            } else {
                Ok::<_, Error>(None)
            }
        }
    })
    .try_flatten()
}

fn commit_list_descendant_bookmarks(
    connection: &Connection,
    commit: thrift::CommitSpecifier,
    limit: Option<i64>,
    after: Option<String>,
    prefix: Option<String>,
    include_scratch: bool,
    identity_schemes: BTreeSet<thrift::CommitIdentityScheme>,
) -> impl Stream<Item = Result<(String, BTreeMap<String, String>)>> {
    let connection = connection.clone();
    stream::try_unfold(Some((after, limit)), move |state| {
        let connection = connection.clone();
        let commit = commit.clone();
        let identity_schemes = identity_schemes.clone();
        let prefix = prefix.clone();
        async move {
            if let Some((after, limit)) = state {
                if limit == Some(0) {
                    return Ok(None);
                }
                let params = thrift::CommitListDescendantBookmarksParams {
                    include_scratch,
                    bookmark_prefix: prefix.unwrap_or_default(),
                    limit: source_control::consts::COMMIT_LIST_DESCENDANT_BOOKMARKS_MAX_LIMIT,
                    after,
                    identity_schemes: identity_schemes.clone(),
                    ..Default::default()
                };
                let response = connection
                    .commit_list_descendant_bookmarks(&commit, &params)
                    .await?;
                let mut count = response.bookmarks.len() as i64;
                if let Some(limit) = limit {
                    count = count.min(limit);
                }
                let bookmarks = response
                    .bookmarks
                    .into_iter()
                    .take(count as usize)
                    .map(|(name, ids)| Ok((name, map_commit_ids(ids.values()))));
                let next_state = response
                    .continue_after
                    .map(|after| (Some(after), limit.map(|limit| limit.saturating_sub(count))));
                Ok(Some((stream::iter(bookmarks), next_state)))
            } else {
                Ok::<_, Error>(None)
            }
        }
    })
    .try_flatten()
}

pub(super) async fn run(matches: &ArgMatches<'_>, connection: Connection) -> Result<RenderStream> {
    let repo = get_repo_specifier(matches).expect("repository is required");

    let limit = matches
        .value_of(ARG_LIMIT)
        .map(str::parse::<i64>)
        .transpose()?;

    let after = matches.value_of(ARG_AFTER);
    let prefix = matches.value_of(ARG_PREFIX);
    let include_scratch = matches.is_present(ARG_INCLUDE_SCRATCH);

    let bookmarks = match get_commit_ids(matches)?.first() {
        Some(commit_id) => {
            let id = resolve_commit_id(&connection, &repo, commit_id).await?;
            let commit = thrift::CommitSpecifier {
                repo,
                id,
                ..Default::default()
            };
            commit_list_descendant_bookmarks(
                &connection,
                commit,
                limit,
                after.map(String::from),
                prefix.map(String::from),
                include_scratch,
                get_request_schemes(matches),
            )
            .left_stream()
        }
        None => repo_list_bookmarks(
            &connection,
            repo,
            limit,
            after.map(String::from),
            prefix.map(String::from),
            include_scratch,
            get_request_schemes(matches),
        )
        .right_stream(),
    };

    Ok(bookmarks
        .map_ok(|(name, ids)| Box::new(BookmarkOutput { name, ids }) as Box<dyn Render>)
        .boxed())
}
