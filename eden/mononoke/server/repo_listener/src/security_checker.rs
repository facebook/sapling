/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use fbinit::FacebookInit;
use metaconfig_types::{AllowlistIdentity, CommonConfig};
use permission_checker::{
    BoxPermissionChecker, MononokeIdentity, MononokeIdentitySet, PermissionCheckerBuilder,
};

pub struct ConnectionsSecurityChecker {
    checker: BoxPermissionChecker,
}

impl ConnectionsSecurityChecker {
    pub async fn new(fb: FacebookInit, common_config: CommonConfig) -> Result<Self> {
        let mut builder = PermissionCheckerBuilder::new();

        if let Some(tier) = &common_config.trusted_parties_hipster_tier {
            builder = builder.allow_tier_acl(fb, tier).await?;
        }

        let mut allowlisted_identities = MononokeIdentitySet::new();
        for AllowlistIdentity { id_type, id_data } in &common_config.trusted_parties_allowlist {
            allowlisted_identities.insert(MononokeIdentity::new(id_type, id_data)?);
        }
        if !allowlisted_identities.is_empty() {
            builder = builder.allow_allowlist(allowlisted_identities);
        }

        Ok(Self {
            checker: builder.build(),
        })
    }

    /// Check if the given identities are trusted to act as a proxy, and
    /// provide the identities of the originator of the request.
    pub async fn check_if_trusted(&self, identities: &MononokeIdentitySet) -> Result<bool> {
        self.checker
            .check_set(identities, &["trusted_parties"])
            .await
    }
}
