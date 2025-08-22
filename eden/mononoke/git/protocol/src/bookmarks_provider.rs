/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::Freshness;
use context::CoreContext;
use futures::TryStreamExt;
use mononoke_types::ChangesetId;
use rustc_hash::FxHashMap;

use crate::REF_PREFIX;
use crate::Repo;
use crate::types::BonsaiBookmarks;
use crate::types::RefsSource;
use crate::types::RequestedRefs;

/// Get the bookmarks (branches, tags) and their corresponding commits
/// for the given repo based on the request parameters. If the request
/// specifies a predefined mapping of an existing or new bookmark to a
/// commit, include that in the output as well
pub(crate) async fn bookmarks(
    ctx: &CoreContext,
    repo: &impl Repo,
    requested_refs: &RequestedRefs,
    refs_source: RefsSource,
) -> Result<BonsaiBookmarks> {
    let mut bookmarks = list_bookmarks(ctx, repo, refs_source, &BookmarkPrefix::empty())
        .await?
        .into_iter()
        .filter_map(|(bookmark, (cs_id, _))| {
            let refs = requested_refs.clone();
            let name = bookmark.name().to_string();
            match refs {
                RequestedRefs::Included(refs) if refs.contains(&name) => Some((bookmark, cs_id)),
                RequestedRefs::IncludedWithPrefix(ref_prefixes) => {
                    let ref_name = format!("{}{}", REF_PREFIX, name);
                    if ref_prefixes
                        .iter()
                        .any(|ref_prefix| ref_name.starts_with(ref_prefix))
                    {
                        Some((bookmark, cs_id))
                    } else {
                        None
                    }
                }
                RequestedRefs::Excluded(refs) if !refs.contains(&name) => Some((bookmark, cs_id)),
                RequestedRefs::IncludedWithValue(refs) => {
                    refs.get(&name).map(|cs_id| (bookmark, cs_id.clone()))
                }
                _ => None,
            }
        })
        .collect::<FxHashMap<_, _>>();
    // In case the requested refs include specified refs with value and those refs are not
    // bookmarks known at the server, we need to manually include them in the output
    if let RequestedRefs::IncludedWithValue(ref_value_map) = requested_refs {
        for (ref_name, ref_value) in ref_value_map {
            bookmarks.insert(
                BookmarkKey::with_name(ref_name.as_str().try_into()?),
                ref_value.clone(),
            );
        }
    }
    Ok(BonsaiBookmarks::new(bookmarks))
}

/// Method for listing tags for the current repo based on specified freshness
pub(crate) async fn list_tags(
    ctx: &CoreContext,
    repo: &impl Repo,
    refs_source: RefsSource,
) -> Result<Vec<(BookmarkKey, (ChangesetId, BookmarkKind))>> {
    list_bookmarks(ctx, repo, refs_source, &BookmarkPrefix::empty()).await
}

/// Method for listing bookmarks for the current repo based on specified freshness
async fn list_bookmarks(
    ctx: &CoreContext,
    repo: &impl Repo,
    refs_source: RefsSource,
    bookmark_prefix: &BookmarkPrefix,
) -> Result<Vec<(BookmarkKey, (ChangesetId, BookmarkKind))>> {
    match refs_source {
        RefsSource::WarmBookmarksCache => {
            repo.bookmarks_cache()
                .list(
                    ctx,
                    bookmark_prefix,
                    &BookmarkPagination::FromStart,
                    None, // Limit
                )
                .await
        }
        RefsSource::DatabaseMaster => {
            repo.bookmarks()
                .list(
                    ctx.clone(),
                    Freshness::MostRecent,
                    bookmark_prefix,
                    BookmarkCategory::ALL,
                    BookmarkKind::ALL_PUBLISHING,
                    &BookmarkPagination::FromStart,
                    u64::MAX,
                )
                .map_ok(|(bookmark, cs_id)| (bookmark.key, (cs_id, bookmark.kind)))
                .try_collect::<Vec<_>>()
                .await
        }
        RefsSource::DatabaseFollower => {
            repo.bookmarks()
                .list(
                    ctx.clone(),
                    Freshness::MaybeStale,
                    bookmark_prefix,
                    BookmarkCategory::ALL,
                    BookmarkKind::ALL_PUBLISHING,
                    &BookmarkPagination::FromStart,
                    u64::MAX,
                )
                .map_ok(|(bookmark, cs_id)| (bookmark.key, (cs_id, bookmark.kind)))
                .try_collect::<Vec<_>>()
                .await
        }
    }
}

/// Function that waits for the WBC to move past the given bookmark value
pub async fn wait_for_bookmark_move(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark: &BookmarkKey,
    old_bookmark_val: Option<ChangesetId>,
) -> Result<()> {
    loop {
        let new_bookmark_val = repo.bookmarks_cache().get(ctx, bookmark).await?;
        match (old_bookmark_val, new_bookmark_val) {
            (Some(old), Some(new)) => {
                if old == new {
                    continue; // Retring the loop immediately is fine since this is in-memory access                    
                } else {
                    return Ok(());
                }
            }
            (Some(_), None) => {
                // The bookmark appears to be deleted and WBC caught up with that change
                return Ok(());
            }
            (None, Some(_)) => {
                // The bookmark appears to be created and WBC caught up with that change
                return Ok(());
            }
            (None, None) => {
                // The operation is a bookmark create which still hasn't happened as per WBC
                // Retry until it does.
                continue;
            }
        }
    }
}
