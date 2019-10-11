/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use futures::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};

use mononoke_types::BlobstoreBytes;

use crate::{CacheOps, LeaseOps};

/// A dummy implementation of LeaseOps that meets the letter of the spec, but uselessly
#[derive(Clone, Debug)]
pub struct DummyLease {}

impl LeaseOps for DummyLease {
    fn try_add_put_lease(&self, _key: &str) -> BoxFuture<bool, ()> {
        Ok(true).into_future().boxify()
    }

    fn wait_for_other_leases(&self, _key: &str) -> BoxFuture<(), ()> {
        Ok(()).into_future().boxify()
    }

    fn release_lease(&self, _key: &str, _put_success: bool) -> BoxFuture<(), ()> {
        Ok(()).into_future().boxify()
    }
}

/// A dummy implementation of CacheOps that meets the letter of the spec, but uselessly
#[derive(Clone, Debug)]
pub struct DummyCache {}

impl CacheOps for DummyCache {
    fn get(&self, _key: &str) -> BoxFuture<Option<BlobstoreBytes>, ()> {
        Ok(None).into_future().boxify()
    }

    fn put(&self, _key: &str, _value: BlobstoreBytes) -> BoxFuture<(), ()> {
        Ok(()).into_future().boxify()
    }

    fn check_present(&self, _key: &str) -> BoxFuture<bool, ()> {
        Ok(false).into_future().boxify()
    }
}
