// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::Error;
use inlinable_string::InlinableString;

use futures_ext::BoxFuture;

use context::CoreContext;

use blobstore::Blobstore;
use mononoke_types::BlobstoreBytes;

/// A layer over an existing blobstore that prepends a fixed string to each get and put.
#[derive(Clone, Debug)]
pub struct PrefixBlobstore<T: Blobstore + Clone> {
    // Try to inline the prefix to ensure copies remain cheap. Most prefixes are short anyway.
    prefix: InlinableString,
    blobstore: T,
}

impl<T: Blobstore + Clone> PrefixBlobstore<T> {
    pub fn new<S: Into<InlinableString>>(blobstore: T, prefix: S) -> Self {
        let prefix = prefix.into();
        Self { prefix, blobstore }
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.blobstore
    }

    #[inline]
    pub fn as_inner(&self) -> &T {
        &self.blobstore
    }

    #[inline]
    pub fn prepend(&self, key: String) -> String {
        [&self.prefix, key.as_str()].concat()
    }
}

impl<T: Blobstore + Clone> Blobstore for PrefixBlobstore<T> {
    #[inline]
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.blobstore.get(ctx, self.prepend(key))
    }

    #[inline]
    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.blobstore.put(ctx, self.prepend(key), value)
    }

    #[inline]
    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.blobstore.is_present(ctx, self.prepend(key))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bytes::Bytes;
    use fbinit::FacebookInit;
    use futures::Future;

    use memblob::EagerMemblob;

    #[fbinit::test]
    fn test_prefix(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let base = EagerMemblob::new();
        let prefixed = PrefixBlobstore::new(base.clone(), "prefix123-");
        let unprefixed_key = "foobar".to_string();
        let prefixed_key = "prefix123-foobar".to_string();

        // This is EagerMemblob (immediate future completion) so calling wait() is fine.
        prefixed
            .put(
                ctx.clone(),
                unprefixed_key.clone(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .wait()
            .expect("put should succeed");

        // Test that both the prefixed and the unprefixed stores can access the key.
        assert_eq!(
            prefixed
                .get(ctx.clone(), unprefixed_key.clone())
                .wait()
                .expect("get should succeed")
                .expect("value should be present")
                .into_bytes(),
            Bytes::from("test foobar"),
        );
        assert_eq!(
            base.get(ctx.clone(), prefixed_key.clone())
                .wait()
                .expect("get should succeed")
                .expect("value should be present")
                .into_bytes(),
            Bytes::from("test foobar"),
        );

        // Test that is_present works for both the prefixed and unprefixed stores.
        assert!(prefixed
            .is_present(ctx.clone(), unprefixed_key.clone())
            .wait()
            .expect("is_present should succeed"));
        assert!(base
            .is_present(ctx.clone(), prefixed_key.clone())
            .wait()
            .expect("is_present should succeed"));
    }
}
