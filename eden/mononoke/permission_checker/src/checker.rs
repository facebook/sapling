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

pub type ArcPermissionChecker = Arc<dyn PermissionChecker + Send + Sync + RefUnwindSafe + 'static>;
pub type BoxPermissionChecker = Box<dyn PermissionChecker + Send + Sync + RefUnwindSafe + 'static>;

#[async_trait]
pub trait PermissionChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, actions: &[&str]) -> Result<bool>;
}

pub struct PermissionCheckerBuilder {}
impl PermissionCheckerBuilder {
    pub fn always_allow() -> BoxPermissionChecker {
        Box::new(AlwaysAllow {})
    }

    pub fn always_reject() -> BoxPermissionChecker {
        Box::new(AlwaysReject {})
    }

    pub fn allowlist_checker(allowlist: MononokeIdentitySet) -> BoxPermissionChecker {
        Box::new(AllowlistChecker { allowlist })
    }
}

struct AlwaysAllow {}

#[async_trait]
impl PermissionChecker for AlwaysAllow {
    async fn check_set(&self, _accessors: &MononokeIdentitySet, _actions: &[&str]) -> Result<bool> {
        Ok(true)
    }
}

struct AlwaysReject {}

#[async_trait]
impl PermissionChecker for AlwaysReject {
    async fn check_set(&self, _accessors: &MononokeIdentitySet, _actions: &[&str]) -> Result<bool> {
        Ok(false)
    }
}

struct AllowlistChecker {
    allowlist: MononokeIdentitySet,
}

#[async_trait]
impl PermissionChecker for AllowlistChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, _actions: &[&str]) -> Result<bool> {
        Ok(!self.allowlist.is_disjoint(accessors))
    }
}
