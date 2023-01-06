/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::hash::Hasher;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use blobstore::BlobstoreGetData;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore_stats::record_put_stats;
use context::CoreContext;
use futures::future;
use futures_stats::TimedFutureExt;
use itertools::Either;
use metaconfig_types::BlobstoreId;
use mononoke_types::BlobstoreBytes;
use scuba_ext::MononokeScubaSampleBuilder;
use thiserror::Error;
use tokio::time::timeout;
use twox_hash::XxHash;

use crate::scrub::SrubWriteOnly;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

type BlobstoresWithEntry = Vec<HashSet<BlobstoreId>>;
type BlobstoresReturnedNone = HashSet<BlobstoreId>;
type BlobstoresReturnedError = HashMap<BlobstoreId, Error>;

#[derive(Error, Debug, Clone)]
pub enum ErrorKind {
    #[error("Some blobstores failed, and other returned None: {main_errors:?}")]
    SomeFailedOthersNone {
        main_errors: Arc<BlobstoresReturnedError>,
        write_only_errors: Arc<BlobstoresReturnedError>,
    },
    #[error("All blobstores failed: {main_errors:?}")]
    AllFailed {
        main_errors: Arc<BlobstoresReturnedError>,
        write_only_errors: Arc<BlobstoresReturnedError>,
    },
    // Errors below this point are from ScrubBlobstore only. If they include an
    // Option<BlobstoreBytes>, this implies that this error is recoverable
    #[error(
        "Different blobstores have different values for this item: {0:?} are grouped by content, {1:?} do not have"
    )]
    ValueMismatch(Arc<BlobstoresWithEntry>, Arc<BlobstoresReturnedNone>),
    #[error("Some blobstores missing this item: {missing_main:?}")]
    SomeMissingItem {
        missing_main: Arc<BlobstoresReturnedNone>,
        missing_write_only: Arc<BlobstoresReturnedNone>,
        value: BlobstoreGetData,
    },
    #[error("Multiple failures on put: {0:?}")]
    MultiplePutFailures(Arc<BlobstoresReturnedError>),
}

fn blobstores_failed_error(
    main_blobstore_ids: impl Iterator<Item = BlobstoreId>,
    main_errors: HashMap<BlobstoreId, Error>,
    write_only_errors: HashMap<BlobstoreId, Error>,
) -> ErrorKind {
    let main_errored_ids: HashSet<BlobstoreId> = main_errors.keys().copied().collect();
    let all_main_ids: HashSet<BlobstoreId> = main_blobstore_ids.collect();
    if main_errored_ids == all_main_ids {
        // The write only stores that returned None might not have been fully populated
        ErrorKind::AllFailed {
            main_errors: Arc::new(main_errors),
            write_only_errors: Arc::new(write_only_errors),
        }
    } else {
        ErrorKind::SomeFailedOthersNone {
            main_errors: Arc::new(main_errors),
            write_only_errors: Arc::new(write_only_errors),
        }
    }
}

type GetResult = (BlobstoreId, Result<Option<BlobstoreGetData>, Error>);

/// Get normal and write only results based on SrubWriteOnly
/// mode of getting results, which might optimise for less access
/// to main blobstores or less access to write only blobstores
pub async fn scrub_get_results<MF, WF>(
    get_main_results: impl FnOnce() -> MF,
    mut get_write_only_results: impl FnMut() -> WF,
    write_only_blobstores: impl Iterator<Item = BlobstoreId>,
    write_only: SrubWriteOnly,
) -> impl Iterator<Item = (bool, GetResult)>
where
    MF: Future<Output = Vec<GetResult>>,
    WF: Future<Output = Vec<GetResult>>,
{
    // Exit early if all mostly-write are ok, and don't check main blobstores
    if write_only == SrubWriteOnly::ScrubIfAbsent {
        let mut results = get_write_only_results().await.into_iter();
        if let Some((bs_id, Ok(Some(data)))) = results.next() {
            if results.all(|(_, r)| match r {
                Ok(Some(other_data)) => other_data == data,
                _ => false,
            }) {
                return Either::Left(std::iter::once((true, (bs_id, Ok(Some(data))))));
            }
        }
    }

    let write_only_results = async {
        match write_only {
            SrubWriteOnly::Scrub | SrubWriteOnly::SkipMissing => get_write_only_results().await,
            SrubWriteOnly::PopulateIfAbsent | SrubWriteOnly::ScrubIfAbsent => {
                write_only_blobstores.map(|id| (id, Ok(None))).collect()
            }
        }
    };
    let (normal_results, write_only_results) =
        future::join(get_main_results(), write_only_results).await;

    Either::Right(
        normal_results
            .into_iter()
            .map(|r| (false, r))
            .chain(write_only_results.into_iter().map(|r| (true, r))),
    )
}

pub fn scrub_parse_results(
    results: impl Iterator<Item = (bool, GetResult)>,
    all_main: impl Iterator<Item = BlobstoreId>,
) -> Result<Option<BlobstoreGetData>, ErrorKind> {
    let mut missing_main = HashSet::new();
    let mut missing_write_only = HashSet::new();
    let mut get_data = None;
    let mut main_errors = HashMap::new();
    let mut write_only_errors = HashMap::new();

    for (is_write_only, (blobstore_id, result)) in results {
        match result {
            Ok(None) => {
                if is_write_only {
                    missing_write_only.insert(blobstore_id);
                } else {
                    missing_main.insert(blobstore_id);
                }
            }
            Ok(Some(value)) => {
                let mut content_hash = XxHash::with_seed(0);
                content_hash.write(value.as_raw_bytes());
                let content_hash = content_hash.finish();
                let (all_values, _) = get_data.get_or_insert_with(|| (HashMap::new(), value));
                all_values
                    .entry(content_hash)
                    .or_insert_with(HashSet::new)
                    .insert(blobstore_id);
            }
            Err(err) => {
                if is_write_only {
                    write_only_errors.insert(blobstore_id, err);
                } else {
                    main_errors.insert(blobstore_id, err);
                }
            }
        }
    }
    match get_data {
        None => {
            if main_errors.is_empty() && write_only_errors.is_empty() {
                Ok(None)
            } else {
                Err(blobstores_failed_error(
                    all_main,
                    main_errors,
                    write_only_errors,
                ))
            }
        }
        Some((all_values, value)) if all_values.len() == 1 => {
            if missing_main.is_empty() && missing_write_only.is_empty() {
                Ok(Some(value))
            } else {
                // This silently ignores failed blobstores if at least one has a value
                Err(ErrorKind::SomeMissingItem {
                    missing_main: Arc::new(missing_main),
                    missing_write_only: Arc::new(missing_write_only),
                    value,
                })
            }
        }
        Some((all_values, _)) => {
            let answered = all_values.into_iter().map(|(_, stores)| stores).collect();
            let mut all_missing = HashSet::new();
            all_missing.extend(missing_main.into_iter());
            all_missing.extend(missing_write_only.into_iter());
            Err(ErrorKind::ValueMismatch(
                Arc::new(answered),
                Arc::new(all_missing),
            ))
        }
    }
}

fn remap_timeout_result<O>(
    timeout_or_result: Result<Result<O, Error>, tokio::time::error::Elapsed>,
) -> Result<O, Error> {
    timeout_or_result.unwrap_or_else(|_| Err(Error::msg("blobstore operation timeout")))
}

pub async fn inner_put(
    ctx: &CoreContext,
    mut scuba: MononokeScubaSampleBuilder,
    write_order: &AtomicUsize,
    blobstore_id: BlobstoreId,
    blobstore: &dyn BlobstorePutOps,
    key: String,
    value: BlobstoreBytes,
    put_behaviour: Option<PutBehaviour>,
) -> (BlobstoreId, Result<OverwriteStatus, Error>) {
    let size = value.len();
    let (pc, (stats, timeout_or_res)) = {
        let mut ctx = ctx.clone();
        let pc = ctx.fork_perf_counters();
        let ret = timeout(
            REQUEST_TIMEOUT,
            if let Some(put_behaviour) = put_behaviour {
                blobstore.put_explicit(&ctx, key.clone(), value, put_behaviour)
            } else {
                blobstore.put_with_status(&ctx, key.clone(), value)
            },
        )
        .timed()
        .await;
        (pc, ret)
    };
    let result = remap_timeout_result(timeout_or_res);
    record_put_stats(
        &mut scuba,
        &pc,
        stats,
        result.as_ref(),
        &key,
        ctx.metadata().session_id().as_str(),
        size,
        Some(blobstore_id),
        blobstore,
        Some(write_order.fetch_add(1, Ordering::Relaxed) + 1),
    );
    (blobstore_id, result)
}
