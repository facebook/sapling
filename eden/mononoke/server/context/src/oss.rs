/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use futures::{future::ok, Future};
use sshrelay::Metadata;

use crate::core::CoreContext;

pub fn is_quicksand(_metadata: &Metadata) -> bool {
    false
}

pub fn is_external_sync(_metadata: &Metadata) -> bool {
    false
}

impl CoreContext {
    pub fn trace_upload(&self) -> impl Future<Item = (), Error = Error> {
        ok(())
    }
}
