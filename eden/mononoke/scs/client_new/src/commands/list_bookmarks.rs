/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::io::Write;

use anyhow::Result;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use serde::Serialize;
use source_control::types as thrift;

use crate::args::commit_id::map_commit_ids;
use crate::args::commit_id::resolve_commit_id;
use crate::args::commit_id::CommitIdsArgs;
use crate::args::commit_id::SchemeArgs;
use crate::args::repo::RepoArgs;
use crate::connection::Connection;
use crate::lib::commit_id::render_commit_id;
use crate::render::Render;
use crate::ScscApp;

#[derive(clap::Parser)]
/// List bookmarks and their current commits
///
/// If a commit id is provided, the results are limited to descendants
/// of that commit.
pub(super) struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,
    #[clap(flatten)]
    scheme_args: SchemeArgs,
    #[clap(flatten)]
    commit_ids_args: CommitIdsArgs,
    #[clap(long)]
    /// Limit bookmarks to those starting with a certain prefix
    prefix: Option<String>,
    #[clap(long, short, default_value_t = 100)]
    /// Limit the number of bookmarks
    limit: usize,
    #[clap(long)]
    /// Only show bookmarks after the provided name
    after: Option<String>,
    #[clap(long, short)]
    /// Only show the bookmark names
    name_only: bool,
    #[clap(long)]
    /// Include scratch bookmarks in results
    include_scratch: bool,
}

#[derive(Serialize)]
struct BookmarkOutput {
    name: String,
    ids: BTreeMap<String, String>,
}

impl Render for BookmarkOutput {
    type Args = CommandArgs;

    fn render(&self, args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        let name_only = args.name_only;
        if name_only {
            write!(w, "{}\n", self.name)?;
        } else {
            let schemes: HashSet<String> = args.scheme_args.scheme_string_set();
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

    fn render_json(&self, _args: &Self::Args, w: &mut dyn Write) -> Result<()> {
        Ok(serde_json::to_writer(w, self)?)
    }
}

fn repo_list_bookmarks(
    connection: Connection,
    repo: thrift::RepoSpecifier,
    limit: Option<i64>,
    after: Option<String>,
    prefix: Option<String>,
    include_scratch: bool,
    identity_schemes: BTreeSet<thrift::CommitIdentityScheme>,
) -> impl Stream<Item = Result<(String, BTreeMap<String, String>)>> {
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

                anyhow::Ok(Some((stream::iter(bookmarks), next_state)))
            } else {
                anyhow::Ok(None)
            }
        }
    })
    .try_flatten()
}

fn commit_list_descendant_bookmarks(
    connection: Connection,
    commit: thrift::CommitSpecifier,
    limit: Option<i64>,
    after: Option<String>,
    prefix: Option<String>,
    include_scratch: bool,
    identity_schemes: BTreeSet<thrift::CommitIdentityScheme>,
) -> impl Stream<Item = Result<(String, BTreeMap<String, String>)>> {
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
                anyhow::Ok(Some((stream::iter(bookmarks), next_state)))
            } else {
                anyhow::Ok(None)
            }
        }
    })
    .try_flatten()
}

pub(super) async fn run(app: ScscApp, args: CommandArgs) -> Result<()> {
    let repo = args.repo_args.clone().into_repo_specifier();

    let limit: i64 = args.limit.try_into()?;

    let after = args.after.clone();
    let prefix = args.prefix.clone();
    let include_scratch = args.include_scratch;

    let bookmarks = match args.commit_ids_args.clone().into_commit_ids().as_slice() {
        [ref commit_id] => {
            let id = resolve_commit_id(&app.connection, &repo, commit_id).await?;
            let commit = thrift::CommitSpecifier {
                repo,
                id,
                ..Default::default()
            };
            commit_list_descendant_bookmarks(
                app.connection.clone(),
                commit,
                Some(limit),
                after.map(String::from),
                prefix.map(String::from),
                include_scratch,
                args.scheme_args.clone().into_request_schemes(),
            )
            .left_stream()
        }
        [] => repo_list_bookmarks(
            app.connection.clone(),
            repo,
            Some(limit),
            after.map(String::from),
            prefix.map(String::from),
            include_scratch,
            args.scheme_args.clone().into_request_schemes(),
        )
        .right_stream(),
        _ => anyhow::bail!("At most one commit must be specified"),
    };
    app.target
        .render(
            &args,
            bookmarks.map_ok(|(name, ids)| BookmarkOutput { name, ids }),
        )
        .await
}
