/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[derive(Debug, thiserror::Error)]
#[error("checkout error: {source}")]
pub struct CheckoutError {
    pub resumable: bool,
    pub source: anyhow::Error,
}

#[derive(Debug, thiserror::Error)]
#[error("error updating {path}: {message}")]
pub struct EdenConflictError {
    pub path: String,
    pub message: String,
}
