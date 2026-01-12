/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use async_trait::async_trait;
use cloned::cloned;
use context::PerfCounterType;
use edenapi_types::ServerError;
use edenapi_types::StreamingChangelogRequest;
use edenapi_types::StreamingChangelogResponse;
use edenapi_types::legacy::Metadata;
use edenapi_types::legacy::StreamingChangelogBlob;
use edenapi_types::legacy::StreamingChangelogData;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_ext::FbStreamExt;
use futures_stats::TimedFutureExt;
use mononoke_api::Repo;
use streaming_clone::StreamingCloneArc;
use time_ext::DurationExt;
use tracing::debug;

use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use super::handler::SaplingRemoteApiContext;

const TIMEOUT_SECS: Duration = Duration::from_hours(4);

/// Legacy streaming changelog handler from wireproto.
pub struct StreamingCloneHandler;

#[async_trait]
impl SaplingRemoteApiHandler for StreamingCloneHandler {
    type Request = StreamingChangelogRequest;
    type Response = StreamingChangelogResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::StreamingClone;
    const ENDPOINT: &'static str = "/streaming_clone";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let streaming_clone = ectx.repo().repo().streaming_clone_arc();
        let ctx = ectx.repo().ctx().clone();

        let changelog = streaming_clone
            .fetch_changelog(ctx.clone(), request.tag.as_deref())
            .await?;

        let data_blobs: Vec<_> = changelog
            .data_blobs
            .into_iter()
            .enumerate()
            .map(|(chunk_id, fut)| {
                cloned!(ctx);
                async move {
                    let (stats, res) = fut.timed().await;
                    ctx.perf_counters().add_to_counter(
                        PerfCounterType::SumManifoldPollTime,
                        stats.poll_time.as_nanos_unchecked() as i64,
                    );
                    if let Ok(bytes) = res.as_ref() {
                        ctx.perf_counters()
                            .add_to_counter(PerfCounterType::BytesSent, bytes.len() as i64)
                    }

                    let data = res.map(|res| {
                        StreamingChangelogData::DataBlobChunk(StreamingChangelogBlob {
                            chunk: res.into(),
                            chunk_id: chunk_id as u64,
                        })
                    });

                    StreamingChangelogResponse {
                        data: data.map_err(|e| ServerError::generic(format!("{:?}", e))),
                    }
                }
                .boxed()
            })
            .collect();

        let index_blobs: Vec<_> = changelog
            .index_blobs
            .into_iter()
            .enumerate()
            .map(|(chunk_id, fut)| {
                cloned!(ctx);
                async move {
                    let (stats, res) = fut.timed().await;
                    ctx.perf_counters().add_to_counter(
                        PerfCounterType::SumManifoldPollTime,
                        stats.poll_time.as_nanos_unchecked() as i64,
                    );
                    if let Ok(bytes) = res.as_ref() {
                        ctx.perf_counters()
                            .add_to_counter(PerfCounterType::BytesSent, bytes.len() as i64)
                    }

                    let data = res.map(|res| {
                        StreamingChangelogData::IndexBlobChunk(StreamingChangelogBlob {
                            chunk: res.into(),
                            chunk_id: chunk_id as u64,
                        })
                    });

                    StreamingChangelogResponse {
                        data: data.map_err(|e| ServerError::generic(format!("{:?}", e))),
                    }
                }
                .boxed()
            })
            .collect();

        debug!(
            "streaming changelog {} index bytes, {} data bytes",
            changelog.index_size, changelog.data_size
        );

        let metadata = StreamingChangelogData::Metadata(Metadata {
            index_size: changelog.index_size as u64,
            data_size: changelog.data_size as u64,
        });
        let mut response_header = Vec::new();
        response_header.push(metadata);

        let response = stream::iter(
            response_header
                .into_iter()
                .map(|data| StreamingChangelogResponse { data: Ok(data) }),
        );

        let res = response
            .chain(stream::iter(index_blobs).buffered(100))
            .chain(stream::iter(data_blobs).buffered(100));

        Ok(res
            .whole_stream_timeout(TIMEOUT_SECS)
            .yield_periodically()
            .map_err(|e| e.into())
            .boxed())
    }
}
