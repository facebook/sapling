/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use edenfs_error::Result;

use crate::instance::EdenFsInstance;
use crate::EdenFsThriftClient;
use crate::StreamingEdenFsThriftClient;

pub struct EdenFsClient {
    pub(crate) client: EdenFsThriftClient,
}

impl EdenFsClient {
    pub(crate) async fn new(instance: &EdenFsInstance, timeout: Option<Duration>) -> Result<Self> {
        let client = instance.connect(timeout).await?;
        Ok(Self { client })
    }
}

pub struct StreamingEdenFsClient {
    pub(crate) streaming_client: StreamingEdenFsThriftClient,
}

impl StreamingEdenFsClient {
    pub(crate) async fn new(instance: &EdenFsInstance, timeout: Option<Duration>) -> Result<Self> {
        let streaming_client = instance.connect_streaming(timeout).await?;
        Ok(Self { streaming_client })
    }
}
