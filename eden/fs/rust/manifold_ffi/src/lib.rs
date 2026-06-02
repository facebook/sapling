/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use corpmanifold::manifold::ManifoldClient;
use corpmanifold::manifold::RequestContext;
use cxxerror::Result;

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

fn manifold_write(
    bucket: &str,
    api_key: &str,
    key: &str,
    content: &[u8],
    timeout_msec: i32,
    expiration_secs: u32,
    client_identity: &str,
) -> Result<()> {
    let client = ManifoldClient::new(
        unsafe { fbinit::assume_init() },
        client_identity,
        RequestContext {
            bucket_name: bucket.to_owned(),
            api_key: api_key.to_owned(),
            timeout_msec,
        },
    )
    .with_context(|| format!("Failed to create Manifold client for bucket {bucket}"))?;

    client
        .write(key.to_owned(), content.to_vec(), expiration_secs)
        .with_context(|| format!("Failed to write key {key} to Manifold bucket {bucket}"))?;

    Ok(())
}
