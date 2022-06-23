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

/// Repository permissions checks
///
/// Perform checks against the various access control lists associated with
/// the repository.
#[facet::facet]
#[async_trait]
#[auto_impl(&, Arc, Box)]
pub trait RepoPermissionChecker: Send + Sync + 'static {
    /// Check whether the given identities are permitted to **read** the
    /// repository.
    async fn check_if_read_access_allowed(&self, identities: &MononokeIdentitySet) -> Result<bool>;

    /// Check whether the given identities are permitted to make **draft**
    /// changes to the repository.  This means creating commit cloud commits
    /// and modifying scratch bookmarks.
    async fn check_if_draft_access_allowed(&self, identities: &MononokeIdentitySet)
    -> Result<bool>;

    /// Check whether the given identities are permitted to make **public**
    /// changes to the repository.  This means modifying public bookmarks.
    async fn check_if_write_access_allowed(&self, identities: &MononokeIdentitySet)
    -> Result<bool>;

    /// Check whether the given identities are permitted to **bypass the
    /// read-only state** of the repository.
    async fn check_if_read_only_bypass_allowed(
        &self,
        identities: &MononokeIdentitySet,
    ) -> Result<bool>;

    /// Check whether the given identities are permitted to **act as a
    /// service** to make modifications to the repository.  This means
    /// making any modification to the repository that the named service
    /// is permitted to make.
    async fn check_if_service_writes_allowed(
        &self,
        identities: &MononokeIdentitySet,
        service_name: &str,
    ) -> Result<bool>;
}

pub struct ProdRepoPermissionChecker {
    repo_permchecker: BoxPermissionChecker,
    service_permchecker: BoxPermissionChecker,
}

impl ProdRepoPermissionChecker {
    pub async fn new(
        fb: FacebookInit,
        logger: &Logger,
        repo_hipster_acl: Option<&str>,
        service_hipster_acl: Option<&str>,
        reponame: &str,
        security_config: &[AllowlistEntry],
    ) -> Result<Self> {
        let repo_permchecker = if let Some(acl_name) = repo_hipster_acl {
            PermissionCheckerBuilder::new()
                .allow_repo_acl(fb, acl_name)
                .await
                .with_context(|| format!("Failed to create PermissionChecker for {}", acl_name))?
                .build()
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
            PermissionCheckerBuilder::new()
                .allow_allowlist(allowlisted_identities.clone())
                .build()
        };
        let service_permchecker = if let Some(acl_name) = service_hipster_acl {
            PermissionCheckerBuilder::new()
                .allow_tier_acl(fb, acl_name)
                .await
                .with_context(|| format!("Failed to create PermissionChecker for {}", acl_name))?
                .build()
        } else {
            // If no service tier is set we allow anyone to act as a service
            // (this happens in integration tests).
            PermissionCheckerBuilder::new().allow_all().build()
        };

        Ok(Self {
            repo_permchecker,
            service_permchecker,
        })
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

    async fn check_if_draft_access_allowed(
        &self,
        identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        // TODO(T105334556): This should require draft permission
        // For now, we allow all readers draft access.
        Ok(self
            .repo_permchecker
            .check_set(identities, &["read"])
            .await?)
    }

    async fn check_if_write_access_allowed(
        &self,
        identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        Ok(self
            .repo_permchecker
            .check_set(identities, &["write"])
            .await?)
    }

    async fn check_if_read_only_bypass_allowed(
        &self,
        identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        Ok(self
            .repo_permchecker
            .check_set(identities, &["bypass_readonly"])
            .await?)
    }

    async fn check_if_service_writes_allowed(
        &self,
        identities: &MononokeIdentitySet,
        service_name: &str,
    ) -> Result<bool> {
        Ok(self
            .service_permchecker
            .check_set(identities, &[service_name])
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

    async fn check_if_draft_access_allowed(
        &self,
        _identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        Ok(true)
    }

    async fn check_if_write_access_allowed(
        &self,
        _identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        Ok(true)
    }

    async fn check_if_read_only_bypass_allowed(
        &self,
        _identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        Ok(true)
    }

    async fn check_if_service_writes_allowed(
        &self,
        _identities: &MononokeIdentitySet,
        _service_name: &str,
    ) -> Result<bool> {
        Ok(true)
    }
}
