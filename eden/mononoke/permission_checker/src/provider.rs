/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use auto_impl::auto_impl;

use crate::BoxMembershipChecker;
use crate::BoxPermissionChecker;

/// A provider of access control lists and groups.
///
/// These lists and groups control permissions to access various aspects of
/// Mononoke.
#[async_trait]
#[auto_impl(&, Arc, Box)]
pub trait AclProvider: Send + Sync {
    /// Returns a permission checker for the access control list that
    /// controls the named repository.
    async fn repo_acl(&self, name: &str) -> Result<BoxPermissionChecker>;

    /// Returns a permission checker for the access control list that
    /// controls the named repository region.
    async fn repo_region_acl(&self, name: &str) -> Result<BoxPermissionChecker>;

    /// Returns a permission checker for the named non-repo-specific
    /// access control list.
    async fn tier_acl(&self, name: &str) -> Result<BoxPermissionChecker>;

    /// Returns a membership checker for the named group.
    async fn group(&self, name: &str) -> Result<BoxMembershipChecker>;

    /// Returns a membership checker for the group that may administrate
    /// Mononoke.
    async fn admin_group(&self) -> Result<BoxMembershipChecker>;

    /// Returns a membership checker for the group that may review changes.
    async fn reviewers_group(&self) -> Result<BoxMembershipChecker>;
}
