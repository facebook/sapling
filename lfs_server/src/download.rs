/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use failure_ext::chain::ChainExt;
use futures::Stream;
use futures_ext::StreamExt;
use futures_preview::compat::Future01CompatExt;
use gotham::state::State;
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;
use std::convert::TryInto;
use std::time::Duration;

use filestore::{self, FetchKey};
use mononoke_types::ContentId;
use stats::{
    define_stats,
    service_data::{get_service_data_singleton, ServiceData},
    Timeseries,
};

use crate::config::ServerConfig;
use crate::errors::ErrorKind;
use crate::http::{HttpError, StreamBody, TryIntoResponse};
use crate::lfs_server_context::RepositoryRequestContext;
use crate::middleware::LfsMethod;

define_stats! {
    prefix = "mononoke.lfs.download";
    size_bytes_sent: timeseries("size_bytes_sent"; SUM; 5, 15, 60),
}

const THROTTLE_DELAY_MS: u64 = 1000;

#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct DownloadParams {
    repository: String,
    content_id: String,
}

async fn maybe_throttle(max: u64, key: &str) -> Result<(), HttpError> {
    // NOTE: We ignore negative counter values here: those would be unexpected, and also useless.

    // NOTE: I'm using assume_init because SERVICE_DATA has been refactored away after I started
    // this stack, and I'm removing this bot of code in the next diff, which isn't using
    // assume_init() anymore later in this stack.
    let fb = unsafe { fbinit::assume_init() };

    let usage: Option<u64> = get_service_data_singleton(fb)
        .get_counter(&key)
        .and_then(|v| v.try_into().ok());

    match usage {
        Some(usage) if usage > max => {
            // We apply an arbitrary delay before returning the throttle. This is helpful because
            // our clients don't.
            let _ = tokio_timer::sleep(Duration::from_millis(THROTTLE_DELAY_MS))
                .compat()
                .await;

            Err(HttpError::e429(ErrorKind::Throttled(key.to_string())))
        }
        _ => Ok(()),
    }
}

async fn apply_throttle(config: &ServerConfig) -> Result<(), HttpError> {
    // We read throttling data from our own service data counters. The upside of this approach is
    // that we get 2 nice things for free:
    // - Thread-local stats and stats aggregation across threads.
    // - The counters are guaranteed to be consistent with what we expose in ODS.
    // Note that this rate limiting is per-host.

    if let Some(limit_5s) = config.max_bytes_sent_5s {
        maybe_throttle(limit_5s, "mononoke.lfs.download.size_bytes_sent.sum.5").await?;
    }

    if let Some(limit_15s) = config.max_bytes_sent_15s {
        maybe_throttle(limit_15s, "mononoke.lfs.download.size_bytes_sent.sum.15").await?;
    }

    Ok(())
}

pub async fn download(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let DownloadParams {
        repository,
        content_id,
    } = state.take();

    let ctx = RepositoryRequestContext::instantiate(state, repository.clone(), LfsMethod::Download)
        .map_err(HttpError::e400)?;

    apply_throttle(&ctx.config).await?;

    let content_id = ContentId::from_str(&content_id)
        .chain_err(ErrorKind::InvalidContentId)
        .map_err(HttpError::e400)?;

    // Query a stream out of the Filestore
    let fetch_stream = filestore::fetch_with_size(
        &ctx.repo.get_blobstore(),
        ctx.ctx.clone(),
        &FetchKey::Canonical(content_id),
    )
    .compat()
    .await
    .chain_err(ErrorKind::FilestoreReadFailure)
    .map_err(HttpError::e500)?;

    // Return a 404 if the stream doesn't exist.
    let (stream, size) = fetch_stream
        .ok_or_else(|| ErrorKind::ObjectDoesNotExist(content_id))
        .map_err(HttpError::e404)?;

    let stream = if ctx.config.track_bytes_sent {
        stream
            .inspect(|bytes| STATS::size_bytes_sent.add_value(bytes.len() as i64))
            .left_stream()
    } else {
        stream.right_stream()
    };

    Ok(StreamBody::new(
        stream,
        size,
        mime::APPLICATION_OCTET_STREAM,
    ))
}
