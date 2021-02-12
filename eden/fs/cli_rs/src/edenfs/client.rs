/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! EdenFsClient - abstraction around the bare Thrift client to EdenFS daemon.
//!
//! This layer converts bare data type into structed data types.

use std::sync::Arc;

use anyhow::{Context, Result};

use thrift_types::edenfs::client::EdenService;

pub struct EdenFsClient {
    client: Arc<dyn EdenService>,
}

impl EdenFsClient {
    pub(super) fn new(client: Arc<impl EdenService + 'static>) -> Self {
        Self { client }
    }

    /// Retrieving EdenFS process status from fb303. This function returns PID if the EdenFS daemon
    /// is healthy, otherwise errors of why it's unhealthy.
    pub async fn status(&self) -> Result<i32> {
        let result = self.client.getDaemonInfo().await;

        match result {
            Ok(result) => Ok(result.pid),
            Err(e) => Err(e).context("Unable to retrieve health information from Thrift"),
        }
    }
}
