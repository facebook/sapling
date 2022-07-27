/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repo bookmark attributes
//!
//! Stores configuration and permission checkers for bookmarks

use anyhow::bail;
use anyhow::Result;
use bookmarks_types::BookmarkName;
use context::CoreContext;
use metaconfig_types::BookmarkParams;
use permission_checker::AclProvider;
use permission_checker::BoxMembershipChecker;

/// Repository bookmark attributes.
#[facet::facet]
pub struct RepoBookmarkAttrs {
    bookmark_attrs: Vec<BookmarkAttr>,
}

impl RepoBookmarkAttrs {
    /// Construct a new RepoBookmarkAttrs.
    pub async fn new(
        acl_provider: &dyn AclProvider,
        bookmark_params: impl IntoIterator<Item = BookmarkParams>,
    ) -> Result<RepoBookmarkAttrs> {
        let mut bookmark_attrs = Vec::new();
        for params in bookmark_params {
            let attr = BookmarkAttr::new(acl_provider, params).await?;
            bookmark_attrs.push(attr);
        }
        Ok(RepoBookmarkAttrs { bookmark_attrs })
    }

    /// Construct a new RepoBookmarkAttrs for testing.
    pub fn new_test(
        bookmark_params: impl IntoIterator<Item = BookmarkParams>,
    ) -> Result<RepoBookmarkAttrs> {
        let mut bookmark_attrs = Vec::new();
        for params in bookmark_params {
            let attr = BookmarkAttr::new_test(params)?;
            bookmark_attrs.push(attr);
        }
        Ok(RepoBookmarkAttrs { bookmark_attrs })
    }

    /// Select bookmark params matching provided bookmark
    pub fn select<'a>(
        &'a self,
        bookmark: &'a BookmarkName,
    ) -> impl Iterator<Item = &'a BookmarkAttr> {
        self.bookmark_attrs
            .iter()
            .filter(move |attr| attr.params().bookmark.matches(bookmark))
    }

    /// Check if provided bookmark is fast-forward only
    pub fn is_fast_forward_only(&self, bookmark: &BookmarkName) -> bool {
        self.select(bookmark)
            .any(|attr| attr.params().only_fast_forward)
    }

    /// Check if a bookmark config overrides whether date should be rewritten during pushrebase.
    /// Return None if there are no bookmark config overriding rewrite_dates.
    pub fn should_rewrite_dates(&self, bookmark: &BookmarkName) -> Option<bool> {
        for attr in self.select(bookmark) {
            // NOTE: If there are multiple patterns matching the bookmark, the first match
            // overrides others.
            if let Some(rewrite_dates) = attr.params().rewrite_dates {
                return Some(rewrite_dates);
            }
        }
        None
    }

    /// Check if the user is allowed to move the specified bookmark
    pub async fn is_allowed_user(
        &self,
        ctx: &CoreContext,
        unixname: &str,
        bookmark: &BookmarkName,
    ) -> Result<bool> {
        for attr in self.select(bookmark) {
            let maybe_allowed = attr
                .params()
                .allowed_users
                .as_ref()
                .map(|re| re.is_match(unixname));

            let maybe_member = if let Some(membership) = &attr.membership {
                Some(membership.is_member(ctx.metadata().identities()).await?)
            } else {
                None
            };

            // Check if the user is either allowed to access it or that they
            // are a member of the allowed hipster group.
            //
            // If there is no allowlist and no configured group, then
            // everyone is permitted access.
            let allowed = match (maybe_allowed, maybe_member) {
                (Some(x), Some(y)) => x || y,
                (Some(x), None) | (None, Some(x)) => x,
                (None, None) => true,
            };
            if !allowed {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Single set of attributes for a bookmark or bookmark pattern.
pub struct BookmarkAttr {
    params: BookmarkParams,
    membership: Option<BoxMembershipChecker>,
}

impl BookmarkAttr {
    async fn new(acl_provider: &dyn AclProvider, params: BookmarkParams) -> Result<BookmarkAttr> {
        let membership = match &params.allowed_hipster_group {
            Some(hipster_group) => Some(acl_provider.group(hipster_group).await?),
            None => None,
        };
        Ok(BookmarkAttr { params, membership })
    }

    fn new_test(params: BookmarkParams) -> Result<BookmarkAttr> {
        if params.allowed_hipster_group.is_some() {
            bail!("Bookmark hipster groups are not supported in tests");
        }
        Ok(BookmarkAttr {
            params,
            membership: None,
        })
    }

    /// Bookmark parameters from config
    pub fn params(&self) -> &BookmarkParams {
        &self.params
    }

    /// Membership checker
    pub fn membership(&self) -> Option<&BoxMembershipChecker> {
        self.membership.as_ref()
    }
}
