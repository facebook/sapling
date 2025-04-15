/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::anyhow;
use anyhow::Context;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;
use thrift_thriftclients::thrift::errors::UnmountV2Error;
use thrift_types::edenfs::MountId;
use thrift_types::edenfs::UnmountArgument;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;

use crate::client::EdenFsClient;
use crate::instance::EdenFsInstance;

impl EdenFsClient {
    pub async fn unmount(&self, path: &Path, no_force: bool) -> Result<()> {
        let encoded_path = bytes_from_path(path.to_path_buf())
            .with_context(|| format!("Failed to encode path {}", path.display()))?;

        let unmount_argument = UnmountArgument {
            mountId: MountId {
                mountPoint: encoded_path,
                ..Default::default()
            },
            useForce: !no_force,
            ..Default::default()
        };
        let instance = EdenFsInstance::global();
        match self
            .with_thrift(|thrift| thrift.unmountV2(&unmount_argument))
            .await
        {
            Ok(_) => {
                instance.create_intentional_unmount_flag(path)?;
                Ok(())
            }
            Err(ConnectAndRequestError::RequestError(UnmountV2Error::ApplicationException(
                ref e,
            ))) => {
                if e.type_ == ApplicationExceptionErrorCode::UnknownMethod {
                    let encoded_path = bytes_from_path(path.to_path_buf())
                        .with_context(|| format!("Failed to encode path {}", path.display()))?;
                    self.with_thrift(|thrift| thrift.unmount(&encoded_path))
                        .await
                        .with_context(|| {
                            format!(
                                "Failed to unmount (legacy Thrift unmount endpoint) {}",
                                path.display()
                            )
                        })?;
                    instance.create_intentional_unmount_flag(path)?;
                    Ok(())
                } else {
                    Err(EdenFsError::Other(anyhow!(
                        "Failed to unmount (Thrift unmountV2 endpoint) {}: {}",
                        path.display(),
                        e
                    )))
                }
            }
            Err(e) => Err(EdenFsError::Other(anyhow!(
                "Failed to unmount (Thrift unmountV2 endpoint) {}: {}",
                path.display(),
                e
            ))),
        }
    }
}
