/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use context::CoreContext;
use futures::future::{err, BoxFuture, FutureExt, TryFutureExt};

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

impl Blobstore for DisabledBlob {
    fn get(
        &self,
        _ctx: CoreContext,
        _key: String,
    ) -> BoxFuture<'_, Result<Option<BlobstoreGetData>, Error>> {
        err(format_err!("Blobstore disabled: {}", self.reason)).boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'_, Result<(), Error>> {
        BlobstorePutOps::put_with_status(self, ctx, key, value)
            .map_ok(|_| ())
            .boxed()
    }
}

impl BlobstorePutOps for DisabledBlob {
    fn put_explicit(
        &self,
        _ctx: CoreContext,
        _key: String,
        _value: BlobstoreBytes,
        _put_behaviour: PutBehaviour,
    ) -> BoxFuture<'_, Result<OverwriteStatus, Error>> {
        err(format_err!("Blobstore disabled: {}", self.reason)).boxed()
    }

    fn put_with_status(
        &self,
        _ctx: CoreContext,
        _key: String,
        _value: BlobstoreBytes,
    ) -> BoxFuture<'_, Result<OverwriteStatus, Error>> {
        err(format_err!("Blobstore disabled: {}", self.reason)).boxed()
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
