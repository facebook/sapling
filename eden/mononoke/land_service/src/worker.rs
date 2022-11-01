/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use bookmarks::BookmarkName;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::future::Shared;
use futures::stream::StreamExt;
use futures::FutureExt;
use futures_ext::future::FbFutureExt;
use futures_stats::FutureStats;
use futures_stats::TimedFutureExt;
use identity::Identity;
use land_service_if::types::*;
use mononoke_api::CoreContext;
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_types::BonsaiChangeset;
use permission_checker::MononokeIdentity;
use pushrebase_client::LocalPushrebaseClient;
use pushrebase_client::PushrebaseClient;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_authorization::AuthorizationContext;
use stats::prelude::*;
use time_ext::DurationExt;

use crate::conversion_helpers;
use crate::errors;
use crate::errors::LandChangesetsError;
use crate::land_changeset_object::LandChangesetObject;
use crate::scuba_response::AddScubaResponse;

const LAND_CHANSET_BUFFER_SIZE: usize = 64;

define_stats! {
    prefix = "mononoke.land_service";
    total_request_start: timeseries(Rate, Sum),
    total_request_success: timeseries(Rate, Sum),
    total_request_internal_failure: timeseries(Rate, Sum),
    total_request_canceled: timeseries(Rate, Sum),

    // Duration per changesets landed
    method_completion_time_ms: dynamic_histogram("method.{}.completion_time_ms", (method: String); 10, 0, 1_000, Average, Sum, Count; P 5; P 50 ; P 90),
}

pub type EnqueueSender = mpsc::UnboundedSender<(
    oneshot::Sender<Result<LandChangesetsResponse, LandChangesetsError>>,
    LandChangesetObject,
)>;
pub fn setup_worker() -> (EnqueueSender, Shared<BoxFuture<'static, ()>>) {
    // The mpsc channel needed as a way to enqueue new requests while there is an
    // in-flight request.
    // - queue_sender will be used to add new requests to the queue (channel),
    // - receiver - to read a new batch of requests and land them.
    //
    // To notify the clients back that the request was successfully landed,
    // a oneshot channel is used. When the enqueued requests are processed, the clients
    // receive result of the operation:
    // error if something went wrong and nothing if it's ok.
    let (queue_sender, receiver) = mpsc::unbounded::<(
        oneshot::Sender<Result<LandChangesetsResponse, LandChangesetsError>>,
        LandChangesetObject,
    )>();
    let worker = async move {
        let enqueued_landed_changesets = receiver.ready_chunks(LAND_CHANSET_BUFFER_SIZE).for_each(
            move |batch /* (Sender, LandChangesetObject) */| async move {
                process_requests(batch).await;
            },
        );
        tokio::spawn(enqueued_landed_changesets);
    }
    .boxed()
    .shared();
    (queue_sender, worker)
}
async fn process_requests(
    requests: Vec<(
        oneshot::Sender<Result<LandChangesetsResponse, LandChangesetsError>>,
        LandChangesetObject,
    )>,
) {
    // TODO: Right now we are processing each request for the batch.
    // Next, we will process batches of the ones that fit together
    for (sender, request) in requests.into_iter() {
        match sender.send(
            impl_land_changesets(
                request.mononoke,
                request.identity,
                request.ctx,
                request.request,
            )
            .await,
        ) {
            Ok(_) => (),
            Err(_) => (),
        };
    }
}

//TODO: Once the batching is done, this method does not need to be public
pub async fn impl_land_changesets(
    mononoke: Arc<Mononoke>,
    identity: Identity,
    ctx: CoreContext,
    land_changesets: LandChangesetRequest,
) -> Result<LandChangesetsResponse, LandChangesetsError> {
    ctx.scuba().clone().log_with_msg("Request start", None);
    STATS::total_request_start.add_value(1);

    let (stats, res) = process_land_changesets_request(mononoke, identity, &ctx, land_changesets)
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

/// Create a RepoContext
async fn get_repo_context(
    mononoke: Arc<Mononoke>,
    repo_name: String,
    ctx: CoreContext,
    authz: AuthorizationContext,
) -> Result<RepoContext, LandChangesetsError> {
    Ok(mononoke
        .repo(ctx, &repo_name)
        .await?
        .ok_or_else(|| errors::internal_error(anyhow!(repo_name).as_ref()))?
        .with_authorization_context(authz)
        .build()
        .await?)
}

// Check for the scm_service_identity
fn assert_internal_identity(
    identity: Identity,
    repo: &RepoContext,
) -> Result<(), LandChangesetsError> {
    let original_identities = repo.ctx().metadata().original_identities();
    if !original_identities.map_or(false, |ids| {
        ids.contains(&MononokeIdentity::from_identity(&identity))
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
    mononoke: Arc<Mononoke>,
    identity: Identity,
    ctx: &CoreContext,
    land_changesets: LandChangesetRequest,
) -> Result<LandChangesetsResponse, LandChangesetsError> {
    let authz = AuthorizationContext::new(ctx);
    //TODO: Avoid using RepoContext, build a leaner Repo type if possible (T132600441)
    let repo: RepoContext = get_repo_context(
        mononoke,
        land_changesets.repo_name,
        ctx.clone(),
        authz.clone(),
    )
    .await?;

    assert_internal_identity(identity, &repo)?;

    let lca_hint: Arc<dyn LeastCommonAncestorsHint> = repo.skiplist_index_arc();

    let bookmark = BookmarkName::new(land_changesets.bookmark)?;
    let changesets: HashSet<BonsaiChangeset> =
        conversion_helpers::convert_bonsai_changesets(land_changesets.changesets, ctx, &repo)
            .await?;
    let pushvars =
        conversion_helpers::convert_pushvars(land_changesets.pushvars.unwrap_or_default());

    let cross_repo_push_source =
        conversion_helpers::convert_cross_repo_push_source(land_changesets.cross_repo_push_source)?;

    let bookmark_restrictions =
        conversion_helpers::convert_bookmark_restrictions(land_changesets.bookmark_restrictions)?;

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
            pushrebase_distance: conversion_helpers::convert_to_i64(outcome.pushrebase_distance.0)?,
            retry_num: conversion_helpers::convert_to_i64(outcome.retry_num.0)?,
            old_bookmark_value: outcome
                .old_bookmark_value
                .map(conversion_helpers::convert_changeset_id_to_vec_binary),
        },
    })
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
