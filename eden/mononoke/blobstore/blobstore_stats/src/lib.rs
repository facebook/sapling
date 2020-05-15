/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Error;
use futures_stats::FutureStats;
use scuba::{ScubaSampleBuilder, ScubaValue};
use time_ext::DurationExt;

use blobstore::BlobstoreGetData;
use metaconfig_types::BlobstoreId;

const SLOW_REQUEST_THRESHOLD: Duration = Duration::from_secs(5);

const BLOBSTORE_ID: &str = "blobstore_id";
const COMPLETION_TIME: &str = "completion_time";
const ERROR: &str = "error";
const KEY: &str = "key";
const OPERATION: &str = "operation";
const SESSION: &str = "session";
const SIZE: &str = "size";
const WRITE_ORDER: &str = "write_order";

#[derive(Clone, Copy)]
pub enum OperationType {
    Get,
    Put,
    ScrubGet,
}

impl From<OperationType> for ScubaValue {
    fn from(value: OperationType) -> ScubaValue {
        match value {
            OperationType::Get => ScubaValue::from("get"),
            OperationType::Put => ScubaValue::from("put"),
            OperationType::ScrubGet => ScubaValue::from("scrub_get"),
        }
    }
}

fn add_common_values(
    scuba: &mut ScubaSampleBuilder,
    key: String,
    session: String,
    stats: FutureStats,
    operation: OperationType,
    blobstore_id: Option<BlobstoreId>,
) {
    scuba
        .add(KEY, key)
        .add(OPERATION, operation)
        .add(COMPLETION_TIME, stats.completion_time.as_micros_unchecked());

    if let Some(blobstore_id) = blobstore_id {
        scuba.add(BLOBSTORE_ID, blobstore_id);
    }

    if stats.completion_time >= SLOW_REQUEST_THRESHOLD {
        scuba.add(SESSION, session);
    }
}

pub fn record_get_stats(
    scuba: &mut ScubaSampleBuilder,
    stats: FutureStats,
    result: Result<&Option<BlobstoreGetData>, &Error>,
    key: String,
    session: String,
    operation: OperationType,
    blobstore_id: Option<BlobstoreId>,
) {
    add_common_values(scuba, key, session, stats, operation, blobstore_id);

    match result {
        Ok(Some(data)) => {
            scuba.add(SIZE, data.as_bytes().len());
        }
        Err(error) => {
            // Always log errors
            scuba.unsampled();
            scuba.add(ERROR, error.to_string());
        }
        Ok(None) => {}
    }

    scuba.log();
}

pub fn record_put_stats(
    scuba: &mut ScubaSampleBuilder,
    stats: FutureStats,
    result: Result<&(), &Error>,
    key: String,
    session: String,
    operation: OperationType,
    size: usize,
    blobstore_id: Option<BlobstoreId>,
    write_order: Option<usize>,
) {
    add_common_values(scuba, key, session, stats, operation, blobstore_id);
    scuba.add(SIZE, size);

    match result {
        Ok(_) => {
            if let Some(write_order) = write_order {
                scuba.add(WRITE_ORDER, write_order);
            }
        }
        Err(error) => {
            scuba.add(ERROR, error.to_string());
        }
    };

    scuba.log();
}
