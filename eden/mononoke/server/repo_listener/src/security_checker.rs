/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Result};
use fbinit::FacebookInit;
use metaconfig_types::{AllowlistEntry, CommonConfig};
use permission_checker::{
    BoxMembershipChecker, BoxPermissionChecker, MembershipCheckerBuilder, MononokeIdentity,
    MononokeIdentitySet, PermissionCheckerBuilder,
};

pub struct ConnectionsSecurityChecker {
    tier_permchecker: BoxPermissionChecker,
    allowlisted_checker: BoxMembershipChecker,
}

impl ConnectionsSecurityChecker {
    pub async fn new(fb: FacebookInit, common_config: CommonConfig) -> Result<Self> {
        let mut allowlisted_identities = MononokeIdentitySet::new();
        let mut tier_permchecker = None;

        for allowlist_entry in common_config.security_config {
            match allowlist_entry {
                AllowlistEntry::HardcodedIdentity { ty, data } => {
                    allowlisted_identities.insert(MononokeIdentity::new(&ty, &data)?);
                }
                AllowlistEntry::Tier(tier) => {
                    if tier_permchecker.is_some() {
                        bail!("invalid config: only one PermissionChecker for tier is allowed");
                    }
                    tier_permchecker = Some(
                        PermissionCheckerBuilder::new()
                            .allow_tier_acl(fb, &tier)
                            .await?
                            .build(),
                    );
                }
            }
        }

        Ok(Self {
            tier_permchecker: tier_permchecker
                .unwrap_or_else(|| PermissionCheckerBuilder::new().build()),
            allowlisted_checker: MembershipCheckerBuilder::allowlist_checker(
                allowlisted_identities,
            ),
        })
    }

    pub async fn check_if_trusted(&self, identities: &MononokeIdentitySet) -> Result<bool> {
        // This function is conflating two very different concepts:
        //  * allowlisted identities (used to allow system admins to always access the system)
        //  * trusted parties access used to allow trust some tiers (like proxies) with user identities they provide
        //
        // There's absolutely no reason for this to be a single function.
        //
        // TODO(T115444202): tidy up this logic
        let action = "trusted_parties";
        Ok(self.allowlisted_checker.is_member(&identities).await?
            || self
                .tier_permchecker
                .check_set(&identities, &[action])
                .await?)
    }
}
