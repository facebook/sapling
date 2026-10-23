/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;

use facebook::manifold_write;
#[cfg(not(fbcode_build))]
use oss as facebook;

#[cxx::bridge(namespace = "facebook::eden")]
mod ffi {
    extern "Rust" {
        /// Upload content to a Manifold bucket.
        fn manifold_write(
            bucket: &str,
            api_key: &str,
            key: &str,
            content: &[u8],
            timeout_msec: i32,
            expiration_secs: u32,
            client_identity: &str,
        ) -> Result<()>;
    }
}
