/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks_types::BookmarkName;
use context::CoreContext;
use metaconfig_types::{BookmarkAttrs, InfinitepushParams, SourceControlServiceParams};

use crate::BookmarkMovementError;

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
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum BookmarkKind {
    Scratch,
    Public,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum BookmarkKindRestrictions {
    AnyKind,
    OnlyScratch,
    OnlyPublic,
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
            (Self::OnlyPublic, Some(namespace)) if namespace.matches_bookmark(name) => {
                Err(BookmarkMovementError::InvalidPublicBookmark {
                    bookmark: name.clone(),
                    pattern: namespace.as_str().to_string(),
                })
            }
            (_, Some(namespace)) if namespace.matches_bookmark(name) => Ok(BookmarkKind::Scratch),
            (_, _) => Ok(BookmarkKind::Public),
        }
    }
}
