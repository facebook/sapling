/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Logic for logging to mononoke_multiplex scuba table.
//! Maybe we should be using `LogBlob` here, but that might need
//! some modification there.

use anyhow::Result;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore_stats::BLOB_PRESENT;
use blobstore_stats::ERROR;
use blobstore_stats::KEY;
use blobstore_stats::OPERATION;
use blobstore_stats::OperationType;
use blobstore_stats::add_completion_time;
use context::CoreContext;
use futures_stats::FutureStats;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;
use scuba_ext::MononokeScubaSampleBuilder;

const MULTIPLEX_ID: &str = "multiplex_id";
const BLOB_SIZE: &str = "blob_size";
const SUCCESS: &str = "success";
const SYNC_QUEUE: &str = "mysql_sync_queue";
const BLOBSTORE_ID: &str = "blobstore_id";

fn record_scuba_common(
    mut ctx: CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    stats: FutureStats,
    operation: OperationType,
) {
    let pc = ctx.fork_perf_counters();
    pc.insert_nonzero_perf_counters(scuba);

    add_completion_time(scuba, ctx.metadata().session_id().as_str(), stats);

    scuba.add(KEY, key);
    scuba.add(OPERATION, operation);
    scuba.add(MULTIPLEX_ID, multiplex_id.clone());
    if let Some(client_info) = ctx.client_request_info() {
        scuba.add_client_request_info(client_info);
    }
    if let Some(fetch_cause) = ctx.metadata().fetch_cause() {
        scuba.add_fetch_cause(fetch_cause);
    }
}

pub fn record_put<T>(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    blob_size: usize,
    stats: FutureStats,
    result: &Result<T>,
) {
    let op = OperationType::Put;
    record_scuba_common(ctx.clone(), scuba, multiplex_id, key, stats, op);

    scuba.add(BLOB_SIZE, blob_size);

    if let Err(error) = result.as_ref() {
        scuba.unsampled();
        scuba.add(ERROR, format!("{:#}", error)).add(SUCCESS, false);
    } else {
        scuba.add(SUCCESS, true);
    }
    scuba.log();
}

pub fn record_unlink(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    stats: FutureStats,
    result: &Result<()>,
) {
    let op = OperationType::Unlink;
    record_scuba_common(ctx.clone(), scuba, multiplex_id, key, stats, op);

    if let Err(error) = result.as_ref() {
        scuba.unsampled();
        scuba.add(ERROR, format!("{:#}", error)).add(SUCCESS, false);
    } else {
        scuba.add(SUCCESS, true);
    }
    scuba.log();
}

pub fn record_get(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    stats: FutureStats,
    result: &Result<Option<(BlobstoreId, BlobstoreGetData)>>,
) {
    let op = OperationType::Get;
    record_scuba_common(ctx.clone(), scuba, multiplex_id, key, stats, op);

    match result.as_ref() {
        Err(error) => {
            scuba.unsampled();
            scuba.add(ERROR, format!("{:#}", error)).add(SUCCESS, false);
        }
        Ok(mb_blob) => {
            let blob_present = mb_blob.is_some();
            scuba.add(BLOB_PRESENT, blob_present).add(SUCCESS, true);

            if let Some((bs_id, blob)) = mb_blob.as_ref() {
                scuba.add(BLOBSTORE_ID, *bs_id);
                scuba.add(BLOB_SIZE, blob.len());
            }
        }
    }
    scuba.log();
}

pub fn record_is_present(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    stats: FutureStats,
    result: &Result<BlobstoreIsPresent>,
) {
    let op = OperationType::IsPresent;
    record_scuba_common(ctx.clone(), scuba, multiplex_id, key, stats, op);

    let outcome = result.as_ref().map(|is_present| match is_present {
        BlobstoreIsPresent::Present => Some(true),
        BlobstoreIsPresent::Absent => Some(false),
        BlobstoreIsPresent::ProbablyNotPresent(_) => None,
    });

    match outcome {
        Err(error) => {
            scuba.unsampled();
            scuba.add(ERROR, format!("{:#}", error)).add(SUCCESS, false);
        }
        Ok(is_present) => {
            if let Some(is_present) = is_present {
                scuba.add(BLOB_PRESENT, is_present).add(SUCCESS, true);
            }
        }
    }
    scuba.log();
}

pub fn record_queue_stats(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    key: &str,
    stats: FutureStats,
    blobstore_id: Option<BlobstoreId>,
    blobstore_type: String,
    result: Result<&(), &anyhow::Error>,
) {
    let pc = ctx.clone().fork_perf_counters();
    blobstore_stats::record_queue_stats(
        scuba,
        &pc,
        stats,
        result,
        key,
        ctx.metadata().session_id().as_str(),
        OperationType::Put,
        blobstore_id,
        blobstore_type,
        SYNC_QUEUE,
    );
}
