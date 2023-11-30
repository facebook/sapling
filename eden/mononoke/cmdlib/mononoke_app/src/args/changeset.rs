/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use clap::ArgGroup;
use clap::Args;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;

/// Command line arguments for specifying a changeset.
#[derive(Args, Debug)]
#[clap(group(
    ArgGroup::new("changeset")
        .required(true)
        .args(&["changeset_id", "hg_id", "bookmark", "all_bookmarks"]),
))]
pub struct ChangesetArgs {
    /// Bonsai changeset id
    #[clap(long, short = 'i')]
    changeset_id: Vec<ChangesetId>,

    /// Hg changeset id
    #[clap(long)]
    hg_id: Vec<HgChangesetId>,

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
        repo: &(impl BookmarksRef + BonsaiHgMappingRef),
    ) -> Result<Option<ChangesetId>> {
        self.resolve_changesets(ctx, repo)
            .await
            .and_then(|changesets| {
                if changesets.len() > 1 {
                    bail!("Only one changeset may be provided")
                } else {
                    Ok(changesets.into_iter().next())
                }
            })
    }
    pub async fn resolve_changesets(
        &self,
        ctx: &CoreContext,
        repo: &(impl BookmarksRef + BonsaiHgMappingRef),
    ) -> Result<Vec<ChangesetId>> {
        stream::iter(self.bookmark.iter())
            .then(|bookmark| async move {
                repo.bookmarks()
                    .get(ctx.clone(), bookmark)
                    .await
                    .with_context(|| format!("Failed to resolve bookmark '{}'", bookmark))?
                    .ok_or_else(|| anyhow!("Couldn't find bookmark: {}", bookmark))
            })
            .chain(stream::iter(self.hg_id.iter()).then(|hg_id| async move {
                repo.bonsai_hg_mapping()
                    .get_bonsai_from_hg(ctx, *hg_id)
                    .await
                    .with_context(|| format!("Failed to resolve hg changeset id {}", hg_id))?
                    .ok_or_else(|| anyhow!("Couldn't find hg id: {}", hg_id))
            }))
            .chain(
                stream::iter(self.changeset_id.iter())
                    .then(|changeset_id| async move { Ok(*changeset_id) }),
            )
            .chain(
                stream::iter(self.all_bookmarks.then(|| {
                    repo.bookmarks()
                        .list(
                            ctx.clone(),
                            Freshness::MostRecent,
                            &BookmarkPrefix::empty(),
                            BookmarkCategory::ALL,
                            BookmarkKind::ALL,
                            &BookmarkPagination::FromStart,
                            std::u64::MAX,
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
