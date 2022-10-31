/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use bookmarks::BookmarkName;
use fbinit::FacebookInit;
use futures_ext::future::FbFutureExt;
use futures_stats::FutureStats;
use futures_stats::TimedFutureExt;
use identity::Identity;
use land_service_if::server::LandService;
use land_service_if::services::land_service::LandChangesetsExn;
use land_service_if::types::*;
use login_objects_thrift::EnvironmentType;
use metaconfig_types::CommonConfig;
use metadata::Metadata;
use mononoke_api::CoreContext;
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_api::SessionContainer;
use mononoke_types::BonsaiChangeset;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use pushrebase_client::LocalPushrebaseClient;
use pushrebase_client::PushrebaseClient;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_authorization::AuthorizationContext;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;
use slog::Logger;
use srserver::RequestContext;
use stats::prelude::*;
use time_ext::DurationExt;
use tunables::tunables;

use crate::conversion_helpers;
use crate::errors;
use crate::errors::LandChangesetsError;
use crate::land_changeset_object::LandChangesetObject;
use crate::scuba_response::AddScubaResponse;

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

    async fn impl_land_changesets(
        &self,
        ctx: CoreContext,
        land_changesets: LandChangesetRequest,
    ) -> Result<LandChangesetsResponse, LandChangesetsError> {
        ctx.scuba().clone().log_with_msg("Request start", None);
        STATS::total_request_start.add_value(1);

        let (stats, res) = self
            .process_land_changesets_request(&ctx, land_changesets)
            .timed()
            .on_cancel_with_data(|stats| log_canceled(&ctx, &stats))
            .await;
        log_result(ctx, &stats, &res);
        STATS::method_completion_time_ms.add_value(
            stats.completion_time.as_millis_unchecked() as i64,
            ("impl_land_changesets".to_string(),),
        );
        res
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

    /// Create a RepoContext
    async fn get_repo_context(
        &self,
        repo_name: String,
        ctx: CoreContext,
        authz: AuthorizationContext,
    ) -> Result<RepoContext, LandChangesetsError> {
        Ok(self
            .mononoke
            .repo(ctx, &repo_name)
            .await?
            .ok_or_else(|| errors::internal_error(anyhow!(repo_name).as_ref()))?
            .with_authorization_context(authz)
            .build()
            .await?)
    }

    // Check for the scm_service_identity
    fn assert_internal_identity(&self, repo: &RepoContext) -> Result<(), LandChangesetsError> {
        let original_identities = repo.ctx().metadata().original_identities();
        if !original_identities.map_or(false, |ids| {
            ids.contains(&MononokeIdentity::from_identity(&self.identity))
        }) {
            return Err(errors::internal_error(
                anyhow!(
                    "Insufficient permissions, internal options only. Identities: {}",
                    original_identities
                        .map_or_else(|| "<none>".to_string(), permission_checker::pretty_print)
                )
                .as_ref(),
            )
            .into());
        }
        Ok(())
    }

    async fn process_land_changesets_request(
        &self,
        ctx: &CoreContext,
        land_changesets: LandChangesetRequest,
    ) -> Result<LandChangesetsResponse, LandChangesetsError> {
        let authz = AuthorizationContext::new(ctx);
        //TODO: Avoid using RepoContext, build a leaner Repo type if possible (T132600441)
        let repo: RepoContext = self
            .get_repo_context(land_changesets.repo_name, ctx.clone(), authz.clone())
            .await?;

        self.assert_internal_identity(&repo)?;

        let lca_hint: Arc<dyn LeastCommonAncestorsHint> = repo.skiplist_index_arc();

        let bookmark = BookmarkName::new(land_changesets.bookmark)?;
        let changesets: HashSet<BonsaiChangeset> =
            conversion_helpers::convert_bonsai_changesets(land_changesets.changesets, ctx, &repo)
                .await?;
        let pushvars =
            conversion_helpers::convert_pushvars(land_changesets.pushvars.unwrap_or_default());

        let cross_repo_push_source = conversion_helpers::convert_cross_repo_push_source(
            land_changesets.cross_repo_push_source,
        )?;

        let bookmark_restrictions = conversion_helpers::convert_bookmark_restrictions(
            land_changesets.bookmark_restrictions,
        )?;

        let outcome = LocalPushrebaseClient {
            ctx,
            authz: &authz,
            repo: &repo.inner_repo().clone(),
            lca_hint: &lca_hint,
            hook_manager: repo.hook_manager().as_ref(),
        }
        .pushrebase(
            &bookmark,
            changesets,
            Some(&pushvars),
            cross_repo_push_source,
            bookmark_restrictions,
        )
        .await?;

        Ok(LandChangesetsResponse {
            pushrebase_outcome: PushrebaseOutcome {
                head: outcome.head.as_ref().to_vec(),
                rebased_changesets: outcome
                    .rebased_changesets
                    .into_iter()
                    .map(|rebased_changeset| {
                        conversion_helpers::convert_rebased_changesets_into_pairs(rebased_changeset)
                    })
                    .collect(),
                pushrebase_distance: conversion_helpers::convert_to_i64(
                    outcome.pushrebase_distance.0,
                )?,
                retry_num: conversion_helpers::convert_to_i64(outcome.retry_num.0)?,
                old_bookmark_value: outcome
                    .old_bookmark_value
                    .map(conversion_helpers::convert_changeset_id_to_vec_binary),
            },
        })
    }
}

fn log_result<T: AddScubaResponse>(
    ctx: CoreContext,
    stats: &FutureStats,
    result: &Result<T, LandChangesetsError>,
) {
    let mut scuba = ctx.scuba().clone();

    match result {
        Ok(response) => {
            response.add_scuba_response(&mut scuba);
            STATS::total_request_success.add_value(1);
            scuba.add("status", "SUCCESS");
        }
        Err(err) => {
            STATS::total_request_internal_failure.add_value(1);
            scuba.add("status", "INTERNAL_ERROR");
            scuba.add("error", err.to_string());
        }
    };

    ctx.perf_counters().insert_perf_counters(&mut scuba);
    scuba.add_future_stats(stats);
    scuba.log_with_msg("Request complete", None);
}

fn log_canceled(ctx: &CoreContext, stats: &FutureStats) {
    STATS::total_request_canceled.add_value(1);
    let mut scuba = ctx.scuba().clone();
    ctx.perf_counters().insert_perf_counters(&mut scuba);
    scuba.add_future_stats(stats);
    scuba.add("status", "CANCELED");
    scuba.log_with_msg("Request canceled", None);
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

        if tunables().get_batching_to_land_service() {
            // Create an object with all required info to process a request
            // TODO: This object will be used later when requests are send to the queue
            let _land_changeset_object = LandChangesetObject::new(
                self.0.mononoke.clone(),
                self.0.identity.clone(),
                ctx,
                land_changesets.clone(),
            );
            todo!()
        }

        Ok(self.0.impl_land_changesets(ctx, land_changesets).await?)
    }
}
