/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::panic::RefUnwindSafe;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use thiserror::Error;

use crate::MononokeIdentitySet;

pub type ArcPermissionChecker = Arc<dyn PermissionChecker + Send + Sync + RefUnwindSafe + 'static>;
pub type BoxPermissionChecker = Box<dyn PermissionChecker + Send + Sync + RefUnwindSafe + 'static>;

/// Why an ACL check said no.
///
/// "You were not granted this action" and "a policy rejected the way you
/// connected" need different fixes, so they must not collapse into the same
/// user-facing message. These mirror the deny outcomes of the access checker's
/// `AclQueryStatusCode`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Error)]
pub enum DenialReason {
    /// The identities hold no grant for this action on the ACL.
    #[error("the identities hold no grant for this action")]
    NoMatchingPermission,
    /// No ACL exists for the resource.
    #[error("the ACL does not exist")]
    NoAcl,
    /// The ACL exists but is not active.
    #[error("the ACL is not active")]
    InactiveAcl,
    /// A kill switch is denying access.
    #[error("a kill switch is denying all access")]
    KillSwitch,
    /// The identities are explicitly blocked.
    #[error("the identities are blocklisted")]
    Blocklist,
    /// Mandatory access control rejected the request.
    #[error("mandatory access control rejected the request")]
    MandatoryAccessControl,
    /// Attribute-based access control rejected the request. `attribute` names
    /// the contextual attribute that decided it, when the checker reports one.
    #[error("attribute-based access control rejected {}", describe_attribute(.attribute))]
    AttributeBasedAccessControl { attribute: Option<String> },
    /// A soft-denied rule rejected the request.
    #[error("a soft-denied rule rejected the request")]
    SoftDeniedRule,
    /// The ACL's dependencies were not loaded, so no decision could be made.
    #[error("the ACL's dependencies are not loaded")]
    DependenciesNotLoaded,
    /// The access check itself failed. We fail closed, but this is not an
    /// authoritative deny and is worth distinguishing when debugging.
    #[error("the access check could not be completed, so access was denied")]
    CheckUnavailable,
    /// The checker reported nothing we can render.
    #[default]
    #[error("no reason was reported")]
    Unspecified,
}

/// The checker does not always name the attribute it rejected, and a message
/// ending in "rejected attribute ''" would read as a bug.
fn describe_attribute(attribute: &Option<String>) -> String {
    match attribute {
        Some(attribute) => format!("attribute '{attribute}'"),
        None => String::from("the request"),
    }
}

impl DenialReason {
    /// How much the reason tells the user. When several ACLs deny, the most
    /// informative one is worth reporting: "your agent attribute was rejected"
    /// is actionable, "no matching permission" is the default outcome.
    fn specificity(&self) -> u8 {
        match self {
            DenialReason::AttributeBasedAccessControl { .. }
            | DenialReason::MandatoryAccessControl
            | DenialReason::SoftDeniedRule
            | DenialReason::Blocklist
            | DenialReason::KillSwitch => 3,
            DenialReason::InactiveAcl
            | DenialReason::DependenciesNotLoaded
            | DenialReason::CheckUnavailable => 2,
            DenialReason::NoMatchingPermission | DenialReason::NoAcl => 1,
            DenialReason::Unspecified => 0,
        }
    }
}

/// A denied permission check: what was checked, and why the access checker
/// said no. Rendered into the error the pushing user sees, so a rejected push
/// explains itself without a trip to the ACL tool.
#[derive(Clone, Debug, Default, Eq, PartialEq, Error)]
#[error("{reason}{}", describe_target(.action, .acl))]
pub struct PermissionDenial {
    /// The ACL that was consulted, e.g. `REPO:repos/git/fbsource`.
    pub acl: Option<String>,
    /// The action that was checked, e.g. `write`.
    pub action: Option<String>,
    pub reason: DenialReason,
}

/// Name what was checked, when the checker told us. A checker that reports
/// neither leaves the reason to stand on its own rather than trailing an empty
/// parenthesis.
fn describe_target(action: &Option<String>, acl: &Option<String>) -> String {
    match (action, acl) {
        (Some(action), Some(acl)) => format!(" (action '{action}' on {acl})"),
        (Some(action), None) => format!(" (action '{action}')"),
        (None, Some(acl)) => format!(" (on {acl})"),
        (None, None) => String::new(),
    }
}

impl PermissionDenial {
    /// Whether this denial says anything the user can act on. Checkers that
    /// don't report a reason produce denials that are noise in an error
    /// message.
    pub fn is_informative(&self) -> bool {
        self.reason != DenialReason::Unspecified
    }
}

/// Result of a permission check that includes the deciding identity type, or
/// the reason for the denial.
#[derive(Clone, Debug)]
pub enum PermissionCheckResult {
    Allowed(Option<String>),
    Denied(PermissionDenial),
}

impl PermissionCheckResult {
    /// A denial with no reason attached, for checkers that don't report one.
    pub fn denied() -> Self {
        Self::Denied(PermissionDenial::default())
    }

    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed(_))
    }

    pub fn deciding_identity_type(&self) -> Option<&str> {
        match self {
            Self::Allowed(id_type) => id_type.as_deref(),
            Self::Denied(_) => None,
        }
    }

    /// The denial details, if the check denied access.
    pub fn into_denial(self) -> Option<PermissionDenial> {
        match self {
            Self::Allowed(_) => None,
            Self::Denied(denial) => Some(denial),
        }
    }
}

#[async_trait]
pub trait PermissionChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, actions: &[&str]) -> bool;

    /// Like check_set, but returns a PermissionCheckResult indicating whether
    /// access was granted and which identity type decided it, or why it was
    /// denied.
    async fn check_set_with_result(
        &self,
        accessors: &MononokeIdentitySet,
        actions: &[&str],
    ) -> PermissionCheckResult {
        if self.check_set(accessors, actions).await {
            PermissionCheckResult::Allowed(None)
        } else {
            PermissionCheckResult::denied()
        }
    }
}

pub struct PermissionCheckerBuilder {
    pub(crate) checkers: Vec<BoxPermissionChecker>,
}

impl PermissionCheckerBuilder {
    pub fn new() -> PermissionCheckerBuilder {
        PermissionCheckerBuilder {
            checkers: Vec::new(),
        }
    }

    pub fn allow(mut self, acl: BoxPermissionChecker) -> PermissionCheckerBuilder {
        self.checkers.push(acl);
        self
    }

    pub fn allow_all(mut self) -> PermissionCheckerBuilder {
        self.checkers.push(Box::new(AlwaysAllow));
        self
    }

    pub fn allow_allowlist(mut self, allowlist: MononokeIdentitySet) -> PermissionCheckerBuilder {
        self.checkers.push(Box::new(AllowlistChecker { allowlist }));
        self
    }

    pub fn build(mut self) -> BoxPermissionChecker {
        if self.checkers.len() <= 1 {
            match self.checkers.pop() {
                None => Box::new(AlwaysReject),
                Some(checker) => checker,
            }
        } else {
            Box::new(UnionPermissionChecker {
                checkers: self.checkers,
            })
        }
    }
}

pub(crate) struct AlwaysAllow;

#[async_trait]
impl PermissionChecker for AlwaysAllow {
    async fn check_set(&self, _accessors: &MononokeIdentitySet, _actions: &[&str]) -> bool {
        true
    }
}

pub(crate) struct AlwaysReject;

#[async_trait]
impl PermissionChecker for AlwaysReject {
    async fn check_set(&self, _accessors: &MononokeIdentitySet, _actions: &[&str]) -> bool {
        false
    }
}

pub(crate) struct AllowlistChecker {
    allowlist: MononokeIdentitySet,
}

#[async_trait]
impl PermissionChecker for AllowlistChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, _actions: &[&str]) -> bool {
        !self.allowlist.is_disjoint(accessors)
    }
}

struct UnionPermissionChecker {
    checkers: Vec<BoxPermissionChecker>,
}

#[async_trait]
impl PermissionChecker for UnionPermissionChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, actions: &[&str]) -> bool {
        // Check all checkers in parallel.
        let mut checks: FuturesUnordered<_> = self
            .checkers
            .iter()
            .map(|checker| async { checker.check_set(accessors, actions).await })
            .collect();

        while let Some(check_result) = checks.next().await {
            if check_result {
                // Return true as soon as any checker says access is permitted.
                return true;
            }
        }

        return false;
    }

    async fn check_set_with_result(
        &self,
        accessors: &MononokeIdentitySet,
        actions: &[&str],
    ) -> PermissionCheckResult {
        // Check all checkers in parallel.
        let mut checks: FuturesUnordered<_> = self
            .checkers
            .iter()
            .map(|checker| async { checker.check_set_with_result(accessors, actions).await })
            .collect();

        // Every checker has to deny before the union denies, so report the
        // denial that explains the most.
        let mut best: Option<PermissionDenial> = None;
        while let Some(result) = checks.next().await {
            match result {
                allowed @ PermissionCheckResult::Allowed(_) => return allowed,
                PermissionCheckResult::Denied(denial) => {
                    if best
                        .as_ref()
                        .is_none_or(|b| denial.reason.specificity() > b.reason.specificity())
                    {
                        best = Some(denial);
                    }
                }
            }
        }

        PermissionCheckResult::Denied(best.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    struct DenyWith(DenialReason);

    #[async_trait]
    impl PermissionChecker for DenyWith {
        async fn check_set(&self, _accessors: &MononokeIdentitySet, _actions: &[&str]) -> bool {
            false
        }

        async fn check_set_with_result(
            &self,
            _accessors: &MononokeIdentitySet,
            _actions: &[&str],
        ) -> PermissionCheckResult {
            PermissionCheckResult::Denied(PermissionDenial {
                acl: Some(String::from("REPO:repos/git/mzr-test")),
                action: Some(String::from("write")),
                reason: self.0.clone(),
            })
        }
    }

    fn denial(reason: DenialReason) -> PermissionDenial {
        PermissionDenial {
            acl: Some(String::from("REPO:repos/git/mzr-test")),
            action: Some(String::from("write")),
            reason,
        }
    }

    #[mononoke::test]
    fn denial_names_the_acl_and_action() {
        assert_eq!(
            denial(DenialReason::AttributeBasedAccessControl {
                attribute: Some(String::from("agent.id")),
            })
            .to_string(),
            "attribute-based access control rejected attribute 'agent.id' \
             (action 'write' on REPO:repos/git/mzr-test)"
        );
        assert_eq!(
            denial(DenialReason::NoMatchingPermission).to_string(),
            "the identities hold no grant for this action \
             (action 'write' on REPO:repos/git/mzr-test)"
        );
    }

    /// A denial with nothing to say must be recognisable as such, so callers
    /// can leave their message alone rather than append "no reason".
    #[mononoke::test]
    fn unspecified_denial_is_not_informative() {
        assert!(!PermissionDenial::default().is_informative());
        assert!(denial(DenialReason::KillSwitch).is_informative());
    }

    /// When several ACLs deny, a policy rejection tells the user more than the
    /// default "you hold no grant", so it is the one that must survive.
    #[mononoke::test]
    async fn union_reports_the_most_specific_denial() {
        let checker = PermissionCheckerBuilder::new()
            .allow(Box::new(DenyWith(DenialReason::NoMatchingPermission)))
            .allow(Box::new(DenyWith(DenialReason::KillSwitch)))
            .allow(Box::new(DenyWith(DenialReason::NoAcl)))
            .build();

        let result = checker
            .check_set_with_result(&MononokeIdentitySet::new(), &["write"])
            .await;

        assert_eq!(
            result.into_denial().map(|denial| denial.reason),
            Some(DenialReason::KillSwitch)
        );
    }

    /// Reporting a reason must not cost the deciding identity type that the
    /// allow path already records for logging.
    #[mononoke::test]
    async fn union_keeps_the_deciding_identity_type_when_allowed() {
        let checker = PermissionCheckerBuilder::new()
            .allow(Box::new(DenyWith(DenialReason::NoMatchingPermission)))
            .allow_all()
            .build();

        let result = checker
            .check_set_with_result(&MononokeIdentitySet::new(), &["write"])
            .await;

        assert!(result.is_allowed());
    }
}
