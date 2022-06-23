/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Result};
use fbinit::FacebookInit;
use openssl::x509::X509;

use crate::checker::{BoxPermissionChecker, PermissionCheckerBuilder};
use crate::identity::{MononokeIdentity, MononokeIdentitySet, MononokeIdentitySetExt};
use crate::membership::{BoxMembershipChecker, MembershipCheckerBuilder};

impl MononokeIdentity {
    pub fn reviewer_identities(_username: &str) -> MononokeIdentitySet {
        MononokeIdentitySet::new()
    }

    pub fn try_from_ssh_encoded(_encoded: &str) -> Result<MononokeIdentitySet> {
        bail!("Decoding from SSH Principals is not yet implemented for MononokeIdentity")
    }

    pub fn try_from_json_encoded(_: &str) -> Result<MononokeIdentitySet> {
        bail!("Decoding from JSON is not yet implemented for MononokeIdentity")
    }

    pub fn try_from_x509(cert: &X509) -> Result<MononokeIdentitySet> {
        let subject_vec: Result<Vec<_>> = cert
            .subject_name()
            .entries()
            .map(|entry| {
                Ok(format!(
                    "{}={}",
                    entry.object().nid().short_name()?,
                    entry.data().as_utf8()?
                ))
            })
            .collect();
        let subject_name = subject_vec?.as_slice().join(",");

        let mut idents = MononokeIdentitySet::new();
        idents.insert(MononokeIdentity::new("X509_SUBJECT_NAME", subject_name)?);
        Ok(idents)
    }
}

impl MononokeIdentitySetExt for MononokeIdentitySet {
    fn is_quicksand(&self) -> bool {
        false
    }

    fn is_hg_sync_job(&self) -> bool {
        false
    }

    fn hostprefix(&self) -> Option<&str> {
        None
    }

    fn hostname(&self) -> Option<&str> {
        None
    }
}

impl PermissionCheckerBuilder {
    pub async fn allow_repo_acl(
        self,
        _fb: FacebookInit,
        _name: &str,
    ) -> Result<PermissionCheckerBuilder> {
        Ok(self.allow_all())
    }

    pub async fn allow_tier_acl(
        self,
        _fb: FacebookInit,
        _name: &str,
    ) -> Result<PermissionCheckerBuilder> {
        Ok(self.allow_all())
    }
}

impl MembershipCheckerBuilder {
    pub async fn for_reviewers_group(_fb: FacebookInit) -> Result<BoxMembershipChecker> {
        Ok(Self::always_member())
    }

    pub async fn for_admin_group(_fb: FacebookInit) -> Result<BoxMembershipChecker> {
        Ok(Self::never_member())
    }

    pub async fn for_group(_fb: FacebookInit, _group_name: &str) -> Result<BoxMembershipChecker> {
        Ok(Self::never_member())
    }
}
