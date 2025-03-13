/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_utils::bytes_from_path;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::Serialize;

use crate::client::EdenFsClient;
use crate::utils::get_mount_point;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct JournalPosition {
    pub mount_generation: i64,
    pub sequence_number: u64,
    pub snapshot_hash: Vec<u8>,
}

impl fmt::Display for JournalPosition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.mount_generation,
            self.sequence_number,
            hex::encode(&self.snapshot_hash)
        )
    }
}

impl FromStr for JournalPosition {
    type Err = EdenFsError;

    /// Parse journal position string into a JournalPosition.
    /// Format: "<mount-generation>:<sequence-number>:<hexified-snapshot-hash>"
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.split(':').collect::<Vec<&str>>();
        if parts.len() != 3 {
            return Err(anyhow!(format!("Invalid journal position format: {}", s)).into());
        }

        let mount_generation = parts[0].parse::<i64>().from_err()?;
        let sequence_number = parts[1].parse::<u64>().from_err()?;
        let snapshot_hash = hex::decode(parts[2]).from_err()?;
        Ok(JournalPosition {
            mount_generation,
            sequence_number,
            snapshot_hash,
        })
    }
}

impl From<thrift_types::edenfs::JournalPosition> for JournalPosition {
    fn from(from: thrift_types::edenfs::JournalPosition) -> Self {
        Self {
            mount_generation: from.mountGeneration,
            sequence_number: from.sequenceNumber as u64,
            snapshot_hash: from.snapshotHash,
        }
    }
}

impl From<JournalPosition> for thrift_types::edenfs::JournalPosition {
    fn from(from: JournalPosition) -> thrift_types::edenfs::JournalPosition {
        thrift_types::edenfs::JournalPosition {
            mountGeneration: from.mount_generation,
            sequenceNumber: from.sequence_number as i64,
            snapshotHash: from.snapshot_hash,
            ..Default::default()
        }
    }
}

impl<'a> EdenFsClient<'a> {
    pub async fn get_journal_position(
        &self,
        mount_point: &Option<PathBuf>,
    ) -> Result<JournalPosition> {
        let mount_point_path = get_mount_point(mount_point)?;
        let mount_point = bytes_from_path(mount_point_path)?;
        self.with_thrift(|thrift| thrift.getCurrentJournalPosition(&mount_point))
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
            .with_streaming_thrift(|thrift| thrift.streamJournalChanged(&mount_point_vec))
            .await
            .from_err()?
            .map(|item| match item {
                Ok(item) => Ok(item.into()),
                Err(e) => Err(EdenFsError::from(anyhow!(e))),
            })
            .boxed())
    }
}
