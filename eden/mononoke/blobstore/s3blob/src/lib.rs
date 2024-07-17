/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

pub mod s3client;
pub mod store;

pub mod awss3client;

use crate::s3client::S3ClientWrapper;

pub trait S3ClientBackend {
    /// Create a client to connect to the S3 backend.
    fn get_client(&self) -> Arc<S3ClientWrapper>;
    /// Construct the sharded form of the key for a given blobstore key.
    fn get_sharded_key(key: &str) -> String;
}
