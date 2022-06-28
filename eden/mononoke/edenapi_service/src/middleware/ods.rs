/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::State;
use gotham_ext::middleware::ClientIdentity;
use gotham_ext::middleware::Middleware;
use gotham_ext::middleware::PostResponseCallbacks;
use hyper::Body;
use hyper::Response;
use hyper::StatusCode;
use stats::prelude::*;

use crate::handlers::EdenApiMethod;
use crate::handlers::HandlerInfo;

define_stats! {
    prefix = "mononoke.edenapi.request";
    requests: dynamic_timeseries("{}.requests", (method: String); Rate, Sum),
    success: dynamic_timeseries("{}.success", (method: String); Rate, Sum),
    failure_4xx: dynamic_timeseries("{}.failure_4xx", (method: String); Rate, Sum),
    failure_5xx: dynamic_timeseries("{}.failure_5xx", (method: String); Rate, Sum),
    response_bytes_sent: dynamic_histogram("{}.response_bytes_sent", (method: String); 1_500_000, 0, 150_000_000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    capabilities_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    files_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    files2_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    trees_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    history_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    commit_location_to_hash_duration_ms: histogram(10, 0, 500, Average, Sum, Count; P 50; P 75; P 95; P 99),
    commit_hash_to_location_duration_ms: histogram(10, 0, 500, Average, Sum, Count; P 50; P 75; P 95; P 99),
    commit_revlog_data_duration_ms: histogram(10, 0, 500, Average, Sum, Count; P 50; P 75; P 95; P 99),
    commit_hash_lookup_duration_ms: histogram(10, 0, 500, Average, Sum, Count; P 50; P 75; P 95; P 99),
    clone_duration: dynamic_histogram("{}.clone_data_ms", (repo: String); 10, 0, 500, Average, Sum, Count; P 50; P 75; P 95; P 99),
    bookmarks_duration_ms: histogram(10, 0, 500, Average, Sum, Count; P 50; P 75; P 95; P 99),
    set_bookmark_duration_ms: histogram(10, 0, 500, Average, Sum, Count; P 50; P 75; P 95; P 99),
    land_stack_duration_ms: histogram(10, 0, 500, Average, Sum, Count; P 50; P 75; P 95; P 99),
    lookup_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    upload_file_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    pull_fast_forward_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    pull_lazy_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    upload_hg_filenodes_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    upload_trees_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    upload_hg_changesets_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    upload_bonsai_changeset_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    ephemeral_prepare_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    fetch_snapshot_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    commit_graph_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    download_file_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    commit_mutations_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
    commit_translate_id_duration_ms: histogram(100, 0, 5000, Average, Sum, Count; P 50; P 75; P 95; P 99),
}

fn log_stats(state: &mut State, status: StatusCode) -> Option<()> {
    // Proxygen can be configured to periodically send a preconfigured set of
    // requests to check server health. These requests will look like ordinary
    // user requests, but should be filtered out of the server's metrics.
    match state.try_borrow::<ClientIdentity>() {
        Some(id) if id.is_proxygen_test_identity() => return None,
        _ => {}
    }

    let hander_info = state.try_borrow::<HandlerInfo>()?;
    let method = hander_info.method?;
    let repo = hander_info.repo.clone()?;

    let callbacks = state.try_borrow_mut::<PostResponseCallbacks>()?;

    callbacks.add(move |info| {
        if let Some(duration) = info.duration {
            let dur_ms = duration.as_millis() as i64;

            use EdenApiMethod::*;
            match method {
                Capabilities => STATS::capabilities_duration_ms.add_value(dur_ms),
                Files => STATS::files_duration_ms.add_value(dur_ms),
                Files2 => STATS::files2_duration_ms.add_value(dur_ms),
                Trees => STATS::trees_duration_ms.add_value(dur_ms),
                History => STATS::history_duration_ms.add_value(dur_ms),
                CommitLocationToHash => {
                    STATS::commit_location_to_hash_duration_ms.add_value(dur_ms)
                }
                CommitHashToLocation => {
                    STATS::commit_hash_to_location_duration_ms.add_value(dur_ms)
                }
                CommitRevlogData => STATS::commit_revlog_data_duration_ms.add_value(dur_ms),
                CommitHashLookup => STATS::commit_hash_lookup_duration_ms.add_value(dur_ms),
                Clone => STATS::clone_duration.add_value(dur_ms, (repo,)),
                Bookmarks => STATS::bookmarks_duration_ms.add_value(dur_ms),
                SetBookmark => STATS::set_bookmark_duration_ms.add_value(dur_ms),
                LandStack => STATS::land_stack_duration_ms.add_value(dur_ms),
                Lookup => STATS::lookup_duration_ms.add_value(dur_ms),
                UploadFile => STATS::upload_file_duration_ms.add_value(dur_ms),
                PullFastForwardMaster => STATS::pull_fast_forward_duration_ms.add_value(dur_ms),
                PullLazy => STATS::pull_lazy_duration_ms.add_value(dur_ms),
                UploadHgFilenodes => STATS::upload_hg_filenodes_duration_ms.add_value(dur_ms),
                UploadTrees => STATS::upload_trees_duration_ms.add_value(dur_ms),
                UploadHgChangesets => STATS::upload_hg_changesets_duration_ms.add_value(dur_ms),
                UploadBonsaiChangeset => {
                    STATS::upload_bonsai_changeset_duration_ms.add_value(dur_ms)
                }
                EphemeralPrepare => STATS::ephemeral_prepare_duration_ms.add_value(dur_ms),
                FetchSnapshot => STATS::fetch_snapshot_duration_ms.add_value(dur_ms),
                CommitGraph => STATS::commit_graph_duration_ms.add_value(dur_ms),
                DownloadFile => STATS::download_file_duration_ms.add_value(dur_ms),
                CommitMutations => STATS::commit_mutations_duration_ms.add_value(dur_ms),
                CommitTranslateId => STATS::commit_translate_id_duration_ms.add_value(dur_ms),
            }
        }

        let method = method.to_string();
        STATS::requests.add_value(1, (method.clone(),));

        if status.is_success() {
            STATS::success.add_value(1, (method.clone(),));
        } else if status.is_client_error() {
            STATS::failure_4xx.add_value(1, (method.clone(),));
        } else if status.is_server_error() {
            STATS::failure_5xx.add_value(1, (method.clone(),));
        }

        if let Some(response_bytes_sent) = info.meta.as_ref().map(|m| m.body().bytes_sent) {
            STATS::response_bytes_sent.add_value(response_bytes_sent as i64, (method,))
        }
    });

    Some(())
}

pub struct OdsMiddleware;

impl OdsMiddleware {
    pub fn new() -> Self {
        OdsMiddleware
    }
}

#[async_trait::async_trait]
impl Middleware for OdsMiddleware {
    async fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        log_stats(state, response.status());
    }
}
