/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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

pub struct AlwaysMember;

impl AlwaysMember {
    pub fn new() -> BoxMembershipChecker {
        Box::new(AlwaysMember)
    }
}

#[async_trait]
impl MembershipChecker for AlwaysMember {
    async fn is_member(&self, _identities: &MononokeIdentitySet) -> Result<bool> {
        Ok(true)
    }
}

pub struct NeverMember;

impl NeverMember {
    pub fn new() -> BoxMembershipChecker {
        Box::new(NeverMember)
    }
}

#[async_trait]
impl MembershipChecker for NeverMember {
    async fn is_member(&self, _identities: &MononokeIdentitySet) -> Result<bool> {
        Ok(false)
    }
}

pub struct MemberAllowlist {
    allowlist: MononokeIdentitySet,
}

impl MemberAllowlist {
    pub fn new(allowlist: MononokeIdentitySet) -> MemberAllowlist {
        MemberAllowlist { allowlist }
    }
}

#[async_trait]
impl MembershipChecker for MemberAllowlist {
    async fn is_member(&self, identities: &MononokeIdentitySet) -> Result<bool> {
        Ok(!self.allowlist.is_disjoint(identities))
    }
}
