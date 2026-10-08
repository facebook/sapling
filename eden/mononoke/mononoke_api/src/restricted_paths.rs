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

/// JustKnobs gate for partial SCS responses. A client gets skipping
/// behavior when the killswitch is on for its repo, or when it sends the
/// opt-in request header. A client that sends the opt-out request header
/// never gets skipping behavior: denials fail the whole request for it,
/// whatever the killswitch says. The killswitch covers SCS path-info,
/// last-changed, file-diffs, compare and find-files, plus the
/// diff-service compare and single-pair diffs behind the remote route.
/// Switched on repo name for per-repo rollout; when off (the default)
/// and no header is sent, denials fail the whole request.
pub const SCS_ENABLE_PARTIAL_RESPONSES_JK: &str = "scm/mononoke:scs_enable_partial_responses";

impl RestrictedPathsPolicy {
    /// Build the policy for the current SCS (or diff-service) request.
    /// Skipping is allowed when the killswitch is on for the repo or the
    /// client opted in via the request header, unless the client opted out:
    /// an opted-out client gets `Strict` even when the killswitch is on.
    /// Evaluate ONCE per request
    /// and share the result across every call, so the knob decision cannot
    /// disagree with itself. A missing knob is an error; the knob exists
    /// with default off, and integration tests enable it via
    /// `merge_just_knobs`.
    pub fn for_scs_request(ctx: &CoreContext, repo_name: &str) -> Self {
        if ctx.metadata().partial_responses_opt_out() {
            return Self::Strict;
        }
        let client_opt_in = ctx.metadata().partial_responses_opt_in();
        let knob_on = justknobs::eval(SCS_ENABLE_PARTIAL_RESPONSES_JK, None, Some(repo_name));
        if client_opt_in || knob_on {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use fbinit::FacebookInit;
    use futures::FutureExt;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::with_just_knobs_async;
    use metadata::Metadata;
    use mononoke_macros::mononoke;

    use super::*;

    fn knobs(enabled: bool) -> JustKnobsInMemory {
        JustKnobsInMemory::new(HashMap::from([(
            SCS_ENABLE_PARTIAL_RESPONSES_JK.to_string(),
            KnobVal::Bool(enabled),
        )]))
    }

    fn ctx_with_opt_flags(fb: FacebookInit, opt_in: bool, opt_out: bool) -> CoreContext {
        let mut metadata = Metadata::default();
        metadata.add_partial_responses_opt_in(opt_in);
        metadata.add_partial_responses_opt_out(opt_out);
        CoreContext::test_mock(fb).with_overridden_metadata(Arc::new(metadata))
    }

    #[mononoke::fbinit_test]
    async fn strict_without_knob_or_opt_in(fb: FacebookInit) -> Result<(), anyhow::Error> {
        let ctx = ctx_with_opt_flags(fb, false, false);
        with_just_knobs_async(
            knobs(false),
            async move {
                assert!(matches!(
                    RestrictedPathsPolicy::for_scs_request(&ctx, "repo"),
                    RestrictedPathsPolicy::Strict
                ));
                Ok(())
            }
            .boxed(),
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn opt_in_skips_without_knob(fb: FacebookInit) -> Result<(), anyhow::Error> {
        let ctx = ctx_with_opt_flags(fb, true, false);
        with_just_knobs_async(
            knobs(false),
            async move {
                let policy = RestrictedPathsPolicy::for_scs_request(&ctx, "repo");
                assert_eq!(policy.omitted_count(), Some(0));
                Ok(())
            }
            .boxed(),
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn knob_skips_without_opt_in(fb: FacebookInit) -> Result<(), anyhow::Error> {
        let ctx = ctx_with_opt_flags(fb, false, false);
        with_just_knobs_async(
            knobs(true),
            async move {
                let policy = RestrictedPathsPolicy::for_scs_request(&ctx, "repo");
                assert_eq!(policy.omitted_count(), Some(0));
                Ok(())
            }
            .boxed(),
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn opt_out_is_strict_despite_knob(fb: FacebookInit) -> Result<(), anyhow::Error> {
        let ctx = ctx_with_opt_flags(fb, false, true);
        with_just_knobs_async(
            knobs(true),
            async move {
                assert!(matches!(
                    RestrictedPathsPolicy::for_scs_request(&ctx, "repo"),
                    RestrictedPathsPolicy::Strict
                ));
                Ok(())
            }
            .boxed(),
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn opt_out_beats_opt_in(fb: FacebookInit) -> Result<(), anyhow::Error> {
        let ctx = ctx_with_opt_flags(fb, true, true);
        with_just_knobs_async(
            knobs(false),
            async move {
                assert!(matches!(
                    RestrictedPathsPolicy::for_scs_request(&ctx, "repo"),
                    RestrictedPathsPolicy::Strict
                ));
                Ok(())
            }
            .boxed(),
        )
        .await
    }
}
