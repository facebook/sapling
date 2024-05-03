/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod dag_items;
mod errors;
mod underived;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use context::CoreContext;
use derived_data_manager::DerivedDataManager;
use mononoke_types::RepositoryId;

pub use crate::dag_items::DagItemId;
pub use crate::dag_items::DagItemInfo;
pub use crate::dag_items::DerivationDagItem;
pub use crate::errors::InternalError;
#[cfg(fbcode_build)]
pub use crate::errors::ZeusWrapperError;
pub use crate::underived::build_underived_batched_graph;

#[facet::facet]
pub struct RepoDerivationQueues {
    configs_to_queues: HashMap<String, Arc<dyn DerivationQueue + Send + Sync>>,
}

impl RepoDerivationQueues {
    pub fn new(configs_to_queues: HashMap<String, Arc<dyn DerivationQueue + Send + Sync>>) -> Self {
        Self { configs_to_queues }
    }
    pub fn queue(&self, config_name: &str) -> Option<Arc<dyn DerivationQueue + Send + Sync>> {
        self.configs_to_queues.get(config_name).cloned()
    }
}

#[async_trait]
pub trait DerivationQueue {
    async fn enqueue(
        &self,
        ctx: &CoreContext,
        item: DerivationDagItem,
    ) -> Result<EnqueueResponse, InternalError>;

    async fn dequeue(
        &self,
        ctx: &CoreContext,
        limit: usize,
    ) -> Result<DequeueResponse, InternalError>;

    async fn ack(&self, ctx: &CoreContext, item_id: DagItemId) -> Result<(), InternalError>;

    async fn nack(&self, ctx: &CoreContext, item_id: DagItemId) -> Result<(), InternalError>;

    async fn extend_deriving_ttl(
        &self,
        ctx: &CoreContext,
        item_id: &DagItemId,
    ) -> Result<(), InternalError>;

    async fn watch_existing(
        &self,
        ctx: &CoreContext,
        item_id: DagItemId,
    ) -> Result<EnqueueResponse, InternalError>;

    fn derived_data_manager(&self) -> &DerivedDataManager;

    fn repo_id(&self) -> RepositoryId;
}

pub struct EnqueueResponse {
    pub(crate) watch:
        Box<dyn futures::future::Future<Output = anyhow::Result<bool>> + Unpin + Send + Sync>,
}

impl EnqueueResponse {
    pub fn new(
        watch: Box<
            dyn futures::future::Future<Output = anyhow::Result<bool>> + Unpin + Send + Sync,
        >,
    ) -> Self {
        Self { watch }
    }

    pub async fn is_derived(self) -> anyhow::Result<bool> {
        self.watch.await
    }
}

impl std::fmt::Debug for EnqueueResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Wrapper for Zeus watch")
    }
}

pub enum DequeueResponse {
    Empty {
        ready_queue_watch:
            Box<dyn futures::future::Future<Output = anyhow::Result<()>> + Unpin + Send + Sync>,
    },
    Items(Vec<DerivationDagItem>),
}
