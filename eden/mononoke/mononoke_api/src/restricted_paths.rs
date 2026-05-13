/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::NonRootMPath;
use restricted_paths::PathRestrictionInfo;
use restricted_paths::PermissionRequestGroup;

/// Access check result for a restricted path.
#[derive(Clone, Debug, PartialEq)]
pub struct PathAccessInfo {
    /// Core restriction info from the restricted_paths crate.
    pub restriction: PathRestrictionInfo,

    /// Whether the caller has access. None if not checked.
    pub has_access: Option<bool>,
}

impl PathAccessInfo {
    /// Convenience accessor for the restriction root.
    pub fn restriction_root(&self) -> &NonRootMPath {
        &self.restriction.restriction_root
    }

    /// Convenience accessor for the repo region ACL.
    pub fn repo_region_acl(&self) -> &str {
        &self.restriction.repo_region_acl
    }

    /// Convenience accessor for the permission request group.
    pub fn permission_request_group(&self) -> &PermissionRequestGroup {
        &self.restriction.permission_request_group
    }
}

/// Information about restricted path changes in a changeset.
#[derive(Clone, Debug, PartialEq)]
pub struct RestrictedPathsChangesInfo {
    /// Changed paths that fall under restrictions, grouped by restriction root.
    pub restricted_changes: Vec<RestrictedChangeGroup>,
}

/// A group of changed paths that share the same restriction root.
#[derive(Clone, Debug, PartialEq)]
pub struct RestrictedChangeGroup {
    /// The restriction root and access info covering these changes.
    pub restriction_info: PathAccessInfo,
    // TODO(T248660146): remove this field and `RestrictedChangeGroup` if there's
    // no need to use it for now.
    /// The changed paths under this restriction root.
    pub changed_paths: Vec<NonRootMPath>,
}
