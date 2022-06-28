/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use anyhow::Error;
use anyhow::Result;
use bookmarks::Bookmark;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use clap::Args;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;

use super::Repo;
use crate::commit_id::IdentityScheme;

#[derive(Args)]
pub struct BookmarksListArgs {
    /// Kind of bookmarks to list
    #[clap(long = "kind", short = 'k', value_name = "KIND", arg_enum, default_values = &["publishing", "pull-default-publishing"])]
    kinds: Vec<BookmarkKind>,

    /// Prefix of bookmarks to list
    #[clap(long)]
    prefix: Option<BookmarkPrefix>,

    /// Show at most this number of bookmarks
    #[clap(long, short = 'l', default_value_t = 100)]
    limit: u64,

    /// Show bookmarks after this name (continue pagination)
    #[clap(long)]
    after: Option<BookmarkName>,

    /// Commit identity schemes to display
    #[clap(long, short='S', arg_enum, default_values = &["bonsai"], use_value_delimiter = true)]
    schemes: Vec<IdentityScheme>,

    /// Request most recent values of bookmarks (bypass caches and replicas)
    #[clap(long)]
    fresh: bool,
}

struct BookmarkValue {
    bookmark: Bookmark,
    ids: Vec<(IdentityScheme, String)>,
}

impl BookmarkValue {
    async fn new(
        ctx: &CoreContext,
        repo: &Repo,
        bookmark: Bookmark,
        changeset_id: ChangesetId,
        schemes: &[IdentityScheme],
    ) -> Result<Self> {
        let ids = stream::iter(schemes.iter().copied())
            .map(|scheme| {
                Ok::<_, Error>(async move {
                    match scheme.map_commit_id(ctx, repo, changeset_id).await? {
                        Some(commit_id) => Ok(Some((scheme, commit_id))),
                        None => Ok(None),
                    }
                })
            })
            .try_buffered(10)
            .try_filter_map(|commit_id| async move { Ok(commit_id) })
            .try_collect()
            .await?;
        Ok(BookmarkValue { bookmark, ids })
    }
}

impl fmt::Display for BookmarkValue {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self.ids.as_slice() {
            [] => {}
            [(_, id)] => write!(fmt, "{} ", id)?,
            ids => {
                for (scheme, id) in ids {
                    write!(fmt, "{}={} ", scheme.to_string(), id)?;
                }
            }
        }
        write!(fmt, "{}", self.bookmark.name())?;
        Ok(())
    }
}

pub async fn list(ctx: &CoreContext, repo: &Repo, list_args: BookmarksListArgs) -> Result<()> {
    let freshness = if list_args.fresh {
        Freshness::MostRecent
    } else {
        Freshness::MaybeStale
    };
    let prefix = list_args.prefix.unwrap_or_else(BookmarkPrefix::empty);
    let pagination = list_args
        .after
        .map_or(BookmarkPagination::FromStart, BookmarkPagination::After);
    repo.bookmarks()
        .list(
            ctx.clone(),
            freshness,
            &prefix,
            &list_args.kinds,
            &pagination,
            list_args.limit,
        )
        .map_ok(|(bookmark, cs_id)| {
            BookmarkValue::new(ctx, repo, bookmark, cs_id, &list_args.schemes)
        })
        .try_buffered(100)
        .try_for_each(|value| async move {
            println!("{}", value);
            Ok(())
        })
        .await?;
    Ok(())
}
