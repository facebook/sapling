/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::future::Shared;
use land_service_if::server::LandService;
use land_service_if::services::land_service::LandChangesetsExn;
use land_service_if::types::*;
use mononoke_api::CoreContext;
use srserver::RequestContext;
use tunables::tunables;

use crate::errors;
use crate::factory::Factory;
use crate::land_changeset_object::LandChangesetObject;
use crate::worker;
use crate::worker::EnqueueSender;

#[derive(Clone)]
pub(crate) struct LandServiceImpl {
    factory: Factory,
    #[allow(dead_code)]
    enqueue_entry_sender: Arc<EnqueueSender>,
    #[allow(dead_code)]
    ensure_worker_scheduled: Shared<BoxFuture<'static, ()>>,
}

pub(crate) struct LandServiceThriftImpl(LandServiceImpl);

impl LandServiceImpl {
    pub fn new(factory: Factory) -> Self {
        let (sender, ensure_worker_scheduled) = worker::setup_worker();
        Self {
            factory,
            enqueue_entry_sender: Arc::new(sender),
            ensure_worker_scheduled,
        }
    }

    pub(crate) fn thrift_server(&self) -> LandServiceThriftImpl {
        LandServiceThriftImpl(self.clone())
    }
}

#[async_trait]
impl LandService for LandServiceThriftImpl {
    type RequestContext = RequestContext;

    async fn land_changesets(
        &self,
        req_ctxt: &RequestContext,
        land_changesets: LandChangesetRequest,
    ) -> Result<LandChangesetsResponse, LandChangesetsExn> {
        let ctx: CoreContext = self
            .0
            .factory
            .create_ctx("land_changesets", req_ctxt)
            .await?;

        // Create an object with all required info to process a request
        let land_changeset_object = LandChangesetObject::new(
            self.0.factory.mononoke.clone(),
            self.0.factory.identity.clone(),
            ctx,
            land_changesets.clone(),
        );

        if tunables().get_batching_to_land_service() {
            self.0.ensure_worker_scheduled.clone().await;

            let (sender, receiver) = oneshot::channel();

            // Enqueue new entry
            self.0
                .enqueue_entry_sender
                .unbounded_send((sender, land_changeset_object))
                .map_err(|e| errors::internal_error(&e))?;

            return Ok(receiver.await.map_err(|e| errors::internal_error(&e))??);
        }

        Ok(worker::impl_land_changesets(
            land_changeset_object.mononoke,
            land_changeset_object.identity,
            land_changeset_object.ctx,
            land_changeset_object.request,
        )
        .await?)
    }
}
