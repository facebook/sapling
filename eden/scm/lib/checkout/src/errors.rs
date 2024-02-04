/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
