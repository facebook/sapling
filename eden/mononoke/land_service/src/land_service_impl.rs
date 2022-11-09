/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
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
use parking_lot::Mutex;
use srserver::RequestContext;
use tunables::tunables;

use crate::errors;
use crate::errors::LandChangesetsError;
use crate::factory::Factory;
use crate::land_changeset_object::LandChangesetObject;
use crate::worker;
use crate::worker::EnqueueSender;

#[derive(Clone)]
pub(crate) struct LandServiceImpl {
    factory: Factory,
    #[allow(dead_code)]
    repo_bookmark_map:
        Arc<Mutex<HashMap<RepoBookmarkKey, (EnqueueSender, Shared<BoxFuture<'static, ()>>)>>>,
}

pub(crate) struct LandServiceThriftImpl(LandServiceImpl);

impl LandServiceImpl {
    pub fn new(factory: Factory) -> Self {
        let repo_bookmark_map = Arc::new(Mutex::new(HashMap::new()));
        Self {
            factory,
            repo_bookmark_map,
        }
    }

    pub(crate) fn thrift_server(&self) -> LandServiceThriftImpl {
        LandServiceThriftImpl(self.clone())
    }
}

#[derive(Hash, PartialEq, Eq)]
struct RepoBookmarkKey {
    repo_name: String,
    bookmark: String,
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
            let (sender, receiver) =
                oneshot::channel::<Result<LandChangesetsResponse, LandChangesetsError>>();

            let serialized_key = RepoBookmarkKey {
                repo_name: land_changeset_object.request.repo_name.clone(),
                bookmark: land_changeset_object.request.bookmark.clone(),
            };

            let (worker_sender, worker_process_future) = {
                let mut repo_bookmark_map = self.0.repo_bookmark_map.lock();

                repo_bookmark_map
                    .entry(serialized_key)
                    .or_insert_with(worker::setup_worker)
                    .clone()
            };

            worker_sender
                .unbounded_send((sender, land_changeset_object.clone()))
                .map_err(|e| errors::internal_error(&e))?;

            worker_process_future.await;

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
