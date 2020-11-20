/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use futures::future::{BoxFuture, FutureExt};

use blobstore::BlobstoreGetData;

use crate::{CacheOps, LeaseOps};

/// A dummy implementation of LeaseOps that meets the letter of the spec, but uselessly
#[derive(Clone, Debug)]
pub struct DummyLease {}

impl LeaseOps for DummyLease {
    fn try_add_put_lease(&self, _key: &str) -> BoxFuture<'_, Result<bool>> {
        async { Ok(true) }.boxed()
    }

    fn renew_lease_until(&self, _ctx: CoreContext, _key: &str, _done: BoxFuture<'static, ()>) {}

    fn wait_for_other_leases(&self, _key: &str) -> BoxFuture<'_, ()> {
        async {}.boxed()
    }

    fn release_lease(&self, _key: &str) -> BoxFuture<'_, ()> {
        async {}.boxed()
    }
}

/// A dummy implementation of CacheOps that meets the letter of the spec, but uselessly
#[derive(Clone, Debug)]
pub struct DummyCache {}

impl CacheOps for DummyCache {
    fn get(&self, _key: &str) -> BoxFuture<'_, Option<BlobstoreGetData>> {
        async { None }.boxed()
    }

    fn put(&self, _key: &str, _value: BlobstoreGetData) -> BoxFuture<'_, ()> {
        async {}.boxed()
    }

    fn check_present(&self, _key: &str) -> BoxFuture<'_, bool> {
        async { false }.boxed()
    }
}
