/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Result};
use async_trait::async_trait;
use auto_impl::auto_impl;
use fbinit::FacebookInit;
use slog::{trace, Logger};

use metaconfig_types::AllowlistEntry;
use permission_checker::{
    BoxPermissionChecker, MononokeIdentity, MononokeIdentitySet, PermissionCheckerBuilder,
};

#[facet::facet]
#[async_trait]
#[auto_impl(&, Arc, Box)]
pub trait RepoPermissionChecker: Send + Sync + 'static {
    async fn check_if_read_access_allowed(&self, identities: &MononokeIdentitySet) -> Result<bool>;
}

pub struct ProdRepoPermissionChecker {
    repo_permchecker: BoxPermissionChecker,
}

impl ProdRepoPermissionChecker {
    pub async fn new(
        fb: FacebookInit,
        logger: &Logger,
        hipster_acl: &Option<String>,
        reponame: &str,
        security_config: &[AllowlistEntry],
    ) -> Result<Self> {
        let repo_permchecker = if let Some(acl_name) = hipster_acl {
            PermissionCheckerBuilder::acl_for_repo(fb, acl_name)
                .await
                .with_context(|| format!("Failed to create PermissionChecker for {}", acl_name))?
        } else {
            // If we dont have an Acl config here, we just use the allowlisted identities.
            // Those are the identities we'd allow to impersonate anyone anyway. Note that
            // that this is not a setup we run in prod — it's just convenient for local
            // repos.
            let mut allowlisted_identities = MononokeIdentitySet::new();

            for allowlist_entry in security_config {
                match allowlist_entry {
                    AllowlistEntry::HardcodedIdentity { ty, data } => {
                        allowlisted_identities.insert(MononokeIdentity::new(ty, data)?);
                    }
                    AllowlistEntry::Tier(_tier) => (),
                }
            }

            trace!(
                logger,
                "No ACL set for repo {}, defaulting to allowlisted identities",
                reponame
            );
            PermissionCheckerBuilder::allowlist_checker(allowlisted_identities.clone())
        };

        Ok(Self { repo_permchecker })
    }
}

#[async_trait]
impl RepoPermissionChecker for ProdRepoPermissionChecker {
    async fn check_if_read_access_allowed(&self, identities: &MononokeIdentitySet) -> Result<bool> {
        Ok(self
            .repo_permchecker
            .check_set(identities, &["read"])
            .await?)
    }
}

pub struct AlwaysAllowMockRepoPermissionChecker {}

impl AlwaysAllowMockRepoPermissionChecker {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl RepoPermissionChecker for AlwaysAllowMockRepoPermissionChecker {
    async fn check_if_read_access_allowed(
        &self,
        _identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        Ok(true)
    }
}
