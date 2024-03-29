/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::panic::RefUnwindSafe;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;

use crate::MononokeIdentitySet;

pub type ArcPermissionChecker = Arc<dyn PermissionChecker + Send + Sync + RefUnwindSafe + 'static>;
pub type BoxPermissionChecker = Box<dyn PermissionChecker + Send + Sync + RefUnwindSafe + 'static>;

#[async_trait]
pub trait PermissionChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, actions: &[&str]) -> bool;
}

pub struct PermissionCheckerBuilder {
    pub(crate) checkers: Vec<BoxPermissionChecker>,
}

impl PermissionCheckerBuilder {
    pub fn new() -> PermissionCheckerBuilder {
        PermissionCheckerBuilder {
            checkers: Vec::new(),
        }
    }

    pub fn allow(mut self, acl: BoxPermissionChecker) -> PermissionCheckerBuilder {
        self.checkers.push(acl);
        self
    }

    pub fn allow_all(mut self) -> PermissionCheckerBuilder {
        self.checkers.push(Box::new(AlwaysAllow));
        self
    }

    pub fn allow_allowlist(mut self, allowlist: MononokeIdentitySet) -> PermissionCheckerBuilder {
        self.checkers.push(Box::new(AllowlistChecker { allowlist }));
        self
    }

    pub fn build(mut self) -> BoxPermissionChecker {
        if self.checkers.len() <= 1 {
            match self.checkers.pop() {
                None => Box::new(AlwaysReject),
                Some(checker) => checker,
            }
        } else {
            Box::new(UnionPermissionChecker {
                checkers: self.checkers,
            })
        }
    }
}

pub(crate) struct AlwaysAllow;

#[async_trait]
impl PermissionChecker for AlwaysAllow {
    async fn check_set(&self, _accessors: &MononokeIdentitySet, _actions: &[&str]) -> bool {
        true
    }
}

pub(crate) struct AlwaysReject;

#[async_trait]
impl PermissionChecker for AlwaysReject {
    async fn check_set(&self, _accessors: &MononokeIdentitySet, _actions: &[&str]) -> bool {
        false
    }
}

pub(crate) struct AllowlistChecker {
    allowlist: MononokeIdentitySet,
}

#[async_trait]
impl PermissionChecker for AllowlistChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, _actions: &[&str]) -> bool {
        !self.allowlist.is_disjoint(accessors)
    }
}

struct UnionPermissionChecker {
    checkers: Vec<BoxPermissionChecker>,
}

#[async_trait]
impl PermissionChecker for UnionPermissionChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, actions: &[&str]) -> bool {
        // Check all checkers in parallel.
        let mut checks: FuturesUnordered<_> = self
            .checkers
            .iter()
            .map(|checker| async { checker.check_set(accessors, actions).await })
            .collect();

        while let Some(check_result) = checks.next().await {
            if check_result {
                // Return true as soon as any checker says access is permitted.
                return true;
            }
        }

        return false;
    }
}
