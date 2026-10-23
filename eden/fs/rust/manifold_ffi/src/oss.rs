/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Open-source builds have no Manifold client, so uploads fail with
//! `Unsupported`, which `StackTraceUploader` logs like any other failed upload.

use cxxerror::Result;

pub fn manifold_write(
    _bucket: &str,
    _api_key: &str,
    _key: &str,
    _content: &[u8],
    _timeout_msec: i32,
    _expiration_secs: u32,
    _client_identity: &str,
) -> Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Manifold uploads are unavailable in this build",
    )
    .into())
}
