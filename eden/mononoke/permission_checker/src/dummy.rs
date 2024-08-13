/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;

use crate::checker::AlwaysAllow;
use crate::checker::BoxPermissionChecker;
use crate::membership::AlwaysMember;
use crate::membership::BoxMembershipChecker;
use crate::membership::NeverMember;
use crate::provider::AclProvider;
use crate::MononokeIdentity;
pub struct DummyAclProvider;

impl DummyAclProvider {
    #[allow(unused)]
    pub fn new(_fb: FacebookInit) -> Arc<dyn AclProvider> {
        Arc::new(DummyAclProvider)
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

    async fn tier_acl(&self, _name: &str) -> Result<BoxPermissionChecker> {
        Ok(Box::new(AlwaysAllow))
    }

    async fn commitcloud_workspace_acl(
        &self,
        _name: &str,
        _create_with_owner: &Option<MononokeIdentity>,
    ) -> Result<Option<BoxPermissionChecker>> {
        Ok(Some(Box::new(AlwaysAllow)))
    }

    async fn group(&self, _name: &str) -> Result<BoxMembershipChecker> {
        Ok(Box::new(NeverMember))
    }

    async fn admin_group(&self) -> Result<BoxMembershipChecker> {
        Ok(Box::new(NeverMember))
    }

    async fn reviewers_group(&self) -> Result<BoxMembershipChecker> {
        Ok(Box::new(AlwaysMember))
    }
}
