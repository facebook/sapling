// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Error;
use inlinable_string::InlinableString;

use futures_ext::BoxFuture;

use mononoke_types::BlobstoreBytes;

use {Blobstore, CacheBlobstoreExt};

/// A layer over an existing blobstore that prepends a fixed string to each get and put.
#[derive(Clone)]
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
    fn prepend(&self, key: String) -> String {
        [&self.prefix, key.as_str()].concat()
    }
}

impl<T: CacheBlobstoreExt + Clone> CacheBlobstoreExt for PrefixBlobstore<T> {
    #[inline]
    fn get_no_cache_fill(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.blobstore.get_no_cache_fill(self.prepend(key))
    }

    #[inline]
    fn get_cache_only(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.blobstore.get_cache_only(self.prepend(key))
    }
}

impl<T: Blobstore + Clone> Blobstore for PrefixBlobstore<T> {
    #[inline]
    fn get(&self, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.blobstore.get(self.prepend(key))
    }

    #[inline]
    fn put(&self, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        self.blobstore.put(self.prepend(key), value)
    }

    #[inline]
    fn is_present(&self, key: String) -> BoxFuture<bool, Error> {
        self.blobstore.is_present(self.prepend(key))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bytes::Bytes;
    use futures::Future;

    use memblob::EagerMemblob;

    #[test]
    fn test_prefix() {
        let base = EagerMemblob::new();
        let prefixed = PrefixBlobstore::new(base.clone(), "prefix123-");
        let unprefixed_key = "foobar".to_string();
        let prefixed_key = "prefix123-foobar".to_string();

        // This is EagerMemblob (immediate future completion) so calling wait() is fine.
        prefixed
            .put(
                unprefixed_key.clone(),
                BlobstoreBytes::from_bytes("test foobar"),
            )
            .wait()
            .expect("put should succeed");

        // Test that both the prefixed and the unprefixed stores can access the key.
        assert_eq!(
            prefixed
                .get(unprefixed_key.clone())
                .wait()
                .expect("get should succeed")
                .expect("value should be present")
                .into_bytes(),
            Bytes::from("test foobar"),
        );
        assert_eq!(
            base.get(prefixed_key.clone())
                .wait()
                .expect("get should succeed")
                .expect("value should be present")
                .into_bytes(),
            Bytes::from("test foobar"),
        );

        // Test that is_present works for both the prefixed and unprefixed stores.
        assert!(
            prefixed
                .is_present(unprefixed_key.clone())
                .wait()
                .expect("is_present should succeed")
        );
        assert!(
            base.is_present(prefixed_key.clone())
                .wait()
                .expect("is_present should succeed")
        );
    }
}
