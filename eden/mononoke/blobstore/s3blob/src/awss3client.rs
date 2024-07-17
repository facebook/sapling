/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use rusoto_core::Region;
use rusoto_sts::WebIdentityProvider;
use tokio::sync::Semaphore;

use crate::s3client::get_s3_client;
use crate::S3ClientBackend;
use crate::S3ClientWrapper;

pub struct AwsS3ClientPool {
    client: Arc<S3ClientWrapper>,
}

impl S3ClientBackend for AwsS3ClientPool {
    fn get_client(&self) -> Arc<S3ClientWrapper> {
        Arc::clone(&self.client)
    }

    fn get_sharded_key(key: &str) -> String {
        key.to_string()
    }
}

impl AwsS3ClientPool {
    pub async fn new(
        region: Region,
        num_concurrent_operations: Option<usize>,
    ) -> Result<Arc<AwsS3ClientPool>, Error> {
        let semaphore =
            num_concurrent_operations.map(|operations| Arc::new(Semaphore::new(operations)));

        let provider = WebIdentityProvider::from_k8s_env();

        let client = Arc::new(get_s3_client(provider, region, semaphore.clone())?);

        Ok(Arc::new(Self { client }))
    }
}
