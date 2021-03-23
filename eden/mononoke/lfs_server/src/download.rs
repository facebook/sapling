/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::{Context, Error};
use futures::stream::{StreamExt, TryStreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;
use slog::error;

use filestore::{self, Alias, FetchKey, Range};
use gotham_ext::{
    content::{CompressedContentStream, ContentEncoding, ContentStream},
    error::HttpError,
    middleware::ScubaMiddlewareState,
    response::{StreamBody, TryIntoResponse},
    stream_ext::GothamTryStreamExt,
};
use http::header::{HeaderMap, RANGE};
use mononoke_types::{hash::Sha256, ContentId};
use redactedblobstore::has_redaction_root_cause;
use stats::prelude::*;

use crate::errors::ErrorKind;
use crate::lfs_server_context::RepositoryRequestContext;
use crate::middleware::LfsMethod;
use crate::scuba::LfsScubaKey;

define_stats! {
    prefix = "mononoke.lfs.download";
    size_bytes_sent: timeseries(
        "size_bytes_sent";
        Sum;
        Duration::from_secs(5), Duration::from_secs(15), Duration::from_secs(60)
    ),
}
#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct DownloadParamsContentId {
    repository: String,
    content_id: String,
}

#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct DownloadParamsSha256 {
    repository: String,
    oid: String,
}

fn parse_range(header: &str) -> Result<Range, Error> {
    static RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"bytes=(\d+)-(\d+)").unwrap());

    let caps = RE
        .captures(header)
        .with_context(|| format!("Unsupported range: {}", header))?;

    let range = Range::range_inclusive(
        caps[1]
            .parse()
            .with_context(|| format!("Invalid range start: {}", &caps[1]))?,
        caps[2]
            .parse()
            .with_context(|| format!("Invalid range end: {}", &caps[2]))?,
    )?;

    Ok(range)
}

fn extract_range(state: &State) -> Result<Option<Range>, Error> {
    let header = match HeaderMap::try_borrow_from(state).and_then(|h| h.get(RANGE)) {
        Some(h) => h,
        None => return Ok(None),
    };

    let header = std::str::from_utf8(header.as_bytes()).context("Invalid range")?;

    Ok(Some(parse_range(header)?))
}

async fn fetch_by_key(
    ctx: RepositoryRequestContext,
    key: FetchKey,
    content_encoding: ContentEncoding,
    range: Option<Range>,
    scuba: &mut Option<&mut ScubaMiddlewareState>,
) -> Result<impl TryIntoResponse, HttpError> {
    // Query a stream out of the Filestore
    let fetched = filestore::fetch_range_with_size(
        ctx.repo.get_blobstore(),
        ctx.ctx.clone(),
        &key,
        range.unwrap_or_else(Range::all),
    )
    .await
    .map_err(|e| {
        if has_redaction_root_cause(&e) {
            HttpError::e410(e)
        } else {
            HttpError::e500(e.context(ErrorKind::FilestoreReadFailure))
        }
    })?;

    // Return a 404 if the stream doesn't exist.
    let (stream, size) = fetched
        .ok_or_else(|| ErrorKind::ObjectDoesNotExist(key))
        .map_err(HttpError::e404)?;

    ScubaMiddlewareState::maybe_add(scuba, LfsScubaKey::DownloadContentSize, size);

    let stream = match content_encoding {
        ContentEncoding::Identity => ContentStream::new(stream)
            .content_length(size)
            .left_stream(),
        ContentEncoding::Compressed(c) => CompressedContentStream::new(stream, c).right_stream(),
    };

    let stream = if ctx.config.track_bytes_sent() {
        stream
            .inspect_ok(|bytes| STATS::size_bytes_sent.add_value(bytes.len() as i64))
            .left_stream()
    } else {
        stream.right_stream()
    };

    let logger = ctx.logger().clone();
    let stream = stream.end_on_err(move |e| {
        // XXX: Ideally, we'd do something better with the error than just
        // printing it to stderr, but the server's code likely needs to be
        // restructed in order to do anything smarter here (such as setting
        // the error message in the RequestContext, so that the error would
        // get logged to Scuba).
        error!(&logger, "Error during streaming response: {:?}", &e);
    });

    let mut body = StreamBody::new(stream, mime::APPLICATION_OCTET_STREAM);
    if range.is_some() {
        body.partial = true;
    }
    Ok(body)
}

pub async fn download(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let DownloadParamsContentId {
        repository,
        content_id,
    } = state.take();

    let content_id = ContentId::from_str(&content_id)
        .context(ErrorKind::InvalidContentId)
        .map_err(HttpError::e400)?;

    let range = extract_range(state).map_err(HttpError::e400)?;

    let key = FetchKey::Canonical(content_id);
    let content_encoding = ContentEncoding::from_state(&state);

    let ctx = RepositoryRequestContext::instantiate(state, repository.clone(), LfsMethod::Download)
        .await?;

    let mut scuba = state.try_borrow_mut::<ScubaMiddlewareState>();
    fetch_by_key(ctx, key, content_encoding, range, &mut scuba).await
}

pub async fn download_sha256(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let DownloadParamsSha256 { repository, oid } = state.take();

    let oid = Sha256::from_str(&oid)
        .context(ErrorKind::InvalidOid)
        .map_err(HttpError::e400)?;

    let range = extract_range(state).map_err(HttpError::e400)?;

    let key = FetchKey::Aliased(Alias::Sha256(oid));
    let content_encoding = ContentEncoding::from_state(&state);

    let ctx =
        RepositoryRequestContext::instantiate(state, repository.clone(), LfsMethod::DownloadSha256)
            .await?;

    let mut scuba = state.try_borrow_mut::<ScubaMiddlewareState>();
    fetch_by_key(ctx, key, content_encoding, range, &mut scuba).await
}

#[cfg(test)]
mod test {
    use super::*;

    use anyhow::Error;
    use blobrepo_factory::TestRepoBuilder;
    use fbinit::FacebookInit;
    use http::StatusCode;
    use maplit::hashmap;
    use mononoke_types::typed_hash::MononokeId;
    use mononoke_types_mocks::contentid::ONES_CTID;
    use redactedblobstore::RedactedMetadata;

    #[fbinit::test]
    async fn test_redacted_fetch(fb: FacebookInit) -> Result<(), Error> {
        let content_id = ONES_CTID;
        let reason = "test reason";

        let repo = TestRepoBuilder::new()
            .redacted(Some(
                hashmap! { content_id.blobstore_key() => RedactedMetadata {
                   task: reason.to_string(),
                   log_only: false,
                }},
            ))
            .build()?;

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .repo(repo)
            .build()?;

        let key = FetchKey::Canonical(content_id);

        let err = fetch_by_key(ctx, key, ContentEncoding::Identity, None, &mut None)
            .await
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.status_code, StatusCode::GONE);
        assert!(err.error.to_string().contains(reason));
        Ok(())
    }

    #[test]
    fn test_parse_range() -> Result<(), Error> {
        // NOTE: This range is inclusive, so here we want bytes 1, 2, 3, 5 (a 5-byte range starting
        // at byte 1).
        assert_eq!(parse_range("bytes=1-5")?, Range::sized(1, 5));
        Ok(())
    }
}
