/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;

use super::Blobstore;
use super::BlobstoreBytes;
use super::BlobstoreGetData;
use super::BlobstorePutOps;
use super::BlobstoreUnlinkOps;
use super::OverwriteStatus;
use super::PutBehaviour;

/// Disabled blobstore which fails all operations with a reason. Primarily used as a
/// placeholder for administratively disabled blobstores.
#[derive(Debug)]
pub struct DisabledBlob {
    reason: String,
}

impl DisabledBlob {
    pub fn new(reason: impl Into<String>) -> Self {
        DisabledBlob {
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for DisabledBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DisabledBlob ({})", &self.reason)
    }
}

#[async_trait]
impl Blobstore for DisabledBlob {
    async fn get<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        Err(anyhow!("Blobstore disabled: {}", self.reason))
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        BlobstorePutOps::put_with_status(self, ctx, key, value).await?;
        Ok(())
    }

    async fn copy<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _old_key: &'a str,
        _new_key: String,
    ) -> Result<()> {
        Err(anyhow!("Blobstore disabled: {}", self.reason))
    }
}

#[async_trait]
impl BlobstorePutOps for DisabledBlob {
    async fn put_explicit<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _key: String,
        _value: BlobstoreBytes,
        _put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        Err(anyhow!("Blobstore disabled: {}", self.reason))
    }

    async fn put_with_status<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _key: String,
        _value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        Err(anyhow!("Blobstore disabled: {}", self.reason))
    }
}

#[async_trait]
impl BlobstoreUnlinkOps for DisabledBlob {
    async fn unlink<'a>(&'a self, _ctx: &'a CoreContext, _key: &'a str) -> Result<()> {
        Err(anyhow!("Blobstore disabled: {}", self.reason))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;

    #[fbinit::test]
    async fn test_disabled(fb: FacebookInit) {
        let disabled = DisabledBlob::new("test");
        let ctx = CoreContext::test_mock(fb);

        match disabled.get(&ctx, "foobar").await {
            Ok(_) => panic!("Unexpected success"),
            Err(err) => println!("Got error: {:?}", err),
        }

        match disabled
            .put(
                &ctx,
                "foobar".to_string(),
                BlobstoreBytes::from_bytes(vec![]),
            )
            .await
        {
            Ok(_) => panic!("Unexpected success"),
            Err(err) => println!("Got error: {:?}", err),
        }
    }
}
