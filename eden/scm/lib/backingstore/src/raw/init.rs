/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Once;

static RUST_INIT: Once = Once::new();

/// We use this function to ensure everything we need to initialized as the Rust code may not be
/// called when EdenFS starts. Right now it only calls `env_logger::init` so we can see logs from
/// `edenapi` and other crates. In longer term we should bridge the logs to folly logging.
pub(crate) fn backingstore_global_init() {
    RUST_INIT.call_once(|| {
        env_logger::init();
    });
}
