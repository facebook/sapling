/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use std::panic::RefUnwindSafe;
use std::sync::Arc;

use crate::MononokeIdentitySet;

pub type ArcMembershipChecker = Arc<dyn MembershipChecker + Send + Sync + RefUnwindSafe + 'static>;
pub type BoxMembershipChecker = Box<dyn MembershipChecker + Send + Sync + RefUnwindSafe + 'static>;

#[async_trait]
pub trait MembershipChecker {
    async fn is_member(&self, identities: &MononokeIdentitySet) -> Result<bool>;
}

pub struct MembershipCheckerBuilder {}
impl MembershipCheckerBuilder {
    pub fn always_member() -> BoxMembershipChecker {
        Box::new(AlwaysMember {})
    }

    pub fn never_member() -> BoxMembershipChecker {
        Box::new(NeverMember {})
    }

    pub fn whitelist_checker(whitelist: MononokeIdentitySet) -> BoxMembershipChecker {
        Box::new(WhitelistChecker { whitelist })
    }
}

struct AlwaysMember {}

#[async_trait]
impl MembershipChecker for AlwaysMember {
    async fn is_member(&self, _identities: &MononokeIdentitySet) -> Result<bool> {
        Ok(true)
    }
}

struct NeverMember {}

#[async_trait]
impl MembershipChecker for NeverMember {
    async fn is_member(&self, _identities: &MononokeIdentitySet) -> Result<bool> {
        Ok(false)
    }
}

struct WhitelistChecker {
    whitelist: MononokeIdentitySet,
}

#[async_trait]
impl MembershipChecker for WhitelistChecker {
    async fn is_member(&self, identities: &MononokeIdentitySet) -> Result<bool> {
        Ok(!self.whitelist.is_disjoint(identities))
    }
}
