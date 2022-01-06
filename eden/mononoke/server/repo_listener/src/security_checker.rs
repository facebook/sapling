/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::repo_handlers::RepoHandler;
use anyhow::{bail, Context, Error, Result};
use borrowed::borrowed;
use fbinit::FacebookInit;
use futures::future::try_join_all;
use metaconfig_types::{AllowlistEntry, CommonConfig};
use permission_checker::{
    BoxMembershipChecker, BoxPermissionChecker, MembershipCheckerBuilder, MononokeIdentity,
    MononokeIdentitySet, PermissionCheckerBuilder,
};
use slog::{warn, Logger};
use std::collections::HashMap;

pub struct ConnectionsSecurityChecker {
    tier_permchecker: BoxPermissionChecker,
    allowlisted_checker: BoxMembershipChecker,
    repo_permcheckers: HashMap<String, BoxPermissionChecker>,
}

impl ConnectionsSecurityChecker {
    pub async fn new(
        fb: FacebookInit,
        common_config: CommonConfig,
        repo_handlers: &HashMap<String, RepoHandler>,
        logger: &Logger,
    ) -> Result<Self> {
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
                    tier_permchecker =
                        Some(PermissionCheckerBuilder::acl_for_tier(fb, &tier).await?);
                }
            }
        }

        let futures = repo_handlers.iter().map(|(reponame, repohandler)| {
            borrowed!(allowlisted_identities);
            async move {
                if let Some(acl_name) = repohandler.repo.hipster_acl() {
                    let permchecker = PermissionCheckerBuilder::acl_for_repo(fb, acl_name)
                        .await
                        .with_context(|| {
                            format!("Failed to create PermissionChecker for {}", acl_name)
                        })?;

                    Result::<(String, BoxPermissionChecker), Error>::Ok((
                        reponame.clone(),
                        permchecker,
                    ))
                } else {
                    // If we dont have an Acl config here, we just use the allowlisted identities.
                    // Those are the identities we'd allow to impersonate anyone anyway. Note that
                    // that this is not a setup we run in prod — it's just convenient for local
                    // repos.
                    warn!(
                        logger,
                        "No ACL set for repo {}, defaulting to allowlisted identities", reponame
                    );
                    Result::<(String, BoxPermissionChecker), Error>::Ok((
                        reponame.clone(),
                        PermissionCheckerBuilder::allowlist_checker(allowlisted_identities.clone()),
                    ))
                }
            }
        });

        let repo_permcheckers: HashMap<_, _> = try_join_all(futures).await?.into_iter().collect();

        Ok(Self {
            tier_permchecker: tier_permchecker
                .unwrap_or_else(|| PermissionCheckerBuilder::always_reject()),
            allowlisted_checker: MembershipCheckerBuilder::allowlist_checker(
                allowlisted_identities,
            ),
            repo_permcheckers,
        })
    }

    pub async fn check_if_trusted(&self, identities: &MononokeIdentitySet) -> Result<bool> {
        let action = "trusted_parties";
        Ok(self.allowlisted_checker.is_member(&identities).await?
            || self
                .tier_permchecker
                .check_set(&identities, &[action])
                .await?)
    }

    pub async fn check_if_repo_access_allowed(
        &self,
        reponame: &str,
        identities: &MononokeIdentitySet,
    ) -> Result<bool> {
        match self.repo_permcheckers.get(reponame) {
            Some(permchecker) => Ok(permchecker.check_set(&identities, &["read"]).await?),
            None => Ok(false),
        }
    }
}
