/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(target_os = "macos")]

use std::path::PathBuf;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;
use edenfs_utils::path_from_bytes;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::methods::EdenThriftMethod;

#[derive(Debug, Clone)]
pub struct StartFileAccessMonitor {
    pub pid: i32,
    pub tmp_output_path: PathBuf,
}

impl From<thrift_types::edenfs::StartFileAccessMonitorResult> for StartFileAccessMonitor {
    fn from(from: thrift_types::edenfs::StartFileAccessMonitorResult) -> Self {
        StartFileAccessMonitor {
            pid: from.pid,
            tmp_output_path: path_from_bytes(&from.tmpOutputPath)
                .expect("Failed to convert StartFileAccessMonitor::tmpOutputPath"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StopFileAccessMonitor {
    pub tmp_output_path: PathBuf,
    pub specified_output_path: PathBuf,
    pub should_upload: bool,
}

impl From<thrift_types::edenfs::StopFileAccessMonitorResult> for StopFileAccessMonitor {
    fn from(from: thrift_types::edenfs::StopFileAccessMonitorResult) -> Self {
        StopFileAccessMonitor {
            tmp_output_path: path_from_bytes(&from.tmpOutputPath)
                .expect("Failed to convert StartFileAccessMonitor::tmpOutputPath"),
            specified_output_path: path_from_bytes(&from.specifiedOutputPath)
                .expect("Failed to convert StartFileAccessMonitor::specifiedOutputPath"),
            should_upload: from.shouldUpload,
        }
    }
}

impl EdenFsClient {
    pub async fn start_file_access_monitor(
        &self,
        path_prefix: &Vec<PathBuf>,
        specified_output_file: Option<PathBuf>,
        should_upload: bool,
    ) -> Result<StartFileAccessMonitor> {
        let mut paths = Vec::new();
        for path in path_prefix {
            let path = bytes_from_path(path.to_path_buf())?;
            paths.push(path);
        }
        let start_file_access_monitor_params = thrift_types::edenfs::StartFileAccessMonitorParams {
            paths,
            specifiedOutputPath: match specified_output_file {
                Some(path) => Some(bytes_from_path(path.to_path_buf())?),
                None => None,
            },
            shouldUpload: should_upload,
            ..Default::default()
        };
        self.with_thrift(|thrift| {
            (
                thrift.startFileAccessMonitor(&start_file_access_monitor_params),
                EdenThriftMethod::StartFileAccessMonitor,
            )
        })
        .await
        .map(|res| res.into())
        .map_err(|e| EdenFsError::from(anyhow!("failed to start file access monitor: {}", e)))
    }

    pub async fn stop_file_access_monitor(&self) -> Result<StopFileAccessMonitor> {
        self.with_thrift(|thrift| {
            (
                thrift.stopFileAccessMonitor(),
                EdenThriftMethod::StopFileAccessMonitor,
            )
        })
        .await
        .map(|res| res.into())
        .map_err(|e| EdenFsError::from(anyhow!("failed to stop file access monitor: {}", e)))
    }
}
