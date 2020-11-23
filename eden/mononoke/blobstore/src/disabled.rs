/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use context::CoreContext;

use super::{
    Blobstore, BlobstoreBytes, BlobstoreGetData, BlobstorePutOps, OverwriteStatus, PutBehaviour,
};

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
#[async_trait]
impl Blobstore for DisabledBlob {
    async fn get(&self, _ctx: CoreContext, _key: String) -> Result<Option<BlobstoreGetData>> {
        Err(anyhow!("Blobstore disabled: {}", self.reason))
    }

    async fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> Result<()> {
        BlobstorePutOps::put_with_status(self, ctx, key, value).await?;
        Ok(())
    }
}

#[async_trait]
impl BlobstorePutOps for DisabledBlob {
    async fn put_explicit(
        &self,
        _ctx: CoreContext,
        _key: String,
        _value: BlobstoreBytes,
        _put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        Err(anyhow!("Blobstore disabled: {}", self.reason))
    }

    async fn put_with_status(
        &self,
        _ctx: CoreContext,
        _key: String,
        _value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        Err(anyhow!("Blobstore disabled: {}", self.reason))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;

    #[fbinit::test]
    fn test_disabled(fb: FacebookInit) {
        let disabled = DisabledBlob::new("test");
        let ctx = CoreContext::test_mock(fb);

        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

        match runtime.block_on_std(disabled.get(ctx.clone(), "foobar".to_string())) {
            Ok(_) => panic!("Unexpected success"),
            Err(err) => println!("Got error: {:?}", err),
        }

        match runtime.block_on_std(disabled.put(
            ctx,
            "foobar".to_string(),
            BlobstoreBytes::from_bytes(vec![]),
        )) {
            Ok(_) => panic!("Unexpected success"),
            Err(err) => println!("Got error: {:?}", err),
        }
    }
}
