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

    pub fn whitelist_checker(whitelist: MononokeIdentitySet) -> BoxPermissionChecker {
        Box::new(WhitelistChecker { whitelist })
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

struct WhitelistChecker {
    whitelist: MononokeIdentitySet,
}

#[async_trait]
impl PermissionChecker for WhitelistChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, _actions: &[&str]) -> Result<bool> {
        Ok(!self.whitelist.is_disjoint(accessors))
    }
}

#[cfg(not(fbcode_build))]
mod r#impl {
    use super::*;

    use fbinit::FacebookInit;

    impl PermissionCheckerBuilder {
        pub async fn acl_for_repo(_fb: FacebookInit, _name: &str) -> Result<BoxPermissionChecker> {
            Ok(Self::always_allow())
        }

        pub async fn acl_for_tier(_fb: FacebookInit, _name: &str) -> Result<BoxPermissionChecker> {
            Ok(Self::always_allow())
        }
    }
}
