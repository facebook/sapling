/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_utils::bytes_from_path;
use futures::StreamExt;
use futures::stream::BoxStream;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::methods::EdenThriftMethod;
use crate::types::JournalPosition;
use crate::utils::get_mount_point;

impl EdenFsClient {
    pub async fn get_journal_position(
        &self,
        mount_point: &Option<PathBuf>,
    ) -> Result<JournalPosition> {
        let mount_point_path = get_mount_point(mount_point)?;
        let mount_point = bytes_from_path(mount_point_path)?;
        self.with_thrift(|thrift| {
            (
                thrift.getCurrentJournalPosition(&mount_point),
                EdenThriftMethod::GetCurrentJournalPosition,
            )
        })
        .await
        .map(|p| p.into())
        .from_err()
    }

    pub async fn stream_journal_changed(
        &self,
        mount_point: &Option<PathBuf>,
    ) -> Result<BoxStream<'static, Result<JournalPosition>>> {
        let mount_point_vec = bytes_from_path(get_mount_point(mount_point)?)?;
        Ok(self
            .with_thrift(|thrift| {
                (
                    thrift.streamJournalChanged(&mount_point_vec),
                    EdenThriftMethod::StreamJournalChanged,
                )
            })
            .await
            .from_err()?
            .map(|item| match item {
                Ok(item) => Ok(item.into()),
                Err(e) => Err(EdenFsError::from(anyhow!(e))),
            })
            .boxed())
    }
}
