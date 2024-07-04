/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;

mod store;
mod types;

pub use crate::store::SqlPushRedirection;
pub use crate::store::SqlPushRedirectionBuilder;
pub use crate::types::PushRedirectionEntry;
pub use crate::types::RowId;

#[facet::facet]
#[async_trait]
pub trait PushRedirection: Send + Sync {
    async fn set(&self, ctx: &CoreContext, draft_push: bool, public_push: bool) -> Result<()>;

    async fn get(&self, ctx: &CoreContext) -> Result<Option<PushRedirectionEntry>>;
}

#[derive(Clone)]
pub struct NoopPushRedirection {}

#[async_trait]
impl PushRedirection for NoopPushRedirection {
    async fn set(&self, _ctx: &CoreContext, _draft_push: bool, _public_push: bool) -> Result<()> {
        Ok(())
    }

    async fn get(&self, _ctx: &CoreContext) -> Result<Option<PushRedirectionEntry>> {
        Ok(None)
    }
}
