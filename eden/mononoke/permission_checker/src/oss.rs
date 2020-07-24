/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Result};
use fbinit::FacebookInit;
use openssl::x509::X509;

use crate::checker::{BoxPermissionChecker, PermissionCheckerBuilder};
use crate::identity::{MononokeIdentity, MononokeIdentitySet};
use crate::membership::{BoxMembershipChecker, MembershipCheckerBuilder};

impl MononokeIdentity {
    pub fn reviewer_identities(_username: &str) -> MononokeIdentitySet {
        MononokeIdentitySet::new()
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

impl PermissionCheckerBuilder {
    pub async fn acl_for_repo(_fb: FacebookInit, _name: &str) -> Result<BoxPermissionChecker> {
        Ok(Self::always_allow())
    }

    pub async fn acl_for_tier(_fb: FacebookInit, _name: &str) -> Result<BoxPermissionChecker> {
        Ok(Self::always_allow())
    }
}

impl MembershipCheckerBuilder {
    pub async fn for_reviewers_group(_fb: FacebookInit) -> Result<BoxMembershipChecker> {
        Ok(Self::always_member())
    }
}
