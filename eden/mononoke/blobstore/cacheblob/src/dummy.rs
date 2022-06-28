/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use futures::future::BoxFuture;

use blobstore::BlobstoreGetData;

use crate::CacheOps;
use crate::LeaseOps;

/// A dummy implementation of LeaseOps that meets the letter of the spec, but uselessly
#[derive(Clone, Debug)]
pub struct DummyLease {}

impl std::fmt::Display for DummyLease {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "DummyLease")
    }
}

#[async_trait]
impl LeaseOps for DummyLease {
    async fn try_add_put_lease(&self, _key: &str) -> Result<bool> {
        Ok(true)
    }

    fn renew_lease_until(&self, _ctx: CoreContext, _key: &str, _done: BoxFuture<'static, ()>) {}

    async fn wait_for_other_leases(&self, _key: &str) {}

    async fn release_lease(&self, _key: &str) {}
}

/// A dummy implementation of CacheOps that meets the letter of the spec, but uselessly
#[derive(Clone, Debug)]
pub struct DummyCache {}

impl std::fmt::Display for DummyCache {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "DummyCache")
    }
}

#[async_trait]
impl CacheOps for DummyCache {
    async fn get(&self, _key: &str) -> Option<BlobstoreGetData> {
        None
    }

    async fn put(&self, _key: &str, _value: BlobstoreGetData) {}

    async fn check_present(&self, _key: &str) -> bool {
        false
    }
}
