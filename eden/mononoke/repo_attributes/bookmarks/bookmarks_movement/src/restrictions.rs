/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkUpdateReason;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use metaconfig_types::RepoConfigRef;
use mononoke_types::ChangesetId;
use repo_authorization::AuthorizationContext;
use repo_cross_repo::RepoCrossRepoRef;
use repo_identity::RepoIdentityRef;

use crate::BookmarkMovementError;
use crate::Repo;

pub(crate) fn should_run_hooks(authz: &AuthorizationContext, reason: BookmarkUpdateReason) -> bool {
    if authz.is_service() {
        let enable_hooks_on_service_pushrebase = justknobs::eval(
            "scm/mononoke:enable_hooks_on_service_pushrebase",
            None,
            None,
        )
        .unwrap_or_default();
        reason == BookmarkUpdateReason::Pushrebase && enable_hooks_on_service_pushrebase
    } else {
        true
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BookmarkKindRestrictions {
    AnyKind,
    OnlyScratch,
    OnlyPublishing,
}

impl BookmarkKindRestrictions {
    pub(crate) fn check_kind(
        &self,
        repo: &impl RepoConfigRef,
        name: &BookmarkKey,
    ) -> Result<BookmarkKind, BookmarkMovementError> {
        let infinitepush_params = &repo.repo_config().infinitepush;
        match (self, &infinitepush_params.namespace) {
            (Self::OnlyScratch, None) => Err(BookmarkMovementError::ScratchBookmarksDisabled {
                bookmark: name.clone(),
            }),
            (Self::OnlyScratch, Some(namespace)) if !namespace.matches_bookmark(name) => {
                Err(BookmarkMovementError::InvalidScratchBookmark {
                    bookmark: name.clone(),
                    pattern: namespace.as_str().to_string(),
                })
            }
            (Self::OnlyPublishing, Some(namespace)) if namespace.matches_bookmark(name) => {
                Err(BookmarkMovementError::InvalidPublishingBookmark {
                    bookmark: name.clone(),
                    pattern: namespace.as_str().to_string(),
                })
            }
            (_, Some(namespace)) if namespace.matches_bookmark(name) => Ok(BookmarkKind::Scratch),
            (_, _) => Ok(BookmarkKind::Publishing),
        }
    }
}

pub(crate) async fn check_restriction_ensure_ancestor_of(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark_to_move: &BookmarkKey,
    target: ChangesetId,
) -> Result<(), BookmarkMovementError> {
    // NOTE: Obviously this is a little racy, but the bookmark could move after we check, so it
    // doesn't matter.

    let mut descendant_bookmarks = vec![];
    for attr in repo.repo_bookmark_attrs().select(bookmark_to_move) {
        if let Some(descendant_bookmark) = &attr.params().ensure_ancestor_of {
            descendant_bookmarks.push(descendant_bookmark);
        }
    }

    if let Some(config) = &repo.repo_config().pushrebase.globalrev_config {
        if config.globalrevs_small_repo_id.is_none() {
            // On large repo, it's fine not to be descendant of the bookmark.
            descendant_bookmarks.push(&config.publishing_bookmark);
        }
    }

    stream::iter(descendant_bookmarks)
        .map(Ok)
        .try_for_each_concurrent(10, |descendant_bookmark| async move {
            let is_ancestor =
                ensure_ancestor_of(ctx, repo, bookmark_to_move, descendant_bookmark, target)
                    .await?;
            if !is_ancestor {
                let e = BookmarkMovementError::RequiresAncestorOf {
                    bookmark: bookmark_to_move.clone(),
                    descendant_bookmark: descendant_bookmark.clone(),
                };
                return Err(e);
            }
            Ok(())
        })
        .await?;

    Ok(())
}

pub(crate) async fn ensure_ancestor_of(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark_to_move: &BookmarkKey,
    descendant_bookmark: &BookmarkKey,
    target: ChangesetId,
) -> Result<bool, BookmarkMovementError> {
    let descendant_cs_id = repo
        .bookmarks()
        .get(
            ctx.clone(),
            descendant_bookmark,
            bookmarks::Freshness::MostRecent,
        )
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Bookmark '{}' does not exist, but it should be a descendant of '{}'!",
                descendant_bookmark,
                bookmark_to_move
            )
        })?;

    Ok(repo
        .commit_graph()
        .is_ancestor(ctx, target, descendant_cs_id)
        .await?)
}

pub async fn check_bookmark_sync_config(
    ctx: &CoreContext,
    repo: &(impl RepoIdentityRef + RepoCrossRepoRef),
    bookmark: &BookmarkKey,
    kind: BookmarkKind,
) -> Result<(), BookmarkMovementError> {
    match kind {
        BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing => {
            if repo
                .repo_cross_repo()
                .live_commit_sync_config()
                .push_redirector_enabled_for_public(ctx, repo.repo_identity().id())
                .await?
            {
                return Err(BookmarkMovementError::PushRedirectorEnabledForPublishing {
                    bookmark: bookmark.clone(),
                });
            }
        }
        BookmarkKind::Scratch => {
            if repo
                .repo_cross_repo()
                .live_commit_sync_config()
                .push_redirector_enabled_for_draft(ctx, repo.repo_identity().id())
                .await?
            {
                return Err(BookmarkMovementError::PushRedirectorEnabledForScratch {
                    bookmark: bookmark.clone(),
                });
            }
        }
    }
    Ok(())
}
