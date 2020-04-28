/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

use crate::MononokeIdentitySet;

pub type ArcPermissionChecker = Arc<dyn PermissionChecker + Send + Sync + 'static>;
pub type BoxPermissionChecker = Box<dyn PermissionChecker + Send + Sync + 'static>;

#[async_trait]
pub trait PermissionChecker {
    async fn check_set(&self, accessors: &MononokeIdentitySet, actions: &[&str]) -> Result<bool>;
}

#[async_trait]
impl<T> PermissionChecker for Box<T>
where
    T: PermissionChecker + ?Sized + Send + Sync,
{
    async fn check_set(&self, accessors: &MononokeIdentitySet, actions: &[&str]) -> Result<bool> {
        (*self).check_set(accessors, actions).await
    }
}

#[async_trait]
impl<T> PermissionChecker for Arc<T>
where
    T: PermissionChecker + ?Sized + Send + Sync,
{
    async fn check_set(&self, accessors: &MononokeIdentitySet, actions: &[&str]) -> Result<bool> {
        (*self).check_set(accessors, actions).await
    }
}

pub struct PermissionCheckerBuilder {}
impl PermissionCheckerBuilder {
    pub fn always_allow() -> BoxPermissionChecker {
        Box::new(AlwaysAllow {})
    }

    pub fn always_reject() -> BoxPermissionChecker {
        Box::new(AlwaysReject {})
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
