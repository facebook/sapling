/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use bookmarks::BookmarkKey;
use context::CoreContext;
use futures::Stream;
use mononoke_types::ChangesetId;

use crate::types::MetadataItem;
use crate::Repo;

pub(crate) async fn process_bookmark<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    bookmark: &BookmarkKey,
) -> Result<impl Stream<Item = Result<MetadataItem>> + 'a> {
    let cs_id = repo
        .bookmarks()
        .get(ctx.clone(), bookmark)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "Bookmark {} not found for repo {}",
                bookmark,
                repo.repo_identity().name()
            )
        })?;

    process_changeset(ctx, repo, cs_id).await
}

async fn process_changeset<'a>(
    _ctx: &'a CoreContext,
    _repo: &'a impl Repo,
    _cs_id: ChangesetId,
) -> Result<impl Stream<Item = Result<MetadataItem>> + 'a> {
    Ok(futures::stream::once(async { Ok(MetadataItem::Unknown) }))
}
