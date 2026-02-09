/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::time::Duration;

use anyhow::format_err;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use bookmarks::Freshness;
use bytes::BytesMut;
use cloned::cloned;
use context::PerfCounterType;
use edenapi_types::BookmarkEntry;
use edenapi_types::HgId;
use edenapi_types::ListBookmarkPatternsRequest;
use edenapi_types::ListBookmarkPatternsResponse;
use edenapi_types::ServerError;
use edenapi_types::StreamingChangelogRequest;
use edenapi_types::StreamingChangelogResponse;
use edenapi_types::legacy::Metadata;
use edenapi_types::legacy::StreamingChangelogBlob;
use edenapi_types::legacy::StreamingChangelogData;
use futures::future::FutureExt;
use futures::future::join_all;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_ext::FbStreamExt;
use futures_stats::TimedFutureExt;
use itertools::Itertools;
use mercurial_types::HgChangesetId;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api_hg::HgRepoContext;
use mononoke_types::ChangesetId;
use streaming_clone::StreamingCloneArc;
use time_ext::DurationExt;
use tracing::debug;

use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use super::handler::SaplingRemoteApiContext;
use crate::Error;
use crate::errors::ErrorKind;

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

        let aggregation_factor = justknobs::get_as::<usize>(
            "scm/mononoke:streaming_clone_chunk_aggregation_factor",
            None,
        )?;

        let data_blobs: Vec<_> = changelog
            .data_blobs
            .into_iter()
            .chunks(aggregation_factor)
            .into_iter()
            .enumerate()
            .map(|(chunk_id, chunk_futs)| {
                let futs: Vec<_> = chunk_futs.collect();
                cloned!(ctx);
                async move {
                    // Wrap all futures with timing and await in parallel
                    let timed_futs: Vec<_> = futs.into_iter().map(|fut| fut.timed()).collect();
                    let results = join_all(timed_futs).await;

                    // Process results, accumulating stats
                    let mut combined = BytesMut::new();
                    let mut total_poll_time: i64 = 0;

                    for (stats, res) in results {
                        total_poll_time += stats.poll_time.as_nanos_unchecked() as i64;
                        match res {
                            Ok(bytes) => {
                                combined.extend_from_slice(&bytes);
                            }
                            Err(e) => {
                                return StreamingChangelogResponse {
                                    data: Err(ServerError::generic(format!("{:?}", e))),
                                };
                            }
                        }
                    }

                    // All blobs succeeded - record stats
                    ctx.perf_counters()
                        .add_to_counter(PerfCounterType::SumManifoldPollTime, total_poll_time);
                    ctx.perf_counters()
                        .add_to_counter(PerfCounterType::BytesSent, combined.len() as i64);

                    StreamingChangelogResponse {
                        data: Ok(StreamingChangelogData::DataBlobChunk(
                            StreamingChangelogBlob {
                                chunk: combined.freeze().into(),
                                chunk_id: chunk_id as u64,
                            },
                        )),
                    }
                }
                .boxed()
            })
            .collect();

        let index_blobs: Vec<_> = changelog
            .index_blobs
            .into_iter()
            .chunks(aggregation_factor)
            .into_iter()
            .enumerate()
            .map(|(chunk_id, chunk_futs)| {
                let futs: Vec<_> = chunk_futs.collect();
                cloned!(ctx);
                async move {
                    // Wrap all futures with timing and await in parallel
                    let timed_futs: Vec<_> = futs.into_iter().map(|fut| fut.timed()).collect();
                    let results = join_all(timed_futs).await;

                    // Process results, accumulating stats
                    let mut combined = BytesMut::new();
                    let mut total_poll_time: i64 = 0;

                    for (stats, res) in results {
                        total_poll_time += stats.poll_time.as_nanos_unchecked() as i64;
                        match res {
                            Ok(bytes) => {
                                combined.extend_from_slice(&bytes);
                            }
                            Err(e) => {
                                return StreamingChangelogResponse {
                                    data: Err(ServerError::generic(format!("{:?}", e))),
                                };
                            }
                        }
                    }

                    // All blobs succeeded - record stats
                    ctx.perf_counters()
                        .add_to_counter(PerfCounterType::SumManifoldPollTime, total_poll_time);
                    ctx.perf_counters()
                        .add_to_counter(PerfCounterType::BytesSent, combined.len() as i64);

                    StreamingChangelogResponse {
                        data: Ok(StreamingChangelogData::IndexBlobChunk(
                            StreamingChangelogBlob {
                                chunk: combined.freeze().into(),
                                chunk_id: chunk_id as u64,
                            },
                        )),
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

/// List bookmarks matching patterns (replacement for wireproto listkeyspatterns).
/// Patterns can be exact bookmark names or prefix patterns ending with '*'.
pub struct ListBookmarkPatternsHandler;

#[async_trait]
impl SaplingRemoteApiHandler for ListBookmarkPatternsHandler {
    type Request = ListBookmarkPatternsRequest;
    type Response = ListBookmarkPatternsResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::ListBookmarkPatterns;
    const ENDPOINT: &'static str = "/bookmarks/list_patterns";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let max = repo.repo_ctx().config().list_keys_patterns_max;

        let results: Vec<Result<Vec<BookmarkEntry>, Error>> = stream::iter(request.patterns)
            .map(|pattern| list_bookmarks_for_pattern(&repo, pattern, max))
            .buffered(100)
            .collect()
            .await;

        let responses = results.into_iter().flat_map(|result| match result {
            Ok(entries) => entries
                .into_iter()
                .map(|entry| Ok(ListBookmarkPatternsResponse { data: Ok(entry) }))
                .collect::<Vec<_>>(),
            Err(e) => vec![Ok(ListBookmarkPatternsResponse {
                data: Err(ServerError::generic(format!("{:?}", e))),
            })],
        });

        Ok(stream::iter(responses).boxed())
    }

    fn extract_in_band_error(response: &Self::Response) -> Option<Error> {
        response
            .data
            .as_ref()
            .err()
            .map(|err| format_err!("{:?}", err))
    }
}

/// List bookmarks matching a single pattern.
/// If the pattern ends with '*', it's treated as a prefix match.
/// Otherwise, it's treated as an exact match.
async fn list_bookmarks_for_pattern<R: MononokeRepo>(
    repo: &HgRepoContext<R>,
    pattern: String,
    max: u64,
) -> Result<Vec<BookmarkEntry>, Error> {
    if pattern.ends_with('*') {
        // Prefix match
        let prefix: &str = &pattern[..pattern.len() - 1];

        let bookmarks = repo
            .repo_ctx()
            .list_bookmarks(true, Some(prefix), None, Some(max))
            .await?
            .try_collect::<Vec<(String, ChangesetId)>>()
            .await?;

        if bookmarks.len() >= max as usize {
            return Err(format_err!(
                "Bookmark query was truncated after {} results, use a more specific prefix search.",
                max,
            ));
        }

        // Batch convert ChangesetId to HgId
        let cs_ids: Vec<ChangesetId> = bookmarks.iter().map(|(_, cs_id)| *cs_id).collect();
        let hg_mapping: HashMap<ChangesetId, HgChangesetId> = repo
            .repo_ctx()
            .many_changeset_hg_ids(cs_ids)
            .await?
            .into_iter()
            .collect();

        let entries: Vec<BookmarkEntry> = bookmarks
            .into_iter()
            .map(|(name, cs_id)| {
                let hgid = hg_mapping
                    .get(&cs_id)
                    .map(|hg_cs_id| HgId::from(hg_cs_id.into_nodehash()));
                BookmarkEntry {
                    bookmark: name,
                    hgid,
                }
            })
            .collect();

        Ok(entries)
    } else {
        // Exact match
        let _ = BookmarkKey::new(&pattern)?; // Validate the bookmark name
        let hgid = repo
            .resolve_bookmark(pattern.clone(), Freshness::MaybeStale)
            .await
            .map_err(|_| ErrorKind::BookmarkResolutionFailed(pattern.clone()))?
            .map(|id| HgId::from(id.into_nodehash()));

        match hgid {
            Some(_) => Ok(vec![BookmarkEntry {
                bookmark: pattern,
                hgid,
            }]),
            None => Ok(vec![]),
        }
    }
}
