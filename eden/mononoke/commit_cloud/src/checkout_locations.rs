/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

#[allow(unused)]
pub(crate) struct CheckoutLocation {
    reponame: String,
    hostname: String,
    commit: String,
    checkout_path: PathBuf,
    shared_path: PathBuf,
    timestamp: i64,
    unixname: String,
}
