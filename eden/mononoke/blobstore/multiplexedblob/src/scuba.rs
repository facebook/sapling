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
use blobstore_stats::add_completion_time;
pub use blobstore_stats::record_queue_stats;
use blobstore_stats::OperationType;
use blobstore_stats::BLOB_PRESENT;
use blobstore_stats::ERROR;
use blobstore_stats::KEY;
use blobstore_stats::OPERATION;
use context::CoreContext;
use futures_stats::FutureStats;
use metaconfig_types::MultiplexId;
use scuba_ext::MononokeScubaSampleBuilder;

const MULTIPLEX_ID: &str = "multiplex_id";
const BLOB_SIZE: &str = "blob_size";
const SUCCESS: &str = "success";

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
}

pub fn record_put(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    blob_size: usize,
    stats: FutureStats,
    result: &Result<()>,
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

pub fn record_get(
    ctx: &CoreContext,
    scuba: &mut MononokeScubaSampleBuilder,
    multiplex_id: &MultiplexId,
    key: &str,
    stats: FutureStats,
    result: &Result<Option<BlobstoreGetData>>,
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

            if let Some(blob) = mb_blob.as_ref() {
                let size = blob.as_bytes().len();
                scuba.add(BLOB_SIZE, size);
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
