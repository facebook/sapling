/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Error;
use clap::ArgEnum;
use futures_stats::FutureStats;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;
use strum_macros::AsRefStr;
use strum_macros::Display;
use strum_macros::EnumString;
use strum_macros::EnumVariantNames;
use time_ext::DurationExt;

use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::OverwriteStatus;
use context::PerfCounters;
use metaconfig_types::BlobstoreId;
use tunables::tunables;

const SLOW_REQUEST_THRESHOLD: Duration = Duration::from_secs(5);

const BLOBSTORE_ID: &str = "blobstore_id";
const BLOBSTORE_TYPE: &str = "blobstore_type";
const COMPLETION_TIME: &str = "completion_time";
const ERROR: &str = "error";
const KEY: &str = "key";
const OPERATION: &str = "operation";
const QUEUE: &str = "queue";
const SESSION: &str = "session";
const SIZE: &str = "size";
const WRITE_ORDER: &str = "write_order";
/// Was the blob found during the get/is_present operations?
const BLOB_PRESENT: &str = "blob_present";

const OVERWRITE_STATUS: &str = "overwrite_status";

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    Hash,
    Display,
    AsRefStr,
    EnumString,
    EnumVariantNames,
    ArgEnum
)]
#[strum(serialize_all = "kebab_case")]
pub enum OperationType {
    Get,
    Put,
    ScrubGet,
    IsPresent,
    Link,
    Unlink,
    Enumerate,
}

impl From<OperationType> for ScubaValue {
    fn from(value: OperationType) -> ScubaValue {
        ScubaValue::from(value.as_ref())
    }
}

pub fn add_completion_time(
    scuba: &mut MononokeScubaSampleBuilder,
    session: &str,
    stats: FutureStats,
) {
    scuba.add(COMPLETION_TIME, stats.completion_time.as_micros_unchecked());
    if stats.completion_time >= SLOW_REQUEST_THRESHOLD {
        scuba.add(SESSION, session);
    }
}

fn add_common_values(
    scuba: &mut MononokeScubaSampleBuilder,
    pc: &PerfCounters,
    key: &str,
    session: &str,
    stats: FutureStats,
    operation: OperationType,
    blobstore_id: Option<BlobstoreId>,
    blobstore_type: impl ToString,
) {
    scuba
        .add(KEY, key)
        .add(OPERATION, operation)
        .add(BLOBSTORE_TYPE, blobstore_type.to_string());

    pc.insert_nonzero_perf_counters(scuba);

    if let Some(blobstore_id) = blobstore_id {
        scuba.add(BLOBSTORE_ID, blobstore_id);
    }

    add_completion_time(scuba, session, stats);
}

pub fn record_get_stats(
    scuba: &mut MononokeScubaSampleBuilder,
    pc: &PerfCounters,
    stats: FutureStats,
    result: Result<&Option<BlobstoreGetData>, &Error>,
    key: &str,
    session: &str,
    operation: OperationType,
    blobstore_id: Option<BlobstoreId>,
    blobstore_type: impl ToString,
) {
    add_common_values(
        scuba,
        pc,
        key,
        session,
        stats,
        operation,
        blobstore_id,
        blobstore_type,
    );

    match result {
        Ok(Some(data)) => {
            let size = data.as_bytes().len();
            let size_logging_threshold = tunables().get_blobstore_read_size_logging_threshold();
            if size_logging_threshold > 0 && size > size_logging_threshold as usize {
                scuba.unsampled();
            }
            scuba.add(SIZE, size);
            scuba.add(BLOB_PRESENT, true);
        }
        Err(error) => {
            // Always log errors
            scuba.unsampled();
            scuba.add(ERROR, format!("{:#}", error));
        }
        Ok(None) => {
            scuba.add(BLOB_PRESENT, false);
        }
    }

    scuba.log();
}

pub fn record_is_present_stats(
    scuba: &mut MononokeScubaSampleBuilder,
    pc: &PerfCounters,
    stats: FutureStats,
    result: Result<&BlobstoreIsPresent, &Error>,
    key: &str,
    session: &str,
    blobstore_id: Option<BlobstoreId>,
    blobstore_type: impl ToString,
) {
    add_common_values(
        scuba,
        pc,
        key,
        session,
        stats,
        OperationType::IsPresent,
        blobstore_id,
        blobstore_type,
    );

    match result {
        Ok(BlobstoreIsPresent::Present) => {
            scuba.add(BLOB_PRESENT, true);
        }
        Ok(BlobstoreIsPresent::Absent) => {
            scuba.add(BLOB_PRESENT, false);
        }
        Ok(BlobstoreIsPresent::ProbablyNotPresent(error)) => {
            // Always log errors
            scuba.unsampled();
            scuba.add(BLOB_PRESENT, false);
            scuba.add(ERROR, format!("{:#}", error));
        }
        Err(error) => {
            scuba.unsampled();
            scuba.add(ERROR, format!("{:#}", error));
        }
    }

    scuba.log();
}

pub fn record_put_stats(
    scuba: &mut MononokeScubaSampleBuilder,
    pc: &PerfCounters,
    stats: FutureStats,
    result: Result<&OverwriteStatus, &Error>,
    key: &str,
    session: &str,
    size: usize,
    blobstore_id: Option<BlobstoreId>,
    blobstore_type: impl ToString,
    write_order: Option<usize>,
) {
    add_common_values(
        scuba,
        pc,
        key,
        session,
        stats,
        OperationType::Put,
        blobstore_id,
        blobstore_type,
    );
    scuba.add(SIZE, size);

    match result {
        Ok(overwrite_status) => {
            scuba.add(OVERWRITE_STATUS, overwrite_status.as_ref());
            if let Some(write_order) = write_order {
                scuba.add(WRITE_ORDER, write_order);
            }
        }
        Err(error) => {
            scuba.add(ERROR, format!("{:#}", error));
        }
    };

    scuba.log();
}

pub fn record_queue_stats(
    scuba: &mut MononokeScubaSampleBuilder,
    pc: &PerfCounters,
    stats: FutureStats,
    result: Result<&(), &Error>,
    key: &str,
    session: &str,
    operation: OperationType,
    blobstore_id: Option<BlobstoreId>,
    blobstore_type: impl ToString,
    queue: &str,
) {
    add_common_values(
        scuba,
        pc,
        key,
        session,
        stats,
        operation,
        blobstore_id,
        blobstore_type,
    );

    scuba.add(QUEUE, queue);

    if let Err(error) = result {
        scuba.add(ERROR, format!("{:#}", error));
    }

    scuba.log();
}
