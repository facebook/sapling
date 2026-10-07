/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use context::CoreContext;
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

/// What to do when restricted-path enforcement denies a path.
///
/// Provided as a parameter to all methods that may encounter denials.
/// Callers that do not handle omissions should use `Strict`.
#[derive(Clone, Debug)]
pub enum RestrictedPathsPolicy {
    /// Denials fail the whole request.
    Strict,
    /// Skip (streams) or omit-or-downgrade (diffs) denied paths, counting
    /// them in the shared counter. Repo-auth denials and all other errors
    /// still fail the whole request.
    SkipAndCount(Arc<AtomicUsize>),
}

/// JustKnobs gate for partial SCS responses. One killswitch for the whole
/// feature: SCS path-info, last-changed, file-diffs and compare, plus the
/// diff-service compare and single-pair diffs behind the remote route.
/// When off (the default), denials fail the whole request.
pub const SCS_ENABLE_PARTIAL_RESPONSES_JK: &str = "scm/mononoke:scs_enable_partial_responses";

/// Evaluate the partial-responses killswitch. A missing knob is an error;
/// the knob exists with default off, and integration tests enable it via
/// `merge_just_knobs`.
pub fn scs_partial_responses_enabled() -> bool {
    justknobs::eval(SCS_ENABLE_PARTIAL_RESPONSES_JK, None, None)
}

impl RestrictedPathsPolicy {
    /// Build the policy for the current SCS (or diff-service) request from
    /// the killswitch. Evaluate ONCE per request and share the result across
    /// every call, so the knob decision cannot disagree with itself.
    pub fn for_scs_request() -> Self {
        if scs_partial_responses_enabled() {
            Self::SkipAndCount(Arc::new(AtomicUsize::new(0)))
        } else {
            Self::Strict
        }
    }

    /// Record one omitted path.
    pub fn record_omission(&self) {
        if let Self::SkipAndCount(omitted) = self {
            omitted.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Number of paths omitted so far, or `None` when partial responses
    /// are not allowed.
    pub fn omitted_count(&self) -> Option<usize> {
        match self {
            Self::Strict => None,
            Self::SkipAndCount(omitted) => Some(omitted.load(Ordering::Relaxed)),
        }
    }

    /// Set the partial-response flag when paths were omitted.
    pub fn set_partial_if_omitted(&self, ctx: &CoreContext) {
        if self.omitted_count().is_some_and(|count| count > 0) {
            ctx.set_partial_response();
        }
    }
}
