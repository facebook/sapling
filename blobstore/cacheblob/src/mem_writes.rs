// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::Error;
use futures::future::Either;
use futures::{Future, IntoFuture};

use futures_ext::{BoxFuture, FutureExt};

use context::CoreContext;
use mononoke_types::BlobstoreBytes;

use blobstore::Blobstore;
use memblob::EagerMemblob;

/// A blobstore wrapper that reads from the underlying blobstore but writes to memory.
#[derive(Clone, Debug)]
pub struct MemWritesBlobstore<T: Blobstore + Clone> {
    inner: T,
    // TODO: replace with chashmap or evmap
    memblob: EagerMemblob,
}

impl<T: Blobstore + Clone> MemWritesBlobstore<T> {
    pub fn new(blobstore: T) -> Self {
        Self {
            inner: blobstore,
            memblob: EagerMemblob::new(),
        }
    }
}

impl<T: Blobstore + Clone> Blobstore for MemWritesBlobstore<T> {
    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        // Don't write the key if it's already present.
        self.is_present(ctx.clone(), key.clone())
            .and_then({
                let memblob = self.memblob.clone();
                move |is_present| {
                    if is_present {
                        Either::A(Ok(()).into_future())
                    } else {
                        Either::B(memblob.put(ctx, key, value))
                    }
                }
            })
            .boxify()
    }

    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        self.memblob
            .get(ctx.clone(), key.clone())
            .and_then({
                let inner = self.inner.clone();
                move |val| match val {
                    Some(val) => Either::A(Ok(Some(val)).into_future()),
                    None => Either::B(inner.get(ctx, key)),
                }
            })
            .boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.memblob
            .is_present(ctx.clone(), key.clone())
            .and_then({
                let inner = self.inner.clone();
                move |is_present| {
                    if is_present {
                        Either::A(Ok(true).into_future())
                    } else {
                        Either::B(inner.is_present(ctx, key))
                    }
                }
            })
            .boxify()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bytes::Bytes;

    #[test]
    fn basic_read() {
        let ctx = CoreContext::test_mock();
        let inner = EagerMemblob::new();
        let foo_key = "foo".to_string();
        inner
            .put(
                ctx.clone(),
                foo_key.clone(),
                BlobstoreBytes::from_bytes("foobar"),
            )
            .wait()
            .expect("initial put should work");
        let outer = MemWritesBlobstore::new(inner.clone());

        assert!(outer
            .is_present(ctx.clone(), foo_key.clone())
            .wait()
            .expect("is_present to inner should work"));

        assert_eq!(
            outer
                .get(ctx, foo_key.clone())
                .wait()
                .expect("get to inner should work")
                .expect("value should be present")
                .into_bytes(),
            Bytes::from("foobar"),
        );
    }

    #[test]
    fn redirect_writes() {
        let ctx = CoreContext::test_mock();
        let inner = EagerMemblob::new();
        let foo_key = "foo".to_string();

        let outer = MemWritesBlobstore::new(inner.clone());
        outer
            .put(
                ctx.clone(),
                foo_key.clone(),
                BlobstoreBytes::from_bytes("foobar"),
            )
            .wait()
            .expect("put should work");

        assert!(
            !inner
                .is_present(ctx.clone(), foo_key.clone())
                .wait()
                .expect("is_present on inner should work"),
            "foo should not be present in inner",
        );

        assert!(
            outer
                .is_present(ctx.clone(), foo_key.clone())
                .wait()
                .expect("is_present on outer should work"),
            "foo should be present in outer",
        );

        assert_eq!(
            outer
                .get(ctx, foo_key.clone())
                .wait()
                .expect("get to outer should work")
                .expect("value should be present")
                .into_bytes(),
            Bytes::from("foobar"),
        );
    }

    #[test]
    fn present_in_inner() {
        let ctx = CoreContext::test_mock();
        let inner = EagerMemblob::new();
        let foo_key = "foo".to_string();
        inner
            .put(
                ctx.clone(),
                foo_key.clone(),
                BlobstoreBytes::from_bytes("foobar"),
            )
            .wait()
            .expect("initial put should work");
        let outer = MemWritesBlobstore::new(inner.clone());
        outer
            .put(
                ctx.clone(),
                foo_key.clone(),
                BlobstoreBytes::from_bytes("foobar"),
            )
            .wait()
            .expect("put should work");

        assert!(
            outer
                .is_present(ctx.clone(), foo_key.clone())
                .wait()
                .expect("is_present on outer should work"),
            "foo should be present in outer",
        );

        // Change the value in inner.
        inner
            .put(
                ctx.clone(),
                foo_key.clone(),
                BlobstoreBytes::from_bytes("bazquux"),
            )
            .wait()
            .expect("second put should work");
        assert_eq!(
            outer
                .get(ctx, foo_key.clone())
                .wait()
                .expect("get to outer should work")
                .expect("value should be present")
                .into_bytes(),
            Bytes::from("bazquux"),
        );
    }
}
