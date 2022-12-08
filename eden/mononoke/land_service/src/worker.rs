/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
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

    total_batch_success: timeseries(Rate, Sum),
    total_batch_failures: timeseries(Rate, Sum),

    // Duration per changesets landed with and without batches
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
    let mut changesets_batch: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut first_land_changeset_object_batched: Option<LandChangesetObject> = None;
    let mut backup_batch = Vec::new();

    for (sender, land_changeset_object) in requests.into_iter() {
        // If there is NO pushvars for a request, we batch it
        if land_changeset_object.request.pushvars.is_none() {
            if changesets_batch.is_empty() {
                first_land_changeset_object_batched = Some(land_changeset_object.clone());
            }
            changesets_batch.extend(land_changeset_object.request.changesets.clone());
            backup_batch.push((sender, land_changeset_object));
        //Otherwise, we just process it individually
        } else if let Err(err) =
            sender.send(impl_land_changesets(land_changeset_object.clone()).await)
        {
            let mut scuba = land_changeset_object.ctx.scuba().clone();
            scuba.log_with_msg(
                        "Failed to send individual response back without batching (i.e., request with pushvars)",
                        Some(format!("{:?}", err)),
                    );
        };
    }

    if let Some(mut land_changeset_object) = first_land_changeset_object_batched {
        land_changeset_object.request.changesets = changesets_batch;

        let (stats, result) = impl_land_changesets(land_changeset_object.clone())
            .timed()
            .await;
        log_batch_result(land_changeset_object.ctx, &stats, result, backup_batch).await;
        STATS::method_completion_time_ms.add_value(
            stats.completion_time.as_millis_unchecked() as i64,
            ("impl_land_changesets_with_batch".to_string(),),
        );
    }
}

//TODO: Once the batching is done, this method does not need to be public
pub async fn impl_land_changesets(
    land_changeset_object: LandChangesetObject,
) -> Result<LandChangesetsResponse, LandChangesetsError> {
    land_changeset_object
        .ctx
        .scuba()
        .clone()
        .log_with_msg("Request start", None);
    STATS::total_request_start.add_value(1);

    let (stats, res) = process_land_changesets_request(land_changeset_object.clone())
        .timed()
        .on_cancel_with_data(|stats| log_canceled(&land_changeset_object.ctx, &stats))
        .await;
    log_result(land_changeset_object.ctx, &stats, &res);
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
        .ok_or_else(|| {
            errors::internal_error(anyhow!("Not finding the repo: {}", repo_name).as_ref())
        })?
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
    land_changeset_object: LandChangesetObject,
) -> Result<LandChangesetsResponse, LandChangesetsError> {
    let LandChangesetObject {
        mononoke,
        identity,
        ctx,
        request,
    } = land_changeset_object;

    let authz = AuthorizationContext::new(&ctx);
    //TODO: Avoid using RepoContext, build a leaner Repo type if possible (T132600441)
    let repo: RepoContext =
        get_repo_context(mononoke, request.repo_name, ctx.clone(), authz.clone()).await?;

    assert_internal_identity(identity, &repo)?;

    let lca_hint: Arc<dyn LeastCommonAncestorsHint> = repo.skiplist_index_arc();

    let bookmark = BookmarkName::new(request.bookmark)?;
    let changesets: HashSet<BonsaiChangeset> =
        conversion_helpers::convert_bonsai_changesets(request.changesets, &ctx, &repo).await?;

    let pushvars = conversion_helpers::convert_pushvars(request.pushvars.unwrap_or_default());

    let cross_repo_push_source =
        conversion_helpers::convert_cross_repo_push_source(request.cross_repo_push_source)?;

    let bookmark_restrictions =
        conversion_helpers::convert_bookmark_restrictions(request.bookmark_restrictions)?;

    let outcome = LocalPushrebaseClient {
        ctx: &ctx,
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
        false, // Currently mononoke server logs the new commits
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

async fn log_batch_result(
    ctx: CoreContext,
    stats: &FutureStats,
    result: Result<LandChangesetsResponse, LandChangesetsError>,
    backup_batch: Vec<(
        oneshot::Sender<Result<LandChangesetsResponse, LandChangesetsError>>,
        LandChangesetObject,
    )>,
) {
    let mut scuba = ctx.scuba().clone();
    let batch_size = backup_batch.len();

    match result {
        Ok(ref response) => {
            // if batched request worked, send response back for each request
            for (sender, _) in backup_batch.into_iter() {
                if let Err(err) = sender.send(result.clone()) {
                    scuba.log_with_msg(
                        "Failed sending individual response back after batching completed",
                        Some(format!("{:?}", err)),
                    );
                };
            }
            response.add_scuba_response(&mut scuba);
            STATS::total_batch_success.add_value(1);
            scuba.log_with_msg(
                format!("Batching {} requests completed successfully", batch_size).as_str(),
                None,
            );
        }
        Err(err) => {
            STATS::total_batch_failures.add_value(1);
            scuba.log_with_msg(
                format!("Batching {} requests failed", batch_size).as_str(),
                None,
            );
            scuba.add("error batching", err.to_string());
            // if found error, process requests individually
            for (sender, land_changeset_object) in backup_batch.into_iter() {
                if let Err(err) = sender.send(impl_land_changesets(land_changeset_object).await) {
                    scuba.log_with_msg(
                        "Failed sending individual response back after batching failed",
                        Some(format!("{:?}", err)),
                    );
                };
            }
        }
    };

    ctx.perf_counters().insert_perf_counters(&mut scuba);
    scuba.add_future_stats(stats);
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
