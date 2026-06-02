/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Context;
use anyhow::Error;
use bytes::Bytes;
use filestore::Alias;
use filestore::FetchKey;
use filestore::Range;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use gotham_ext::content_encoding::ContentEncoding;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::ScubaMiddlewareState;
use gotham_ext::response::CompressedResponseStream;
use gotham_ext::response::ResponseStream;
use gotham_ext::response::ResponseTryStreamExt;
use gotham_ext::response::StreamBody;
use gotham_ext::response::TryIntoResponse;
use gotham_ext::util::is_identity_subset;
use http::header::HeaderMap;
use http::header::RANGE;
use mononoke_types::ContentId;
use mononoke_types::hash::Sha256;
use permission_checker::MononokeIdentitySet;
use redactedblobstore::has_redaction_root_cause;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use serde::Deserialize;
use stats::prelude::*;

use crate::compression_sniff;
use crate::config::ServerConfig;
use crate::errors::ErrorKind;
use crate::lfs_server_context::RepositoryRequestContext;
use crate::middleware::LfsMethod;
use crate::scuba::LfsScubaKey;

define_stats! {
    prefix = "mononoke.lfs.download";
    size_bytes_sent: timeseries(
        "size_bytes_sent";
        Sum;
        Duration::from_secs(5), Duration::from_secs(15), Duration::from_mins(1)
    ),
    net_util: timeseries(
        "net_util";
        Average;
        Duration::from_secs(5), Duration::from_secs(15), Duration::from_mins(1)
    ),
    load_shed_counter: dynamic_singleton_counter("{}", (key: String)),
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
        .with_context(|| format!("Unsupported range: {header}"))?;

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

/// JustKnob that gates the runtime behavior of compression sniffing for a
/// given repo. Per `eden/.llms/rules/rust_unwrap_safety.md`, a bare `?` is the
/// correct pattern here; defaults belong in `just_knobs.json`.
const SNIFF_JK: &str = "scm/mononoke:lfs_server_compression_sniff_enabled";

/// Pull the first chunk from `stream`, decide whether to bypass compression
/// based on its magic bytes, then return a stream that re-emits the consumed
/// chunk before the rest. Bypass triggers only when:
///   * the deployment was started with `--enable-compression-sniff`,
///   * the per-repo JustKnob is on,
///   * the client did not request a `Range:` (magic bytes only valid at offset 0),
///   * the desired encoding is `Compressed` (sniffing serves no purpose for `Identity`),
///   * the first chunk is at least `SNIFF_PREFIX_BYTES` long, and
///   * the prefix matches a known already-compressed container format.
async fn maybe_sniff_and_choose_encoding<S>(
    stream: S,
    desired: ContentEncoding,
    range_present: bool,
    sniff_enabled_cli: bool,
    repo_name: &str,
    scuba: &mut Option<&mut ScubaMiddlewareState>,
) -> (ContentEncoding, BoxStream<'static, Result<Bytes, Error>>)
where
    S: Stream<Item = Result<Bytes, Error>> + Send + 'static,
{
    let want_sniff = sniff_enabled_cli
        && !range_present
        && matches!(desired, ContentEncoding::Compressed(_))
        && justknobs::eval(SNIFF_JK, None, Some(repo_name));

    if !want_sniff {
        return (desired, stream.boxed());
    }

    let mut stream = stream.boxed();
    let first = match stream.next().await {
        Some(Ok(b)) => b,
        Some(Err(e)) => {
            // Re-emit the error so the caller's downstream error handling fires.
            let s = stream::once(async move { Err(e) }).chain(stream).boxed();
            return (desired, s);
        }
        // Empty stream — nothing to compress, encoding is moot.
        None => return (desired, stream),
    };

    let sniffed = if first.len() >= compression_sniff::SNIFF_PREFIX_BYTES {
        compression_sniff::looks_compressed(&first[..compression_sniff::SNIFF_PREFIX_BYTES])
    } else {
        None
    };

    let effective = if let Some(format) = sniffed {
        // Logged value is the matched format name (e.g., "zip", "gzip", "png")
        // so we can break down savings per blob type in Scuba.
        ScubaMiddlewareState::maybe_add(scuba, LfsScubaKey::CompressionBypassReason, format);
        ContentEncoding::Identity
    } else {
        desired
    };

    let recombined = stream::once(async move { Ok(first) }).chain(stream).boxed();
    (effective, recombined)
}

async fn fetch_by_key(
    ctx: RepositoryRequestContext,
    key: FetchKey,
    content_encoding: ContentEncoding,
    range: Option<Range>,
    scuba: &mut Option<&mut ScubaMiddlewareState>,
) -> Result<impl TryIntoResponse + use<>, HttpError> {
    // Query a stream out of the Filestore
    let fetched = filestore::fetch_range_with_size(
        ctx.repo.repo_blobstore().clone(),
        &ctx.ctx,
        &key,
        range.unwrap_or_else(Range::all),
    )
    .await
    .map_err(|e| {
        if has_redaction_root_cause(&e).is_some() {
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

    let (effective_encoding, stream) = maybe_sniff_and_choose_encoding(
        stream,
        content_encoding,
        range.is_some(),
        ctx.compression_sniff_enabled(),
        ctx.repo.repo_identity().name(),
        scuba,
    )
    .await;

    let stream = match effective_encoding {
        ContentEncoding::Identity => ResponseStream::new(stream)
            .set_content_length(size)
            .left_stream(),
        ContentEncoding::Compressed(c) => CompressedResponseStream::new(stream, c).right_stream(),
    };

    let stream = if ctx.config.track_bytes_sent() {
        stream
            .inspect_ok(move |bytes| {
                STATS::size_bytes_sent.add_value(bytes.len() as i64);
                if let Some(bandwidth) = ctx.bandwidth() {
                    if let Some(bytes_sent) = STATS::load_shed_counter
                        .get_value(ctx.ctx.fb, ("size_bytes_sent.sum.15".to_string(),))
                    {
                        let bits_per_second = bytes_sent * 8 / 15;
                        STATS::net_util.add_value(100 * bits_per_second / bandwidth);
                    }
                }
            })
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
) -> Result<impl TryIntoResponse + use<>, HttpError> {
    let range = extract_range(state).map_err(HttpError::e400)?;

    let ctx = RepositoryRequestContext::instantiate(state, repository.clone(), method).await?;

    let disable_compression =
        should_disable_compression(&ctx.config, Some(ctx.ctx.metadata().identities()));

    let content_encoding = if disable_compression {
        ContentEncoding::Identity
    } else {
        ContentEncoding::from_state(state)
    };

    let mut scuba = state.try_borrow_mut::<ScubaMiddlewareState>();

    fetch_by_key(ctx, key, content_encoding, range, &mut scuba).await
}

pub async fn download(state: &mut State) -> Result<impl TryIntoResponse + use<>, HttpError> {
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

pub async fn download_sha256(state: &mut State) -> Result<impl TryIntoResponse + use<>, HttpError> {
    let DownloadParamsSha256 { repository, oid } = state.take();

    let oid = Sha256::from_str(&oid)
        .context(ErrorKind::InvalidOid)
        .map_err(HttpError::e400)?;

    let key = FetchKey::Aliased(Alias::Sha256(oid));

    download_inner(state, repository, key, LfsMethod::DownloadSha256).await
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use anyhow::Error;
    use fbinit::FacebookInit;
    use http::StatusCode;
    use maplit::hashmap;
    use mononoke_macros::mononoke;
    use mononoke_types::typed_hash::BlobstoreKey;
    use mononoke_types_mocks::contentid::ONES_CTID;
    use permission_checker::MononokeIdentity;
    use redactedblobstore::RedactedBlobs;
    use redactedblobstore::RedactedMetadata;
    use test_repo_factory::TestRepoFactory;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_redacted_fetch(fb: FacebookInit) -> Result<(), Error> {
        let content_id = ONES_CTID;
        let reason = "test reason";

        let repo = TestRepoFactory::new(fb)?
            .redacted(Some(RedactedBlobs::FromHashMapForTests(Arc::new(
                hashmap! { content_id.blobstore_key() => RedactedMetadata {
                   task: reason.to_string(),
                   log_only: false,
                }},
            ))))
            .build()
            .await?;

        let ctx = RepositoryRequestContext::test_builder(fb)
            .await?
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

    #[mononoke::test]
    fn test_parse_range() -> Result<(), Error> {
        // NOTE: This range is inclusive, so here we want bytes 1, 2, 3, 5 (a 5-byte range starting
        // at byte 1).
        assert_eq!(parse_range("bytes=1-5")?, Range::sized(1, 5).strict());
        assert!(parse_range("1-5").is_err());
        assert!(parse_range("foo=1-5").is_err());
        Ok(())
    }

    #[mononoke::test]
    fn test_should_disable_compression() -> Result<(), Error> {
        let mut config = ServerConfig::default();
        let test_ident = MononokeIdentity::from_legacy_type_data("USER", "test");
        let test_ident_2 = MononokeIdentity::from_legacy_type_data("USER", "test2");
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

    use bytes::Bytes;
    use futures::stream;
    use gotham_ext::content_encoding::ContentCompression;

    fn gz_payload() -> Bytes {
        // gzip magic + 14 bytes of arbitrary trailing data (>= SNIFF_PREFIX_BYTES total).
        Bytes::from_static(b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x03ABCDEFGH")
    }

    fn text_payload() -> Bytes {
        Bytes::from_static(b"plain text content not compressed at all by us")
    }

    async fn drain(s: BoxStream<'static, Result<Bytes, Error>>) -> Result<Vec<u8>, Error> {
        let chunks: Vec<Bytes> = s.try_collect().await?;
        Ok(chunks.into_iter().flatten().collect())
    }

    #[mononoke::fbinit_test]
    async fn sniff_disabled_via_cli_returns_desired_encoding() -> Result<(), Error> {
        let payload = gz_payload();
        let s = stream::once(async move { Ok(payload.clone()) }).boxed();
        let (enc, out) = maybe_sniff_and_choose_encoding(
            s,
            ContentEncoding::Compressed(ContentCompression::Gzip),
            false,
            false, // CLI flag off
            "test_repo",
            &mut None,
        )
        .await;
        assert!(matches!(
            enc,
            ContentEncoding::Compressed(ContentCompression::Gzip)
        ));
        assert_eq!(drain(out).await?, gz_payload().to_vec());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn sniff_skipped_when_range_present() -> Result<(), Error> {
        let payload = gz_payload();
        let s = stream::once(async move { Ok(payload.clone()) }).boxed();
        let (enc, out) = maybe_sniff_and_choose_encoding(
            s,
            ContentEncoding::Compressed(ContentCompression::Gzip),
            true, // range present
            true,
            "test_repo",
            &mut None,
        )
        .await;
        assert!(matches!(
            enc,
            ContentEncoding::Compressed(ContentCompression::Gzip)
        ));
        assert_eq!(drain(out).await?, gz_payload().to_vec());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn sniff_skipped_when_desired_is_identity() -> Result<(), Error> {
        let payload = gz_payload();
        let s = stream::once(async move { Ok(payload.clone()) }).boxed();
        let (enc, out) = maybe_sniff_and_choose_encoding(
            s,
            ContentEncoding::Identity,
            false,
            true,
            "test_repo",
            &mut None,
        )
        .await;
        assert!(matches!(enc, ContentEncoding::Identity));
        assert_eq!(drain(out).await?, gz_payload().to_vec());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn sniff_preserves_text_payload_passthrough() -> Result<(), Error> {
        // CLI flag on, JK off (default in just_knobs.json) → no sniff,
        // payload is preserved as-is.
        let payload = text_payload();
        let s = stream::once(async move { Ok(payload.clone()) }).boxed();
        let (enc, out) = maybe_sniff_and_choose_encoding(
            s,
            ContentEncoding::Compressed(ContentCompression::Gzip),
            false,
            true,
            "test_repo_jk_off",
            &mut None,
        )
        .await;
        // Default JK is false, so encoding is unchanged.
        assert!(matches!(
            enc,
            ContentEncoding::Compressed(ContentCompression::Gzip)
        ));
        assert_eq!(drain(out).await?, text_payload().to_vec());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn sniff_passes_error_through() -> Result<(), Error> {
        let s = stream::once(async { Err::<Bytes, Error>(anyhow::anyhow!("boom")) }).boxed();
        let (_enc, out) = maybe_sniff_and_choose_encoding(
            s,
            ContentEncoding::Compressed(ContentCompression::Gzip),
            false,
            true,
            "any",
            &mut None,
        )
        .await;
        let err = drain(out).await.unwrap_err();
        assert!(err.to_string().contains("boom"));
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn sniff_handles_empty_stream() -> Result<(), Error> {
        let s = stream::empty::<Result<Bytes, Error>>().boxed();
        let (enc, out) = maybe_sniff_and_choose_encoding(
            s,
            ContentEncoding::Compressed(ContentCompression::Gzip),
            false,
            true,
            "any",
            &mut None,
        )
        .await;
        // With an empty stream we keep the desired encoding; the response will
        // be empty either way.
        assert!(matches!(
            enc,
            ContentEncoding::Compressed(ContentCompression::Gzip)
        ));
        assert_eq!(drain(out).await?, Vec::<u8>::new());
        Ok(())
    }
}
