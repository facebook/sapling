/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use context::CoreContext;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use hooks::PushAuthoredBy;
use metaconfig_types::BookmarkAttrs;
use metaconfig_types::InfinitepushParams;
use metaconfig_types::PushrebaseParams;
use metaconfig_types::SourceControlServiceParams;
use mononoke_types::ChangesetId;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_cross_repo::RepoCrossRepoRef;
use repo_identity::RepoIdentityRef;
use tunables::tunables;

use crate::BookmarkMovementError;
use crate::Repo;

/// How authorization for the bookmark move should be determined.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BookmarkMoveAuthorization<'params> {
    /// The bookmark move has been initiated by a user. The user's identity in
    /// the core context should be used to check permission, and hooks must be
    /// run.
    User,

    /// The movement is on behalf of an authenticated service.
    ///
    /// repo_client doesn't have SourceControlServiceParams to hand, so until the
    /// repo attributes refactor is complete, we must store the params here.
    Service(String, &'params SourceControlServiceParams),
}

impl<'params> BookmarkMoveAuthorization<'params> {
    pub(crate) async fn check_authorized(
        &'params self,
        ctx: &CoreContext,
        bookmark_attrs: &BookmarkAttrs,
        bookmark: &BookmarkName,
    ) -> Result<(), BookmarkMovementError> {
        match self {
            BookmarkMoveAuthorization::User => {
                // If user is missing, fallback to "svcscm" which is the catch-all
                // user for service identities etc.
                let user = ctx.metadata().unix_name().unwrap_or("svcscm");

                // TODO: clean up `is_allowed_user` to avoid this clone.
                if !bookmark_attrs
                    .is_allowed_user(&user, ctx.metadata(), bookmark)
                    .await?
                {
                    return Err(BookmarkMovementError::PermissionDeniedUser {
                        user: user.to_string(),
                        bookmark: bookmark.clone(),
                    });
                }

                // TODO: Check using ctx.identities, and deny if neither are provided.
            }
            BookmarkMoveAuthorization::Service(service_name, scs_params) => {
                if !scs_params.service_write_bookmark_permitted(service_name, bookmark) {
                    return Err(BookmarkMovementError::PermissionDeniedServiceBookmark {
                        service_name: service_name.clone(),
                        bookmark: bookmark.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn should_run_hooks(&self, reason: BookmarkUpdateReason) -> bool {
        match self {
            BookmarkMoveAuthorization::User => true,
            BookmarkMoveAuthorization::Service(..) => {
                reason == BookmarkUpdateReason::Pushrebase
                    && tunables().get_enable_hooks_on_service_pushrebase()
            }
        }
    }
}

impl From<&BookmarkMoveAuthorization<'_>> for PushAuthoredBy {
    fn from(auth: &BookmarkMoveAuthorization<'_>) -> PushAuthoredBy {
        match auth {
            BookmarkMoveAuthorization::User => PushAuthoredBy::User,
            BookmarkMoveAuthorization::Service(_, _) => PushAuthoredBy::Service,
        }
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
        infinitepush_params: &InfinitepushParams,
        name: &BookmarkName,
    ) -> Result<BookmarkKind, BookmarkMovementError> {
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
    bookmark_to_move: &BookmarkName,
    bookmark_attrs: &BookmarkAttrs,
    pushrebase_params: &PushrebaseParams,
    lca_hint: &dyn LeastCommonAncestorsHint,
    target: ChangesetId,
) -> Result<(), BookmarkMovementError> {
    // NOTE: Obviously this is a little racy, but the bookmark could move after we check, so it
    // doesn't matter.

    let mut descendant_bookmarks = vec![];
    for attr in bookmark_attrs.select(bookmark_to_move) {
        if let Some(descendant_bookmark) = &attr.params().ensure_ancestor_of {
            descendant_bookmarks.push(descendant_bookmark);
        }
    }

    if let Some(descendant_bookmark) = &pushrebase_params.globalrevs_publishing_bookmark {
        descendant_bookmarks.push(&descendant_bookmark);
    }

    stream::iter(descendant_bookmarks)
        .map(Ok)
        .try_for_each_concurrent(10, |descendant_bookmark| async move {
            let is_ancestor = ensure_ancestor_of(
                ctx,
                repo,
                bookmark_to_move,
                lca_hint,
                &descendant_bookmark,
                target,
            )
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
    bookmark_to_move: &BookmarkName,
    lca_hint: &dyn LeastCommonAncestorsHint,
    descendant_bookmark: &BookmarkName,
    target: ChangesetId,
) -> Result<bool, BookmarkMovementError> {
    let descendant_cs_id = repo
        .bookmarks()
        .get(ctx.clone(), descendant_bookmark)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Bookmark '{}' does not exist, but it should be a descendant of '{}'!",
                descendant_bookmark,
                bookmark_to_move
            )
        })?;

    Ok(target == descendant_cs_id
        || lca_hint
            .is_ancestor(ctx, &repo.changeset_fetcher_arc(), target, descendant_cs_id)
            .await?)
}

pub fn check_bookmark_sync_config(
    repo: &(impl RepoIdentityRef + RepoCrossRepoRef),
    bookmark: &BookmarkName,
    kind: BookmarkKind,
) -> Result<(), BookmarkMovementError> {
    match kind {
        BookmarkKind::Publishing | BookmarkKind::PullDefaultPublishing => {
            if repo
                .repo_cross_repo()
                .live_commit_sync_config()
                .push_redirector_enabled_for_public(repo.repo_identity().id())
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
                .push_redirector_enabled_for_draft(repo.repo_identity().id())
            {
                return Err(BookmarkMovementError::PushRedirectorEnabledForScratch {
                    bookmark: bookmark.clone(),
                });
            }
        }
    }
    Ok(())
}
