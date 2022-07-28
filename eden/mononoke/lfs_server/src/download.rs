/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Context;
use anyhow::Error;
use filestore::Alias;
use filestore::FetchKey;
use filestore::Range;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use gotham_ext::content_encoding::ContentEncoding;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::ClientIdentity;
use gotham_ext::middleware::ScubaMiddlewareState;
use gotham_ext::response::CompressedResponseStream;
use gotham_ext::response::ResponseStream;
use gotham_ext::response::ResponseTryStreamExt;
use gotham_ext::response::StreamBody;
use gotham_ext::response::TryIntoResponse;
use http::header::HeaderMap;
use http::header::RANGE;
use mononoke_types::hash::Sha256;
use mononoke_types::ContentId;
use permission_checker::MononokeIdentitySet;
use redactedblobstore::has_redaction_root_cause;
use repo_blobstore::RepoBlobstoreRef;
use serde::Deserialize;
use stats::prelude::*;

use crate::config::ServerConfig;
use crate::errors::ErrorKind;
use crate::lfs_server_context::RepositoryRequestContext;
use crate::middleware::LfsMethod;
use crate::scuba::LfsScubaKey;
use crate::util::is_identity_subset;

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
    )?
    .strict();

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

fn should_disable_compression(
    config: &ServerConfig,
    client_idents: Option<&MononokeIdentitySet>,
) -> bool {
    if config.disable_compression() {
        return true;
    }

    is_identity_subset(config.disable_compression_identities(), client_idents)
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
        ctx.repo.repo_blobstore().clone(),
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
        .ok_or(ErrorKind::ObjectDoesNotExist(key))
        .map_err(HttpError::e404)?;

    ScubaMiddlewareState::maybe_add(scuba, LfsScubaKey::DownloadContentSize, size);

    let stream = match content_encoding {
        ContentEncoding::Identity => ResponseStream::new(stream)
            .set_content_length(size)
            .left_stream(),
        ContentEncoding::Compressed(c) => CompressedResponseStream::new(stream, c).right_stream(),
    };

    let stream = if ctx.config.track_bytes_sent() {
        stream
            .inspect_ok(|bytes| STATS::size_bytes_sent.add_value(bytes.len() as i64))
            .left_stream()
    } else {
        stream.right_stream()
    };

    let stream = stream.end_on_err();

    let mut body = StreamBody::new(stream, mime::APPLICATION_OCTET_STREAM);
    if range.is_some() {
        body.partial = true;
    }
    Ok(body)
}

async fn download_inner(
    state: &mut State,
    repository: String,
    key: FetchKey,
    method: LfsMethod,
) -> Result<impl TryIntoResponse, HttpError> {
    let range = extract_range(state).map_err(HttpError::e400)?;

    let ctx = RepositoryRequestContext::instantiate(state, repository.clone(), method).await?;

    let idents =
        ClientIdentity::try_borrow_from(state).and_then(|ident| ident.identities().as_ref());

    let disable_compression = should_disable_compression(&ctx.config, idents);

    let content_encoding = if disable_compression {
        ContentEncoding::Identity
    } else {
        ContentEncoding::from_state(state)
    };

    let mut scuba = state.try_borrow_mut::<ScubaMiddlewareState>();

    fetch_by_key(ctx, key, content_encoding, range, &mut scuba).await
}

pub async fn download(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let DownloadParamsContentId {
        repository,
        content_id,
    } = state.take();

    let content_id = ContentId::from_str(&content_id)
        .context(ErrorKind::InvalidContentId)
        .map_err(HttpError::e400)?;

    let key = FetchKey::Canonical(content_id);

    download_inner(state, repository, key, LfsMethod::Download).await
}

pub async fn download_sha256(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let DownloadParamsSha256 { repository, oid } = state.take();

    let oid = Sha256::from_str(&oid)
        .context(ErrorKind::InvalidOid)
        .map_err(HttpError::e400)?;

    let key = FetchKey::Aliased(Alias::Sha256(oid));

    download_inner(state, repository, key, LfsMethod::DownloadSha256).await
}

#[cfg(test)]
mod test {
    use super::*;

    use anyhow::Error;
    use fbinit::FacebookInit;
    use http::StatusCode;
    use maplit::hashmap;
    use mononoke_types::typed_hash::BlobstoreKey;
    use mononoke_types_mocks::contentid::ONES_CTID;
    use permission_checker::MononokeIdentity;
    use redactedblobstore::RedactedBlobs;
    use redactedblobstore::RedactedMetadata;
    use std::sync::Arc;
    use test_repo_factory::TestRepoFactory;

    #[fbinit::test]
    async fn test_redacted_fetch(fb: FacebookInit) -> Result<(), Error> {
        let content_id = ONES_CTID;
        let reason = "test reason";

        let repo = TestRepoFactory::new(fb)?
            .redacted(Some(RedactedBlobs::FromSql(Arc::new(
                hashmap! { content_id.blobstore_key() => RedactedMetadata {
                   task: reason.to_string(),
                   log_only: false,
                }},
            ))))
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
        assert_eq!(parse_range("bytes=1-5")?, Range::sized(1, 5).strict());
        assert!(parse_range("1-5").is_err());
        assert!(parse_range("foo=1-5").is_err());
        Ok(())
    }

    #[test]
    fn test_should_disable_compression() -> Result<(), Error> {
        let mut config = ServerConfig::default();
        let test_ident = MononokeIdentity::new("USER", "test");
        let test_ident_2 = MononokeIdentity::new("USER", "test2");
        let mut client_idents = MononokeIdentitySet::new();
        client_idents.insert(test_ident.clone());

        assert!(!should_disable_compression(&config, None));
        assert!(!should_disable_compression(
            &config,
            Some(&MononokeIdentitySet::new())
        ));
        assert!(!should_disable_compression(&config, Some(&client_idents)));

        config
            .disable_compression_identities_mut()
            .push(client_idents.clone());
        assert!(!should_disable_compression(&config, None));
        assert!(should_disable_compression(&config, Some(&client_idents)));

        // A client must match all idents in a MononokeIdentitySet for compression to be disabled.
        config.disable_compression_identities_mut().clear();

        let mut and_set = MononokeIdentitySet::new();
        and_set.insert(test_ident);
        and_set.insert(test_ident_2);

        config.disable_compression_identities_mut().push(and_set);
        assert!(!should_disable_compression(&config, Some(&client_idents)));

        config.raw_server_config.disable_compression = true;
        assert!(should_disable_compression(&config, None));
        Ok(())
    }
}
