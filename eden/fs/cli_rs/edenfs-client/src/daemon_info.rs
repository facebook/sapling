/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use futures::StreamExt;
use futures::stream::BoxStream;
use tracing::Level;
use tracing::event;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::methods::EdenThriftMethod;
use crate::types::DaemonInfo;
use crate::types::Fb303Status;

pub trait DaemonHealthy {
    fn is_healthy(&self) -> bool;
}

impl DaemonHealthy for DaemonInfo {
    fn is_healthy(&self) -> bool {
        self.status
            .map_or_else(|| false, |val| val == Fb303Status::Alive)
    }
}

impl EdenFsClient {
    pub async fn get_health(&self, timeout: Option<Duration>) -> Result<DaemonInfo> {
        event!(Level::DEBUG, "connected to EdenFS daemon");
        self.with_thrift_with_timeouts(
            timeout.or_else(|| Some(Duration::from_secs(3))),
            None,
            |thrift| (thrift.getDaemonInfo(), EdenThriftMethod::GetDaemonInfo),
        )
        .await
        .with_context(|| "failed to get default eden daemon info")
        .map(|daemon_info| daemon_info.into())
        .map_err(EdenFsError::from)
    }

    pub async fn get_health_with_startup_updates_included(
        &self,
        timeout: Duration,
    ) -> Result<(DaemonInfo, BoxStream<'static, Result<Vec<u8>>>)> {
        let (daemon_info, stream) = self
            .with_thrift_with_timeouts(Some(timeout), None, |thrift| {
                (
                    thrift.streamStartStatus(),
                    EdenThriftMethod::StreamStartStatus,
                )
            })
            .await
            .with_context(|| "failed to get start status stream")
            .map(|(daemon_info, stream)| (daemon_info.into(), stream))
            .map_err(EdenFsError::from)?;

        let stream = stream
            .map(|item| match item {
                Err(thrift_streaming_clients::errors::StreamStartStatusStreamError::ApplicationException(e))
                    if e.type_ == thrift_types::fbthrift::ApplicationExceptionErrorCode::UnknownMethod =>
                {
                    Err(EdenFsError::UnknownMethod(e.message))
                }
                Err(e) => Err(EdenFsError::Other(e.into())),
                Ok(r) => Ok(r),
            })
            .boxed();

        Ok((daemon_info, stream))
    }
}
