/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use cxxerror::Result;
use manifold::manifold::ManifoldClient;
use manifold::manifold::RequestContext;

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
    _client_identity: &str,
) -> Result<()> {
    let client = ManifoldClient::new(request_context(bucket, api_key, timeout_msec));

    client
        .write(key.to_owned(), content.to_vec(), expiration_secs)
        .with_context(|| format!("Failed to write key {key} to Manifold bucket {bucket}"))?;

    Ok(())
}

fn request_context(bucket: &str, api_key: &str, timeout_msec: i32) -> RequestContext {
    RequestContext {
        bucket_name: bucket.to_owned(),
        api_key: api_key.to_owned(),
        timeout_msec,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_context_preserves_connection_settings() {
        let context = request_context("bucket", "api-key", 1234);

        assert_eq!(context.bucket_name, "bucket");
        assert_eq!(context.api_key, "api-key");
        assert_eq!(context.timeout_msec, 1234);
    }
}
