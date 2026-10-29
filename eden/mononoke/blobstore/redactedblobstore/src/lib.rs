/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod errors;
mod redaction_config_blobstore;
pub mod store;

use std::ops::Deref;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::ContentMetadataV2Id;
use scuba_ext::MononokeScubaSampleBuilder;
use tracing::debug;

pub use crate::errors::RedactionError;
pub use crate::redaction_config_blobstore::ArcRedactionConfigBlobstore;
pub use crate::redaction_config_blobstore::RedactionConfigBlobstore;
pub use crate::store::RedactedBlobs;
pub use crate::store::RedactedMetadata;
pub use crate::store::load;

pub mod config {
    pub const GET_OPERATION: &str = "GET";
    pub const PUT_OPERATION: &str = "PUT";
}

#[derive(Debug, Clone)]
pub struct RedactedBlobstoreConfigInner {
    redacted: Option<Arc<RedactedBlobs>>,
    scuba_builder: MononokeScubaSampleBuilder,
    enforce: bool,
}

#[derive(Debug, Clone)]
pub struct RedactedBlobstoreConfig {
    inner: Arc<RedactedBlobstoreConfigInner>,
}

impl Deref for RedactedBlobstoreConfig {
    type Target = RedactedBlobstoreConfigInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl RedactedBlobstoreConfig {
    pub fn new(
        redacted: Option<Arc<RedactedBlobs>>,
        scuba_builder: MononokeScubaSampleBuilder,
    ) -> Self {
        Self {
            inner: Arc::new(RedactedBlobstoreConfigInner {
                redacted,
                scuba_builder,
                enforce: true,
            }),
        }
    }

    pub fn with_log_only_redaction(&self) -> Self {
        Self {
            inner: Arc::new(RedactedBlobstoreConfigInner {
                redacted: self.redacted.clone(),
                scuba_builder: self.scuba_builder.clone(),
                enforce: false,
            }),
        }
    }
}

#[derive(Debug)]
pub struct RedactedBlobstoreInner<T> {
    blobstore: T,
    config: RedactedBlobstoreConfig,
}

impl<T: std::fmt::Display> std::fmt::Display for RedactedBlobstoreInner<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RedactedBlobstoreInner<{}>", self.blobstore)
    }
}

// A wrapper for any blobstore, which provides a verification layer for the redacted blobs.
// The goal is to deny access to fetch sensitive data from the repository.
#[derive(Debug, Clone)]
pub struct RedactedBlobstore<T> {
    inner: Arc<RedactedBlobstoreInner<T>>,
}

impl<T: std::fmt::Display> std::fmt::Display for RedactedBlobstore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RedactedBlobstore<{}>", self.inner.blobstore)
    }
}

impl<T> Deref for RedactedBlobstore<T> {
    type Target = RedactedBlobstoreInner<T>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: Blobstore> RedactedBlobstore<T> {
    pub fn new(blobstore: T, config: RedactedBlobstoreConfig) -> Self {
        Self {
            inner: Arc::new(RedactedBlobstoreInner::new(blobstore, config)),
        }
    }

    pub fn boxed(&self) -> Arc<dyn Blobstore>
    where
        T: 'static,
    {
        self.inner.clone()
    }

    pub fn as_parts(&self) -> (T, RedactedBlobstoreConfig)
    where
        T: Clone,
    {
        (self.blobstore.clone(), self.config.clone())
    }

    pub fn as_inner_unredacted(&self) -> &T {
        &self.blobstore
    }
}

impl<T: Blobstore> RedactedBlobstoreInner<T> {
    pub fn new(blobstore: T, config: RedactedBlobstoreConfig) -> Self {
        Self { blobstore, config }
    }

    // Checks for access to this key, then yields the blobstore if access is allowed.
    pub fn access_blobstore<'s: 'a, 'a>(
        &'s self,
        ctx: &'a CoreContext,
        key: &'a str,
        operation: &'static str,
    ) -> Result<&'s T> {
        match &self.config.redacted {
            Some(redacted) => {
                redacted
                    .redacted()
                    .get(key)
                    .map_or(Ok(&self.blobstore), |metadata| {
                        debug!(
                            "{} operation with redacted blobstore with key {:?}",
                            operation, key
                        );
                        let log_only = metadata.log_only || !self.config.enforce;
                        self.log_redacted_blob_access_to_scuba(
                            ctx, key, operation, metadata, log_only,
                        );

                        if log_only {
                            Ok(&self.blobstore)
                        } else {
                            Err(RedactionError::Redacted(
                                key.to_string(),
                                metadata.task.to_string(),
                            )
                            .into())
                        }
                    })
            }
            None => Ok(&self.blobstore),
        }
    }

    /// `ContentMetadataV2` is stored under its own key but carries the first line of the
    /// file, so a read of the metadata of redacted content is logged, and when redaction is
    /// enforced the first line is removed. The other fields are hashes, sizes and flags that
    /// clients and derivation need to handle redacted files.
    fn redact_content_metadata(
        &self,
        ctx: &CoreContext,
        key: &str,
        data: BlobstoreGetData,
    ) -> Result<BlobstoreGetData> {
        let Some(redacted) = &self.config.redacted else {
            return Ok(data);
        };
        let Ok(metadata_id) = ContentMetadataV2Id::parse_blobstore_key(key) else {
            return Ok(data);
        };
        let content_key = ContentId::new(*metadata_id.blake2()).blobstore_key();
        let redacted = redacted.redacted();
        let Some(metadata) = redacted.get(&content_key) else {
            return Ok(data);
        };

        debug!(
            "GET operation of the content metadata of redacted content with key {:?}",
            key
        );
        let log_only = metadata.log_only || !self.config.enforce;
        self.log_redacted_blob_access_to_scuba(ctx, key, config::GET_OPERATION, metadata, log_only);
        if log_only {
            return Ok(data);
        }

        let content_metadata = ContentMetadataV2::from_bytes(data.as_bytes().as_bytes())
            .map_err(|_| RedactionError::Redacted(key.to_string(), metadata.task.to_string()))?;
        let content_metadata = ContentMetadataV2 {
            first_line: None,
            ..content_metadata
        };
        Ok(BlobstoreGetData::new(
            data.as_meta().clone(),
            content_metadata.into_blob().into(),
        ))
    }

    fn log_redacted_blob_access_to_scuba(
        &self,
        ctx: &CoreContext,
        key: &str,
        operation: &str,
        metadata: &RedactedMetadata,
        log_only: bool,
    ) {
        let mut scuba_builder = self.config.scuba_builder.clone();
        scuba_builder.unsampled();

        scuba_builder
            .add("operation", operation)
            .add("key", key.to_string())
            .add("reason", metadata.task.to_string())
            .add("enforced", (!log_only) as u32)
            .add("session_uuid", ctx.metadata().session_id().to_string());

        if let Some(unix_username) = ctx.metadata().unix_name() {
            scuba_builder.add("unix_username", unix_username);
        }

        if let Some(client_correlator) = ctx.client_correlator() {
            scuba_builder.add("client_correlator", client_correlator);
        }

        scuba_builder.add(
            "client_identities",
            ctx.metadata()
                .identities()
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>(),
        );

        scuba_builder.add(
            "client_identities_typed",
            ctx.metadata()
                .identities()
                .iter()
                .map(|i| i.to_typed_string())
                .collect::<Vec<_>>(),
        );

        scuba_builder.log();
    }
}

#[async_trait]
impl<B: Blobstore> Blobstore for RedactedBlobstoreInner<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let blobstore = self.access_blobstore(ctx, key, config::GET_OPERATION)?;
        blobstore
            .get(ctx, key)
            .await?
            .map(|data| self.redact_content_metadata(ctx, key, data))
            .transpose()
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.blobstore.is_present(ctx, key).await
    }

    async fn copy<'a>(
        &'a self,
        ctx: &'a CoreContext,
        old_key: &'a str,
        new_key: String,
    ) -> Result<()> {
        let blobstore = self.access_blobstore(ctx, old_key, config::GET_OPERATION)?;
        self.access_blobstore(ctx, &new_key, config::PUT_OPERATION)?;
        blobstore.copy(ctx, old_key, new_key).await
    }

    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        let blobstore = self.access_blobstore(ctx, key, config::PUT_OPERATION)?;
        blobstore.unlink(ctx, key).await
    }

    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        let blobstore = self.access_blobstore(ctx, &key, config::PUT_OPERATION)?;
        blobstore.put_explicit(ctx, key, value, put_behaviour).await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        let blobstore = self.access_blobstore(ctx, &key, config::PUT_OPERATION)?;
        blobstore.put_with_status(ctx, key, value).await
    }
}

#[async_trait]
impl<B: Blobstore> Blobstore for RedactedBlobstore<B> {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.inner.get(ctx, key).await
    }
    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.inner.is_present(ctx, key).await
    }

    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        self.inner.unlink(ctx, key).await
    }

    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.inner
            .put_explicit(ctx, key, value, put_behaviour)
            .await
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.inner.put_with_status(ctx, key, value).await
    }
}

pub fn has_redaction_root_cause(e: &Error) -> Option<(&str, &str)> {
    match e.root_cause().downcast_ref::<RedactionError>() {
        Some(RedactionError::Redacted(key, reason)) => Some((key.as_str(), reason.as_str())),
        None => None,
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use assert_matches::assert_matches;
    use borrowed::borrowed;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use memblob::Memblob;
    use mononoke_macros::mononoke;
    use mononoke_types::hash;
    use prefixblob::PrefixBlobstore;

    use super::*;

    const FIRST_LINE: &str = "first line of the file";

    fn content_metadata(content_id: ContentId) -> ContentMetadataV2 {
        ContentMetadataV2 {
            content_id,
            total_size: 100,
            sha1: hash::Sha1::from_byte_array([2; 20]),
            sha256: hash::Sha256::from_byte_array([3; 32]),
            git_sha1: hash::RichGitSha1::from_byte_array([4; 20], "blob", 100),
            is_binary: false,
            is_ascii: true,
            is_utf8: true,
            ends_in_newline: true,
            newline_count: 5,
            first_line: Some(FIRST_LINE.to_string()),
            is_generated: false,
            is_partially_generated: false,
            seeded_blake3: hash::Blake3::from_byte_array([5; 32]),
        }
    }

    /// A redacted blobstore holding the content and metadata of `redacted_id` and
    /// `unredacted_id`, with the content key of `redacted_id` redacted, logging redacted
    /// accesses to the returned log file.
    async fn blobstore_with_redacted_content(
        ctx: &CoreContext,
        redacted_id: ContentId,
        unredacted_id: ContentId,
        log_only: bool,
    ) -> Result<(
        RedactedBlobstore<PrefixBlobstore<Memblob>>,
        tempfile::TempPath,
    )> {
        let log_path = tempfile::NamedTempFile::new()?.into_temp_path();
        let redacted_pairs = RedactedBlobs::FromHashMapForTests(Arc::new(hashmap! {
            redacted_id.blobstore_key() => RedactedMetadata {
                task: "redaction task".to_owned(),
                log_only,
            },
        }));
        let blob = RedactedBlobstore::new(
            PrefixBlobstore::new(Memblob::default(), "prefix"),
            RedactedBlobstoreConfig::new(
                Some(Arc::new(redacted_pairs)),
                MononokeScubaSampleBuilder::with_discard().with_log_file(&log_path)?,
            ),
        );

        for content_id in [redacted_id, unredacted_id] {
            let inner = blob.as_inner_unredacted();
            inner
                .put(
                    ctx,
                    content_id.blobstore_key(),
                    BlobstoreBytes::from_bytes("content"),
                )
                .await?;
            let metadata_blob = content_metadata(content_id).into_blob();
            inner
                .put(
                    ctx,
                    metadata_blob.id().blobstore_key(),
                    metadata_blob.into(),
                )
                .await?;
        }

        Ok((blob, log_path))
    }

    async fn get_content_metadata(
        ctx: &CoreContext,
        blob: &RedactedBlobstore<PrefixBlobstore<Memblob>>,
        content_id: ContentId,
    ) -> Result<ContentMetadataV2> {
        let key = ContentMetadataV2Id::from(content_id).blobstore_key();
        let data = blob
            .get(ctx, &key)
            .await?
            .expect("content metadata should be stored");
        ContentMetadataV2::from_bytes(data.as_bytes().as_bytes())
    }

    #[mononoke::fbinit_test]
    async fn test_redacted_key(fb: FacebookInit) {
        let unredacted_key = "foo";
        let redacted_key = "bar";
        let redacted_task = "bar task";

        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);

        let inner = Memblob::default();
        let redacted_pairs = RedactedBlobs::FromHashMapForTests(Arc::new(hashmap! {
            redacted_key.to_owned() => RedactedMetadata {
                task: redacted_task.to_owned(),
                log_only: false,
            },
        }));

        let blob = RedactedBlobstore::new(
            PrefixBlobstore::new(inner, "prefix"),
            RedactedBlobstoreConfig::new(
                Some(Arc::new(redacted_pairs)),
                MononokeScubaSampleBuilder::with_discard(),
            ),
        );

        //Test put with redacted key
        let res = blob
            .put(
                ctx,
                redacted_key.to_owned(),
                BlobstoreBytes::from_bytes("test bar"),
            )
            .await;

        assert_matches!(
            res.expect_err("the key should be redacted").downcast::<RedactionError>(),
            Ok(RedactionError::Redacted(_, ref task)) if task == redacted_task
        );

        //Test key added to the blob
        let res = blob
            .put(
                ctx,
                unredacted_key.to_owned(),
                BlobstoreBytes::from_bytes("test foo"),
            )
            .await;
        assert!(res.is_ok(), "the key should be added successfully");

        // Test accessing a key which is redacted
        let res = blob.get(ctx, redacted_key).await;

        assert_matches!(
            res.expect_err("the key should be redacted").downcast::<RedactionError>(),
            Ok(RedactionError::Redacted(_, ref task)) if *task == redacted_task
        );

        // Test accessing a key which exists and is accessible
        let res = blob.get(ctx, unredacted_key).await;
        assert!(res.is_ok(), "the key should be found and available");
    }

    #[mononoke::fbinit_test]
    async fn test_redaction_log_only_sources(fb: FacebookInit) -> Result<()> {
        let redacted_log_only_key = "bar";
        let redacted_task = "bar task";
        let ctx = CoreContext::test_mock(fb);
        borrowed!(ctx);

        for (metadata_log_only, config_log_only) in [(true, false), (false, true)] {
            let inner = Memblob::default();
            let redacted_pairs = RedactedBlobs::FromHashMapForTests(Arc::new(hashmap! {
                redacted_log_only_key.to_owned() => RedactedMetadata {
                    task: redacted_task.to_owned(),
                    log_only: metadata_log_only,
                },
            }));
            let config = RedactedBlobstoreConfig::new(
                Some(Arc::new(redacted_pairs)),
                MononokeScubaSampleBuilder::with_discard(),
            );
            let config = if config_log_only {
                config.with_log_only_redaction()
            } else {
                config
            };
            let blob = RedactedBlobstore::new(PrefixBlobstore::new(inner, "prefix"), config);

            let val = BlobstoreBytes::from_bytes("test bar");
            blob.put(ctx, redacted_log_only_key.to_owned(), val.clone())
                .await?;

            let actual = blob.get(ctx, redacted_log_only_key).await?;
            assert_eq!(
                Some(val),
                actual.map(|val| val.into_bytes()),
                "log-only via metadata={metadata_log_only}, config={config_log_only} should permit access"
            );
        }

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_redacted_content_metadata_hides_first_line(fb: FacebookInit) -> Result<()> {
        // What it tests: a read of the metadata of redacted content, with redaction enforced.
        // Expected: the first line is removed, the other fields are unchanged, the read is
        // logged as enforced, and the metadata of unredacted content is returned unchanged
        // and not logged.
        let ctx = CoreContext::test_mock(fb);
        let redacted_id = ContentId::from_bytes([1; 32])?;
        let unredacted_id = ContentId::from_bytes([6; 32])?;
        let (blob, log_path) =
            blobstore_with_redacted_content(&ctx, redacted_id, unredacted_id, false).await?;

        assert_matches!(
            blob.get(&ctx, &redacted_id.blobstore_key())
                .await
                .expect_err("the content should be redacted")
                .downcast::<RedactionError>(),
            Ok(RedactionError::Redacted(..))
        );

        let metadata = get_content_metadata(&ctx, &blob, redacted_id).await?;
        assert_eq!(
            metadata,
            ContentMetadataV2 {
                first_line: None,
                ..content_metadata(redacted_id)
            },
            "only the first line of the redacted content should be removed"
        );

        let metadata = get_content_metadata(&ctx, &blob, unredacted_id).await?;
        assert_eq!(
            metadata,
            content_metadata(unredacted_id),
            "the metadata of unredacted content should be unchanged"
        );

        let log = std::fs::read_to_string(&log_path)?;
        let metadata_key = ContentMetadataV2Id::from(redacted_id).blobstore_key();
        let unredacted_metadata_key = ContentMetadataV2Id::from(unredacted_id).blobstore_key();
        let metadata_rows = log
            .lines()
            .filter(|row| row.contains(&metadata_key))
            .collect::<Vec<_>>();
        assert_eq!(
            metadata_rows.len(),
            1,
            "the metadata read should be logged once: {log}"
        );
        assert!(
            metadata_rows[0].contains(r#""enforced":1"#),
            "the metadata read should be logged as enforced: {log}"
        );
        assert!(
            !log.contains(&unredacted_metadata_key),
            "the metadata read of unredacted content should not be logged: {log}"
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_log_only_redacted_content_metadata(fb: FacebookInit) -> Result<()> {
        // What it tests: a read of the metadata of redacted content, with log-only redaction.
        // Expected: the metadata is unchanged and the read is logged as not enforced.
        let ctx = CoreContext::test_mock(fb);
        let redacted_id = ContentId::from_bytes([1; 32])?;
        let unredacted_id = ContentId::from_bytes([6; 32])?;
        let (blob, log_path) =
            blobstore_with_redacted_content(&ctx, redacted_id, unredacted_id, true).await?;

        let metadata = get_content_metadata(&ctx, &blob, redacted_id).await?;
        assert_eq!(
            metadata,
            content_metadata(redacted_id),
            "log-only redaction should not change the metadata"
        );

        let log = std::fs::read_to_string(&log_path)?;
        let metadata_key = ContentMetadataV2Id::from(redacted_id).blobstore_key();
        let metadata_rows = log
            .lines()
            .filter(|row| row.contains(&metadata_key))
            .collect::<Vec<_>>();
        assert_eq!(
            metadata_rows.len(),
            1,
            "the metadata read should be logged once: {log}"
        );
        assert!(
            metadata_rows[0].contains(r#""enforced":0"#),
            "the metadata read should be logged as not enforced: {log}"
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unreadable_redacted_content_metadata(fb: FacebookInit) -> Result<()> {
        // What it tests: a read of metadata of redacted content that cannot be parsed.
        // Expected: the read is denied rather than returning the stored bytes.
        let ctx = CoreContext::test_mock(fb);
        let redacted_id = ContentId::from_bytes([1; 32])?;
        let unredacted_id = ContentId::from_bytes([6; 32])?;
        let (blob, _log_path) =
            blobstore_with_redacted_content(&ctx, redacted_id, unredacted_id, false).await?;
        let key = ContentMetadataV2Id::from(redacted_id).blobstore_key();
        let inner = blob.as_inner_unredacted();
        inner
            .put_explicit(
                &ctx,
                key.clone(),
                BlobstoreBytes::from_bytes(FIRST_LINE),
                PutBehaviour::Overwrite,
            )
            .await?;

        assert_matches!(
            blob.get(&ctx, &key)
                .await
                .expect_err("unreadable metadata of redacted content should be denied")
                .downcast::<RedactionError>(),
            Ok(RedactionError::Redacted(..))
        );
        Ok(())
    }
}
