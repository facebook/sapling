/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use fbinit::FacebookInit;
use identity::Identity;
use land_service_if::server::LandService;
use land_service_if::services::land_service::LandChangesetsExn;
use land_service_if::types::*;
use login_objects_thrift::EnvironmentType;
use metaconfig_types::CommonConfig;
use metadata::Metadata;
use mononoke_api::CoreContext;
use mononoke_api::Mononoke;
use mononoke_api::SessionContainer;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;
use slog::Logger;
use srserver::RequestContext;
use stats::prelude::*;
use tunables::tunables;

use crate::errors;
use crate::errors::LandChangesetsError;
use crate::land_changeset_object::LandChangesetObject;
use crate::worker;

const FORWARDED_IDENTITIES_HEADER: &str = "scm_forwarded_identities";
const FORWARDED_CLIENT_IP_HEADER: &str = "scm_forwarded_client_ip";
const FORWARDED_CLIENT_DEBUG_HEADER: &str = "scm_forwarded_client_debug";
const FORWARDED_OTHER_CATS_HEADER: &str = "scm_forwarded_other_cats";

define_stats! {
    prefix = "mononoke.land_service";
    total_request_start: timeseries(Rate, Sum),
    total_request_success: timeseries(Rate, Sum),
    total_request_internal_failure: timeseries(Rate, Sum),
    total_request_canceled: timeseries(Rate, Sum),

    // Duration per changesets landed
    method_completion_time_ms: dynamic_histogram("method.{}.completion_time_ms", (method: String); 10, 0, 1_000, Average, Sum, Count; P 5; P 50 ; P 90),
}

#[derive(Clone)]
pub(crate) struct LandServiceImpl {
    pub(crate) fb: FacebookInit,
    pub(crate) logger: Logger,
    pub(crate) scuba_builder: MononokeScubaSampleBuilder,
    pub(crate) mononoke: Arc<Mononoke>,
    pub(crate) scribe: Scribe,
    pub(crate) identity: Identity,
}

pub(crate) struct LandServiceThriftImpl(LandServiceImpl);

impl LandServiceImpl {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        mononoke: Arc<Mononoke>,
        mut scuba_builder: MononokeScubaSampleBuilder,
        scribe: Scribe,
        common_config: &CommonConfig,
    ) -> Self {
        scuba_builder.add_common_server_data();

        Self {
            fb,
            logger,
            mononoke,
            scuba_builder,
            scribe,
            identity: Identity::new(
                common_config.internal_identity.id_type.as_str(),
                common_config.internal_identity.id_data.as_str(),
            ),
        }
    }

    pub(crate) fn thrift_server(&self) -> LandServiceThriftImpl {
        LandServiceThriftImpl(self.clone())
    }

    // Create context from given name and request context
    pub(crate) async fn create_ctx(
        &self,
        name: &str,
        req_ctxt: &RequestContext,
    ) -> Result<CoreContext, LandChangesetsError> {
        let session = self.create_session(req_ctxt).await?;
        let identities = session.metadata().identities();
        let scuba = self.create_scuba(name, req_ctxt, identities)?;
        let ctx = session.new_context_with_scribe(self.logger.clone(), scuba, self.scribe.clone());
        Ok(ctx)
    }

    /// Create and configure a scuba sample builder for a request.
    fn create_scuba(
        &self,
        name: &str,
        req_ctxt: &RequestContext,
        identities: &MononokeIdentitySet,
    ) -> Result<MononokeScubaSampleBuilder, LandChangesetsError> {
        let mut scuba = self.scuba_builder.clone().with_seq("seq");
        scuba.add("type", "thrift");
        scuba.add("method", name);

        const CLIENT_HEADERS: &[&str] = &[
            "client_id",
            "client_type",
            "client_correlator",
            "proxy_client_id",
        ];
        for &header in CLIENT_HEADERS.iter() {
            let value = req_ctxt.header(header)?;
            if let Some(value) = value {
                scuba.add(header, value);
            }
        }

        scuba.add(
            "identities",
            identities
                .iter()
                .map(|id| id.to_string())
                .collect::<ScubaValue>(),
        );

        Ok(scuba)
    }

    async fn create_metadata(
        &self,
        req_ctxt: &RequestContext,
    ) -> Result<Metadata, LandChangesetsError> {
        let header = |h: &str| {
            req_ctxt
                .header(h)
                .map_err(|e| errors::internal_error(e.as_ref()))
        };

        let tls_identities: MononokeIdentitySet = req_ctxt
            .identities()?
            .entries()
            .into_iter()
            .map(MononokeIdentity::from_identity_ref)
            .collect();

        // Get any valid CAT identities.
        let cats_identities: MononokeIdentitySet = req_ctxt
            .identities_cats(
                &self.identity,
                &[EnvironmentType::PROD, EnvironmentType::CORP],
            )?
            .entries()
            .into_iter()
            .map(MononokeIdentity::from_identity_ref)
            .collect();

        if let (Some(forwarded_identities), Some(forwarded_ip)) = (
            header(FORWARDED_IDENTITIES_HEADER)?,
            header(FORWARDED_CLIENT_IP_HEADER)?,
        ) {
            let mut header_identities: MononokeIdentitySet =
                serde_json::from_str(forwarded_identities.as_str())
                    .map_err(|e| errors::internal_error(&e))?;
            let client_ip = Some(
                forwarded_ip
                    .parse::<IpAddr>()
                    .map_err(|e| errors::internal_error(&e))?,
            );
            let client_debug = header(FORWARDED_CLIENT_DEBUG_HEADER)?.is_some();

            header_identities.extend(cats_identities.into_iter());
            let mut metadata =
                Metadata::new(None, header_identities, client_debug, client_ip).await;

            metadata.add_original_identities(tls_identities);

            if let Some(other_cats) = header(FORWARDED_OTHER_CATS_HEADER)? {
                metadata.add_raw_encoded_cats(other_cats);
            }

            return Ok(metadata);
        }

        Ok(Metadata::new(
            None,
            tls_identities.union(&cats_identities).cloned().collect(),
            false,
            None,
        )
        .await)
    }

    /// Create and configure the session container for a request.
    async fn create_session(
        &self,
        req_ctxt: &RequestContext,
    ) -> Result<SessionContainer, LandChangesetsError> {
        let metadata = self.create_metadata(req_ctxt).await?;
        let session = SessionContainer::builder(self.fb)
            .metadata(Arc::new(metadata))
            .build();
        Ok(session)
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
        let ctx: CoreContext = self.0.create_ctx("land_changesets", req_ctxt).await?;
        // Create an object with all required info to process a request
        // TODO: This object will be used later when requests are send to the queue
        let land_changeset_object = LandChangesetObject::new(
            self.0.mononoke.clone(),
            self.0.identity.clone(),
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
