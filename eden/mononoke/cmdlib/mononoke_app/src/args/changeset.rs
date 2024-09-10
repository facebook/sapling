/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bonsai_svnrev_mapping::BonsaiSvnrevMappingRef;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use clap::ArgGroup;
use clap::Args;
use commit_id::parse_commit_id;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use itertools::Itertools;
use mononoke_types::ChangesetId;

pub trait Repo = BookmarksRef
    + BonsaiHgMappingRef
    + BonsaiGitMappingRef
    + BonsaiGlobalrevMappingRef
    + BonsaiSvnrevMappingRef;

/// Command line arguments for specifying a changeset.
#[derive(Args, Debug)]
#[clap(group(
    ArgGroup::new("changeset")
        .required(true)
        .args(&["commit_id", "bookmark", "all_bookmarks"]),
))]
pub struct ChangesetArgs {
    /// Commit Id
    #[clap(long, short = 'i')]
    commit_id: Vec<String>,

    /// Bookmark name
    #[clap(long, short = 'B')]
    bookmark: Vec<BookmarkKey>,

    /// All bookmarks
    #[clap(long)]
    all_bookmarks: bool,
}

impl ChangesetArgs {
    pub async fn resolve_changeset(
        &self,
        ctx: &CoreContext,
        repo: &impl Repo,
    ) -> Result<ChangesetId> {
        self.resolve_changesets(ctx, repo)
            .await
            .and_then(|changesets| {
                changesets
                    .into_iter()
                    .exactly_one()
                    .map_err(|_| anyhow!("Exactly one changeset must be provided"))
            })
    }
    pub async fn resolve_changesets(
        &self,
        ctx: &CoreContext,
        repo: &impl Repo,
    ) -> Result<Vec<ChangesetId>> {
        stream::iter(self.bookmark.iter())
            .then(|bookmark| async move {
                repo.bookmarks()
                    .get(ctx.clone(), bookmark)
                    .await
                    .with_context(|| format!("Failed to resolve bookmark '{}'", bookmark))?
                    .ok_or_else(|| anyhow!("Couldn't find bookmark: {}", bookmark))
            })
            .chain(
                stream::iter(self.commit_id.iter()).then(|commit_id| async move {
                    parse_commit_id(ctx, repo, commit_id)
                        .await
                        .with_context(|| format!("Failed to parse commit id '{}'", commit_id))
                }),
            )
            .chain(
                stream::iter(self.all_bookmarks.then(|| {
                    repo.bookmarks()
                        .list(
                            ctx.clone(),
                            Freshness::MostRecent,
                            &BookmarkPrefix::empty(),
                            BookmarkCategory::ALL,
                            BookmarkKind::ALL_PUBLISHING,
                            &BookmarkPagination::FromStart,
                            u64::MAX,
                        )
                        .map_ok(|(_name, cs_id)| cs_id)
                }))
                .flatten(),
            )
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect()
    }
}
