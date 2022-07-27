/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use metaconfig_types::CommonConfig;
use metaconfig_types::Identity;
use permission_checker::AclProvider;
use permission_checker::BoxPermissionChecker;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use permission_checker::PermissionCheckerBuilder;

pub struct ConnectionSecurityChecker {
    checker: BoxPermissionChecker,
}

impl ConnectionSecurityChecker {
    pub async fn new(acl_provider: impl AclProvider, common_config: &CommonConfig) -> Result<Self> {
        let mut builder = PermissionCheckerBuilder::new();

        if let Some(tier) = &common_config.trusted_parties_hipster_tier {
            builder = builder.allow(acl_provider.tier_acl(tier).await?);
        }

        let mut allowlisted_identities = MononokeIdentitySet::new();
        for Identity { id_type, id_data } in &common_config.trusted_parties_allowlist {
            allowlisted_identities.insert(MononokeIdentity::new(id_type, id_data));
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
