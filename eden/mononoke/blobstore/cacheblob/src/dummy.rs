/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use futures_ext::{BoxFuture, FutureExt};
use futures_old::IntoFuture;

use blobstore::BlobstoreGetData;

use crate::{CacheOps, LeaseOps};

/// A dummy implementation of LeaseOps that meets the letter of the spec, but uselessly
#[derive(Clone, Debug)]
pub struct DummyLease {}

impl LeaseOps for DummyLease {
    fn try_add_put_lease(&self, _key: &str) -> BoxFuture<bool, ()> {
        Ok(true).into_future().boxify()
    }

    fn renew_lease_until(&self, _ctx: CoreContext, _key: &str, _done: BoxFuture<(), ()>) {}

    fn wait_for_other_leases(&self, _key: &str) -> BoxFuture<(), ()> {
        Ok(()).into_future().boxify()
    }

    fn release_lease(&self, _key: &str) -> BoxFuture<(), ()> {
        Ok(()).into_future().boxify()
    }
}

/// A dummy implementation of CacheOps that meets the letter of the spec, but uselessly
#[derive(Clone, Debug)]
pub struct DummyCache {}

impl CacheOps for DummyCache {
    fn get(&self, _key: &str) -> BoxFuture<Option<BlobstoreGetData>, ()> {
        Ok(None).into_future().boxify()
    }

    fn put(&self, _key: &str, _value: BlobstoreGetData) -> BoxFuture<(), ()> {
        Ok(()).into_future().boxify()
    }

    fn check_present(&self, _key: &str) -> BoxFuture<bool, ()> {
        Ok(false).into_future().boxify()
    }
}
