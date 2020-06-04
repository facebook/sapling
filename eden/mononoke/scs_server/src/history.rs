/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;

use futures::stream::{Stream, StreamExt, TryStreamExt};
use mononoke_api::{ChangesetContext, MononokeError};
use source_control as thrift;

use crate::errors;
use crate::into_response::AsyncIntoResponse;

pub(crate) async fn collect_history(
    history_stream: impl Stream<Item = Result<ChangesetContext, MononokeError>>,
    skip: usize,
    limit: usize,
    before_timestamp: Option<i64>,
    after_timestamp: Option<i64>,
    format: thrift::HistoryFormat,
    identity_schemes: &BTreeSet<thrift::CommitIdentityScheme>,
) -> Result<thrift::History, errors::ServiceError> {
    let history_stream = history_stream
        .map_err(errors::ServiceError::from)
        .skip(skip);

    let history = if before_timestamp.is_some() || after_timestamp.is_some() {
        history_stream
            .map(move |changeset| async move {
                let changeset = changeset?;
                let date = changeset.author_date().await?;

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

                Ok(Some(changeset))
            })
            // to check the date we need to fetch changeset first, that can be expensive
            // better to try doing it in parallel
            .buffered(100)
            .try_filter_map(|maybe_changeset| async move {
                Ok::<_, errors::ServiceError>(maybe_changeset)
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
                        Ok(cs) => (cs, identity_schemes).into_response().await,
                        Err(err) => Err(err),
                    }
                })
                .buffered(100)
                .try_collect()
                .await?;
            Ok(thrift::History::commit_infos(commit_infos))
        }
        other_format => Err(errors::invalid_request(format!(
            "unsupported history format {}",
            other_format
        ))
        .into()),
    }
}
