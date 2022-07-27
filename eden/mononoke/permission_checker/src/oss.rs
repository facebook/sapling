/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;
use openssl::x509::X509;

use crate::checker::AlwaysAllow;
use crate::checker::BoxPermissionChecker;
use crate::identity::MononokeIdentity;
use crate::identity::MononokeIdentitySet;
use crate::identity::MononokeIdentitySetExt;
use crate::membership::AlwaysMember;
use crate::membership::BoxMembershipChecker;
use crate::membership::NeverMember;
use crate::provider::AclProvider;

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
        idents.insert(MononokeIdentity::new("X509_SUBJECT_NAME", subject_name));
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

pub struct DummyAclProvider;

impl DummyAclProvider {
    pub fn new(_fb: FacebookInit) -> Box<dyn AclProvider> {
        Box::new(DummyAclProvider)
    }
}

#[async_trait]
impl AclProvider for DummyAclProvider {
    async fn repo_acl(&self, _name: &str) -> Result<BoxPermissionChecker> {
        Ok(Box::new(AlwaysAllow))
    }

    async fn repo_region_acl(&self, _name: &str) -> Result<BoxPermissionChecker> {
        Ok(Box::new(AlwaysAllow))
    }

    async fn tier_acl(&self, name: &str) -> Result<BoxPermissionChecker> {
        Ok(Box::new(AlwaysAllow))
    }

    async fn group(&self, name: &str) -> Result<BoxMembershipChecker> {
        Ok(Box::new(NeverMember))
    }

    async fn admin_group(&self) -> Result<BoxMembershipChecker> {
        Ok(Box::new(NeverMember))
    }

    async fn reviewers_group(&self) -> Result<BoxMembershipChecker> {
        Ok(Box::new(AlwaysMember))
    }
}
