/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;
use futures::join;
use scuba::ScubaSampleBuilder;
use tracing::trace;
use tracing::warn;

use crate::AclProvider;
use crate::BoxMembershipChecker;
use crate::BoxPermissionChecker;
use crate::MembershipChecker;
use crate::MononokeIdentitySet;
use crate::PermissionCheckResult;
use crate::PermissionChecker;

/// Scuba dataset where shadow-mode samples are written. One row per logged
/// event with `divergent={true,false}` to distinguish divergences from
/// baseline samples.
const SCUBA_TABLE: &str = "mononoke_shadow_perm_checker";

/// An `AclProvider` that runs two providers side-by-side. Every check and
/// membership lookup goes through both; the `primary` result is what's
/// returned to callers. Divergences are always logged. Non-divergent checks
/// are sampled at `sample_rate` (1-in-N per checker) so there's a baseline
/// of activity in the dataset even when the two providers agree.
///
/// Use this to validate a candidate provider against the production one
/// before cutting over. Construction failures from the shadow provider are
/// tolerated: the wrapper degrades into "primary-only" mode for that check
/// and logs the construction error. Primary failures propagate as normal.
pub struct ShadowAclProvider {
    primary: Arc<dyn AclProvider>,
    shadow: Arc<dyn AclProvider>,
    scuba: ScubaSampleBuilder,
    sample_rate: u64,
}

impl ShadowAclProvider {
    pub fn new(
        fb: FacebookInit,
        primary: Arc<dyn AclProvider>,
        shadow: Arc<dyn AclProvider>,
        sample_rate: u64,
    ) -> Arc<dyn AclProvider> {
        let scuba = ScubaSampleBuilder::new(fb, SCUBA_TABLE);
        Arc::new(Self {
            primary,
            shadow,
            scuba,
            sample_rate,
        })
    }
}

#[async_trait]
impl AclProvider for ShadowAclProvider {
    async fn repo_acl(&self, name: &str) -> Result<BoxPermissionChecker> {
        let (primary, shadow) = join!(self.primary.repo_acl(name), self.shadow.repo_acl(name));
        self.wrap_permission_checker(format!("repo_acl:{name}"), primary?, shadow)
    }

    async fn repo_region_acl(&self, name: &str) -> Result<BoxPermissionChecker> {
        let (primary, shadow) = join!(
            self.primary.repo_region_acl(name),
            self.shadow.repo_region_acl(name),
        );
        self.wrap_permission_checker(format!("repo_region_acl:{name}"), primary?, shadow)
    }

    async fn tier_acl(&self, name: &str) -> Result<BoxPermissionChecker> {
        let (primary, shadow) = join!(self.primary.tier_acl(name), self.shadow.tier_acl(name));
        self.wrap_permission_checker(format!("tier_acl:{name}"), primary?, shadow)
    }

    async fn commitcloud_workspace_acl(
        &self,
        name: &str,
        create_with_owner: &Option<MononokeIdentitySet>,
    ) -> Result<Option<BoxPermissionChecker>> {
        let (primary, shadow) = join!(
            self.primary
                .commitcloud_workspace_acl(name, create_with_owner),
            self.shadow
                .commitcloud_workspace_acl(name, create_with_owner),
        );
        let label = format!("commitcloud_workspace_acl:{name}");
        match (primary?, shadow) {
            (Some(p), Ok(Some(s))) => self.wrap_permission_checker(label, p, Ok(s)).map(Some),
            (Some(p), Ok(None)) => {
                warn!("[acl_checker_shadow] {label} exists in primary but not in shadow");
                self.log_existence_divergence(&label, true, false);
                self.wrap_permission_checker_unshadowed(label, p).map(Some)
            }
            (None, Ok(Some(_))) => {
                warn!("[acl_checker_shadow] {label} exists in shadow but not in primary");
                self.log_existence_divergence(&label, false, true);
                Ok(None)
            }
            (None, Ok(None)) => {
                trace!("[acl_checker_shadow] {label} absent in both primary and shadow");
                Ok(None)
            }
            (primary_result, Err(e)) => {
                warn!("[acl_checker_shadow] {label} shadow provider failed: {e:#}");
                match primary_result {
                    Some(p) => self.wrap_permission_checker_unshadowed(label, p).map(Some),
                    None => Ok(None),
                }
            }
        }
    }

    async fn group(&self, name: &str) -> Result<BoxMembershipChecker> {
        let (primary, shadow) = join!(self.primary.group(name), self.shadow.group(name));
        self.wrap_membership_checker(format!("group:{name}"), primary?, shadow)
    }

    async fn admin_group(&self) -> Result<BoxMembershipChecker> {
        let (primary, shadow) = join!(self.primary.admin_group(), self.shadow.admin_group());
        self.wrap_membership_checker("admin_group".to_string(), primary?, shadow)
    }

    async fn reviewers_group(&self) -> Result<BoxMembershipChecker> {
        let (primary, shadow) = join!(
            self.primary.reviewers_group(),
            self.shadow.reviewers_group(),
        );
        self.wrap_membership_checker("reviewers_group".to_string(), primary?, shadow)
    }
}

impl ShadowAclProvider {
    fn wrap_permission_checker(
        &self,
        label: String,
        primary: BoxPermissionChecker,
        shadow: Result<BoxPermissionChecker>,
    ) -> Result<BoxPermissionChecker> {
        let shadow = match shadow {
            Ok(s) => Some(s),
            Err(e) => {
                warn!("[acl_checker_shadow] {label} shadow construction failed: {e:#}");
                None
            }
        };
        trace!(
            "[acl_checker_shadow] built permission checker for {label} (shadow {})",
            if shadow.is_some() {
                "enabled"
            } else {
                "disabled"
            },
        );
        Ok(Box::new(ShadowPermissionChecker {
            primary,
            shadow,
            label,
            scuba: self.scuba.clone(),
            sample_rate: self.sample_rate,
            sample_counter: AtomicU64::new(0),
        }))
    }

    fn wrap_permission_checker_unshadowed(
        &self,
        label: String,
        primary: BoxPermissionChecker,
    ) -> Result<BoxPermissionChecker> {
        trace!(
            "[acl_checker_shadow] built permission checker for {label} (shadow disabled, primary-only)"
        );
        Ok(Box::new(ShadowPermissionChecker {
            primary,
            shadow: None,
            label,
            scuba: self.scuba.clone(),
            sample_rate: self.sample_rate,
            sample_counter: AtomicU64::new(0),
        }))
    }

    fn wrap_membership_checker(
        &self,
        label: String,
        primary: BoxMembershipChecker,
        shadow: Result<BoxMembershipChecker>,
    ) -> Result<BoxMembershipChecker> {
        let shadow = match shadow {
            Ok(s) => Some(s),
            Err(e) => {
                warn!("[acl_checker_shadow] {label} shadow construction failed: {e:#}");
                None
            }
        };
        trace!(
            "[acl_checker_shadow] built membership checker for {label} (shadow {})",
            if shadow.is_some() {
                "enabled"
            } else {
                "disabled"
            },
        );
        Ok(Box::new(ShadowMembershipChecker {
            primary,
            shadow,
            label,
            scuba: self.scuba.clone(),
            sample_rate: self.sample_rate,
            sample_counter: AtomicU64::new(0),
        }))
    }

    fn log_existence_divergence(&self, label: &str, primary_exists: bool, shadow_exists: bool) {
        let mut sample = self.scuba.clone();
        sample.add("label", label);
        sample.add("check_type", "commitcloud_existence");
        sample.add("primary_allowed", primary_exists);
        sample.add("shadow_allowed", shadow_exists);
        sample.add("divergent", true);
        sample.log();
    }
}

struct ShadowPermissionChecker {
    primary: BoxPermissionChecker,
    shadow: Option<BoxPermissionChecker>,
    label: String,
    scuba: ScubaSampleBuilder,
    sample_rate: u64,
    sample_counter: AtomicU64,
}

#[async_trait]
impl PermissionChecker for ShadowPermissionChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, actions: &[&str]) -> bool {
        let primary = self.primary.check_set(accessors, actions).await;
        if let Some(shadow) = &self.shadow {
            let shadow_result = shadow.check_set(accessors, actions).await;
            let divergent = primary != shadow_result;
            trace!(
                "[acl_checker_shadow] check_set on {} actions={:?}: primary={} shadow={} divergent={}",
                self.label, actions, primary, shadow_result, divergent,
            );
            if divergent {
                warn!(
                    "[acl_checker_shadow] check_set divergence on {} actions={:?}: primary={} shadow={}",
                    self.label, actions, primary, shadow_result,
                );
            }
            if divergent || should_sample(&self.sample_counter, self.sample_rate) {
                log_check(
                    &self.scuba,
                    &self.label,
                    "check_set",
                    accessors,
                    actions,
                    primary,
                    shadow_result,
                    divergent,
                );
            }
        } else {
            trace!(
                "[acl_checker_shadow] check_set on {} actions={:?}: primary={} (shadow disabled)",
                self.label, actions, primary,
            );
        }
        primary
    }

    async fn check_set_with_result(
        &self,
        accessors: &MononokeIdentitySet,
        actions: &[&str],
    ) -> PermissionCheckResult {
        let primary = self.primary.check_set_with_result(accessors, actions).await;
        if let Some(shadow) = &self.shadow {
            let shadow_result = shadow.check_set_with_result(accessors, actions).await;
            let primary_allowed = primary.is_allowed();
            let shadow_allowed = shadow_result.is_allowed();
            let divergent = primary_allowed != shadow_allowed;
            trace!(
                "[acl_checker_shadow] check_set_with_result on {} actions={:?}: primary_allowed={} shadow_allowed={} divergent={}",
                self.label, actions, primary_allowed, shadow_allowed, divergent,
            );
            if divergent {
                warn!(
                    "[acl_checker_shadow] check_set_with_result divergence on {} actions={:?}: primary_allowed={} shadow_allowed={}",
                    self.label, actions, primary_allowed, shadow_allowed,
                );
            }
            if divergent || should_sample(&self.sample_counter, self.sample_rate) {
                log_check(
                    &self.scuba,
                    &self.label,
                    "check_set_with_result",
                    accessors,
                    actions,
                    primary_allowed,
                    shadow_allowed,
                    divergent,
                );
            }
        } else {
            trace!(
                "[acl_checker_shadow] check_set_with_result on {} actions={:?}: primary_allowed={} (shadow disabled)",
                self.label,
                actions,
                primary.is_allowed(),
            );
        }
        primary
    }
}

struct ShadowMembershipChecker {
    primary: BoxMembershipChecker,
    shadow: Option<BoxMembershipChecker>,
    label: String,
    scuba: ScubaSampleBuilder,
    sample_rate: u64,
    sample_counter: AtomicU64,
}

#[async_trait]
impl MembershipChecker for ShadowMembershipChecker {
    async fn is_member(&self, identities: &MononokeIdentitySet) -> bool {
        let primary = self.primary.is_member(identities).await;
        if let Some(shadow) = &self.shadow {
            let shadow_result = shadow.is_member(identities).await;
            let divergent = primary != shadow_result;
            trace!(
                "[acl_checker_shadow] is_member on {}: primary={} shadow={} divergent={}",
                self.label, primary, shadow_result, divergent,
            );
            if divergent {
                warn!(
                    "[acl_checker_shadow] is_member divergence on {}: primary={} shadow={}",
                    self.label, primary, shadow_result,
                );
            }
            if divergent || should_sample(&self.sample_counter, self.sample_rate) {
                log_check(
                    &self.scuba,
                    &self.label,
                    "is_member",
                    identities,
                    &[],
                    primary,
                    shadow_result,
                    divergent,
                );
            }
        } else {
            trace!(
                "[acl_checker_shadow] is_member on {}: primary={} (shadow disabled)",
                self.label, primary,
            );
        }
        primary
    }
}

fn should_sample(counter: &AtomicU64, rate: u64) -> bool {
    if rate == 0 {
        return false;
    }
    counter.fetch_add(1, Ordering::Relaxed).is_multiple_of(rate)
}

fn log_check(
    scuba: &ScubaSampleBuilder,
    label: &str,
    check_type: &str,
    identities: &MononokeIdentitySet,
    actions: &[&str],
    primary_allowed: bool,
    shadow_allowed: bool,
    divergent: bool,
) {
    let mut sample = scuba.clone();
    sample.add("label", label);
    sample.add("check_type", check_type);
    sample.add("identities", format_identities(identities));
    sample.add("primary_allowed", primary_allowed);
    sample.add("shadow_allowed", shadow_allowed);
    sample.add("divergent", divergent);
    if !actions.is_empty() {
        sample.add("actions", actions.join(","));
    }
    sample.log();
}

fn format_identities(identities: &MononokeIdentitySet) -> String {
    identities
        .iter()
        .map(|id| format!("{}:{}", id.id_type(), id.id_data()))
        .collect::<Vec<_>>()
        .join(",")
}
