/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use fbinit::FacebookInit;

use crate::checker::{BoxPermissionChecker, PermissionCheckerBuilder};
use crate::identity::{MononokeIdentity, MononokeIdentitySet};
use crate::membership::{BoxMembershipChecker, MembershipCheckerBuilder};

impl MononokeIdentity {
    pub fn reviewer_identities(_username: &str) -> MononokeIdentitySet {
        MononokeIdentitySet::new()
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
