/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use futures::stream::BoxStream;
use futures::StreamExt;
use thrift_streaming_clients::errors::StreamStartStatusStreamError;
use thrift_types::edenfs::DaemonInfo;
use thrift_types::fb303_core::fb303_status;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;
use tracing::event;
use tracing::Level;

use crate::client::EdenFsClient;

pub trait DaemonHealthy {
    fn is_healthy(&self) -> bool;
}

impl DaemonHealthy for DaemonInfo {
    fn is_healthy(&self) -> bool {
        self.status
            .map_or_else(|| false, |val| val == fb303_status::ALIVE)
    }
}

impl EdenFsClient {
    pub async fn get_health(&self, timeout: Option<Duration>) -> Result<DaemonInfo> {
        event!(Level::DEBUG, "connected to EdenFS daemon");
        self.with_thrift_with_timeouts(
            timeout.or_else(|| Some(Duration::from_secs(3))),
            None,
            |thrift| thrift.getDaemonInfo(),
        )
        .await
        .from_err()
    }

    pub async fn get_health_with_startup_updates_included(
        &self,
        timeout: Duration,
    ) -> Result<(DaemonInfo, BoxStream<'static, Result<Vec<u8>>>)> {
        let (daemon_info, stream) = self
            .with_streaming_thrift_with_timeouts(Some(timeout), None, |thrift| {
                thrift.streamStartStatus()
            })
            .await
            .from_err()?;

        let stream = stream
            .map(|item| match item {
                Err(StreamStartStatusStreamError::ApplicationException(e))
                    if e.type_ == ApplicationExceptionErrorCode::UnknownMethod =>
                {
                    Err(EdenFsError::UnknownMethod(e.message))
                }
                r => r.from_err(),
            })
            .boxed();

        Ok((daemon_info, stream))
    }
}
