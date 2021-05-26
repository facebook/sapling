/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use async_trait::async_trait;
use context::CoreContext;
use megarepo_error::MegarepoError;
use mononoke_api::{Mononoke, RepoContext};
use mononoke_types::RepositoryId;
use std::{convert::TryInto, sync::Arc};

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct SourceName(pub String);

#[async_trait]
pub trait MegarepoOp {
    fn mononoke(&self) -> &Arc<Mononoke>;

    async fn find_repo_by_id(
        &self,
        ctx: &CoreContext,
        repo_id: i64,
    ) -> Result<RepoContext, MegarepoError> {
        let target_repo_id = RepositoryId::new(repo_id.try_into().unwrap());
        let target_repo = self
            .mononoke()
            .repo_by_id(ctx.clone(), target_repo_id)
            .await
            .map_err(MegarepoError::internal)?
            .ok_or_else(|| MegarepoError::request(anyhow!("repo not found {}", target_repo_id)))?;
        Ok(target_repo)
    }
}
