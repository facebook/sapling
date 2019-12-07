/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobstore::Blobstore;
use context::CoreContext;
use futures::future;
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::BlobstoreBytes;
mod errors;
pub use crate::errors::ErrorKind;

/// A layer over an existing blobstore that prevents writes.
#[derive(Clone, Debug)]
pub struct ReadOnlyBlobstore<T: Blobstore + Clone> {
    blobstore: T,
}

impl<T: Blobstore + Clone> ReadOnlyBlobstore<T> {
    pub fn new(blobstore: T) -> Self {
        Self { blobstore }
    }
}

impl<T: Blobstore + Clone> Blobstore for ReadOnlyBlobstore<T> {
    #[inline]
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.blobstore.get(ctx, key)
    }

    #[inline]
    fn put(&self, _ctx: CoreContext, key: String, _value: BlobstoreBytes) -> BoxFuture<(), Error> {
        future::err(ErrorKind::ReadOnlyPut(key).into()).boxify()
    }

    #[inline]
    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.blobstore.is_present(ctx, key)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use futures::Future;

    use memblob::EagerMemblob;

    #[fbinit::test]
    fn test_error_on_write(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let base = EagerMemblob::new();
        let wrapper = ReadOnlyBlobstore::new(base.clone());
        let key = "foobar".to_string();

        // We're using EagerMemblob (immediate future completion) so calling wait() is fine.
        let r = wrapper
            .put(
                ctx.clone(),
                key.clone(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .wait();
        assert!(!r.is_ok());
        let base_present = base.is_present(ctx, key.clone()).wait().unwrap();
        assert!(!base_present);
    }
}
