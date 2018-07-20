// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};

use LeaseOps;

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
