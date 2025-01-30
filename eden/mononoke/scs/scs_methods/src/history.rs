/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use cloned::cloned;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_ext::FbStreamExt;
use futures_watchdog::WatchdogExt;
use mononoke_api::ChangesetContext;
use mononoke_api::CoreContext;
use mononoke_api::MononokeError;
use mononoke_api::Repo;
use slog::warn;
use source_control as thrift;

use crate::into_response::AsyncIntoResponseWith;

pub(crate) async fn collect_history(
    ctx: &CoreContext,
    history_stream: impl Stream<Item = Result<ChangesetContext<Repo>, MononokeError>>,
    skip: usize,
    limit: usize,
    before_timestamp: Option<i64>,
    after_timestamp: Option<i64>,
    format: thrift::HistoryFormat,
    identity_schemes: &BTreeSet<thrift::CommitIdentityScheme>,
) -> Result<thrift::History, scs_errors::ServiceError> {
    let history_stream = history_stream
        .map_err(scs_errors::ServiceError::from)
        .skip(skip);

    let history = if before_timestamp.is_some() || after_timestamp.is_some() {
        history_stream
            .map(move |changeset| async move {
                let changeset = changeset?;
                if after_timestamp.is_some() || before_timestamp.is_some() {
                    let date = changeset.author_date().watched(ctx.logger()).await?;

                    if let Some(after) = after_timestamp {
                        if after > date.timestamp() {
                            return Ok(None);
                        }
                    }
                    if let Some(before) = before_timestamp {
                        if before < date.timestamp() {
                            return Ok(None);
                        }
                    }
                }

                Ok(Some(changeset))
            })
            // to check the date we need to fetch changeset first, that can be expensive
            // better to try doing it in parallel
            .buffered(100)
            .try_filter_map(|maybe_changeset| async move {
                Ok::<_, scs_errors::ServiceError>(maybe_changeset)
            })
            .take(limit)
            .left_stream()
    } else {
        history_stream.take(limit).right_stream()
    };

    match format {
        thrift::HistoryFormat::COMMIT_INFO => {
            let commit_infos: Vec<_> = history
                .map(|changeset| async {
                    match changeset {
                        Ok(cs) => {
                            cs.into_response_with(identity_schemes)
                                .watched(ctx.logger())
                                .await
                        }
                        Err(err) => Err(err),
                    }
                })
                .buffered(10)
                .yield_periodically()
                .on_large_overshoot(|budget, elapsed| {
                    warn!(ctx.logger(), "yield_periodically(): budget overshot: current_budget={budget:?}, elapsed={elapsed:?}");
                })
                .try_collect()
                .watched(ctx.logger())
                .await?;
            Ok(thrift::History::commit_infos(commit_infos))
        }
        thrift::HistoryFormat::COMMIT_ID => {
            let identity_schemes = identity_schemes.clone();
            let commit_ids: Vec<BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>> = history
                .chunks(100)
                // TryStreamExt doesn't have the try_chunks method yet so we have to do it by mapping
                .map(|chunk| chunk.into_iter().collect::<Result<Vec<_>, _>>())
                .and_then(move |changesets: Vec<ChangesetContext<Repo>>| {
                    let identity_schemes = identity_schemes.clone();
                    cloned!(ctx);
                    async move {
                        Ok(stream::iter(
                            changesets
                                .into_response_with(&identity_schemes)
                                .watched(ctx.logger())
                                .await?
                                .into_iter()
                                .map(Ok::<_, scs_errors::ServiceError>)
                                .collect::<Vec<_>>(),
                        ))
                    }
                })
                .try_flatten()
                .try_collect()
                .watched(ctx.logger())
                .await?;
            Ok(thrift::History::commit_ids(commit_ids))
        }
        other_format => Err(scs_errors::invalid_request(format!(
            "unsupported history format {}",
            other_format
        ))
        .into()),
    }
}
