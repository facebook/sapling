/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use land_service_if::server::LandService;
use land_service_if::services::land_service::LandChangesetsExn;
use land_service_if::types::*;
use mononoke_api::CoreContext;
use srserver::RequestContext;
use tunables::tunables;

use crate::factory::Factory;
use crate::land_changeset_object::LandChangesetObject;
use crate::worker;

#[derive(Clone)]
pub(crate) struct LandServiceImpl {
    factory: Factory,
}

pub(crate) struct LandServiceThriftImpl(LandServiceImpl);

impl LandServiceImpl {
    pub fn new(factory: Factory) -> Self {
        Self { factory }
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
        // TODO: This object will be used later when requests are send to the queue
        let land_changeset_object = LandChangesetObject::new(
            self.0.factory.mononoke.clone(),
            self.0.factory.identity.clone(),
            ctx,
            land_changesets.clone(),
        );

        if tunables().get_batching_to_land_service() {
            todo!()
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
