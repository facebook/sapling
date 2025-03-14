/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

#[derive(Clone, Debug, Default)]
pub struct RootIdOptions {
    pub filter_id: Option<String>,
}

impl From<thrift_types::edenfs::RootIdOptions> for RootIdOptions {
    fn from(from: thrift_types::edenfs::RootIdOptions) -> Self {
        Self {
            filter_id: from.filterId,
        }
    }
}

impl From<RootIdOptions> for thrift_types::edenfs::RootIdOptions {
    fn from(from: RootIdOptions) -> thrift_types::edenfs::RootIdOptions {
        thrift_types::edenfs::RootIdOptions {
            filterId: from.filter_id,
            ..Default::default()
        }
    }
}

pub enum OSName {
    Windows,
    Darwin,
    Linux,
    Unknown,
}

impl From<&str> for OSName {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "windows" => Self::Windows,
            "darwin" | "macos" => Self::Darwin,
            "linux" => Self::Linux,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for OSName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                // Matches getOperatingSystemName() in common/telemetry/SessionInfo.cpp
                Self::Windows => "Windows",
                Self::Linux => "Linux",
                Self::Darwin => "macOS",
                Self::Unknown => "unknown",
            }
        )
    }
}

impl Default for OSName {
    fn default() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Darwin
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Unknown
        }
    }
}

pub struct SyncBehavior {
    pub sync_timeout_seconds: Option<i64>,
}

const NO_SYNC: SyncBehavior = SyncBehavior {
    sync_timeout_seconds: None,
};

impl SyncBehavior {
    /// Returns a SyncBehavior object that informs EdenFS that no filesystem synchronization should
    /// be performed before servicing the Thrift request that this SyncBehavior is attached to.
    pub fn no_sync() -> Self {
        NO_SYNC
    }
}

impl From<thrift_types::edenfs::SyncBehavior> for SyncBehavior {
    fn from(from: thrift_types::edenfs::SyncBehavior) -> Self {
        Self {
            sync_timeout_seconds: from.syncTimeoutSeconds,
        }
    }
}

impl From<SyncBehavior> for thrift_types::edenfs::SyncBehavior {
    fn from(from: SyncBehavior) -> thrift_types::edenfs::SyncBehavior {
        thrift_types::edenfs::SyncBehavior {
            syncTimeoutSeconds: from.sync_timeout_seconds,
            ..Default::default()
        }
    }
}
