/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! [`EdenFsDaemon`] implemented over the full thriftclient-backed
//! [`EdenFsClient`], so daemon calls made by the `edenfs-core` layer keep
//! this crate's retry, reconnect, and telemetry behavior.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use edenfs_core::daemon::DaemonError;
use edenfs_core::daemon::DaemonResult;
use edenfs_core::daemon::EdenFsDaemon;
use edenfs_core::daemon::encode_path;
use edenfs_core::daemon::request_error;
use edenfs_error::ConnectAndRequestError;
use thrift_types::edenfs::MountId;
use thrift_types::edenfs::MountInfo;
use thrift_types::edenfs::UnmountArgument;
use thrift_types::edenfs_clients::errors::UnmountV2Error;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::methods::EdenThriftMethod;

fn classify<E>(method: &'static str, error: ConnectAndRequestError<E>) -> DaemonError
where
    E: std::error::Error + Send + Sync + 'static,
{
    match error {
        ConnectAndRequestError::ConnectionError(error) => DaemonError::Connect(error.into()),
        ConnectAndRequestError::RequestError(error) => request_error(method, error),
    }
}

#[cfg(target_os = "linux")]
pub(crate) async fn add_bind_mount_with_client(
    client: &EdenFsClient,
    mount_point: &Path,
    repo_path: &Path,
    target_path: &Path,
) -> DaemonResult<()> {
    let mount_point = encode_path("addBindMount", mount_point)?;
    let repo_path = encode_path("addBindMount", repo_path)?;
    let target_path = encode_path("addBindMount", target_path)?;
    client
        .with_thrift(|thrift| {
            (
                thrift.addBindMount(&mount_point, &repo_path, &target_path),
                EdenThriftMethod::AddBindMount,
            )
        })
        .await
        .map_err(|error| classify("addBindMount", error))
}

#[cfg(target_os = "linux")]
pub(crate) async fn remove_bind_mount_with_client(
    client: &EdenFsClient,
    mount_point: &Path,
    repo_path: &Path,
) -> DaemonResult<()> {
    let mount_point = encode_path("removeBindMount", mount_point)?;
    let repo_path = encode_path("removeBindMount", repo_path)?;
    client
        .with_thrift(|thrift| {
            (
                thrift.removeBindMount(&mount_point, &repo_path),
                EdenThriftMethod::RemoveBindMount,
            )
        })
        .await
        .map_err(|error| classify("removeBindMount", error))
}

#[async_trait]
impl EdenFsDaemon for EdenFsClient {
    async fn list_mounts(&self, conn_timeout: Option<Duration>) -> DaemonResult<Vec<MountInfo>> {
        self.with_thrift_with_timeouts(conn_timeout, None, |thrift| {
            (thrift.listMounts(), EdenThriftMethod::ListMounts)
        })
        .await
        .map_err(|error| classify("listMounts", error))
    }

    async fn unmount_v2(&self, mount_point: &Path, use_force: bool) -> DaemonResult<()> {
        let unmount_argument = UnmountArgument {
            mountId: MountId {
                mountPoint: encode_path("unmountV2", mount_point)?,
                ..Default::default()
            },
            useForce: use_force,
            ..Default::default()
        };
        match self
            .with_thrift(|thrift| {
                (
                    thrift.unmountV2(&unmount_argument),
                    EdenThriftMethod::UnmountV2,
                )
            })
            .await
        {
            Ok(_) => Ok(()),
            Err(ConnectAndRequestError::RequestError(UnmountV2Error::ApplicationException(
                ref e,
            ))) if e.type_ == ApplicationExceptionErrorCode::UnknownMethod => {
                Err(DaemonError::UnknownMethod {
                    method: "unmountV2",
                })
            }
            Err(error) => Err(classify("unmountV2", error)),
        }
    }

    async fn unmount_legacy(&self, mount_point: &Path) -> DaemonResult<()> {
        let encoded_path = encode_path("unmount", mount_point)?;
        self.with_thrift(|thrift| (thrift.unmount(&encoded_path), EdenThriftMethod::Unmount))
            .await
            .map_err(|error| classify("unmount", error))
    }

    #[cfg(target_os = "linux")]
    async fn add_bind_mount(
        &self,
        mount_point: &Path,
        repo_path: &Path,
        target_path: &Path,
    ) -> DaemonResult<()> {
        add_bind_mount_with_client(self, mount_point, repo_path, target_path).await
    }

    #[cfg(target_os = "linux")]
    async fn remove_bind_mount(&self, mount_point: &Path, repo_path: &Path) -> DaemonResult<()> {
        remove_bind_mount_with_client(self, mount_point, repo_path).await
    }
}
