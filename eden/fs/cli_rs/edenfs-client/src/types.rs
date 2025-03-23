/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use serde::Serialize;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Fb303Status {
    Dead = 0,
    Starting = 1,
    Alive = 2,
    Stopping = 3,
    Stopped = 4,
    Warning = 5,
    Undefined = -1,
}

impl From<thrift_types::fb303_core::fb303_status> for Fb303Status {
    fn from(from: thrift_types::fb303_core::fb303_status) -> Self {
        match from {
            thrift_types::fb303_core::fb303_status::DEAD => Self::Dead,
            thrift_types::fb303_core::fb303_status::STARTING => Self::Starting,
            thrift_types::fb303_core::fb303_status::ALIVE => Self::Alive,
            thrift_types::fb303_core::fb303_status::STOPPING => Self::Stopping,
            thrift_types::fb303_core::fb303_status::STOPPED => Self::Stopped,
            thrift_types::fb303_core::fb303_status::WARNING => Self::Warning,
            _ => Self::Undefined,
        }
    }
}

#[derive(Debug)]
pub struct DaemonInfo {
    pub pid: i32,
    pub command_line: Vec<String>,
    pub status: Option<Fb303Status>,
    pub uptime: Option<f32>,
}

impl From<thrift_types::edenfs::DaemonInfo> for DaemonInfo {
    fn from(from: thrift_types::edenfs::DaemonInfo) -> Self {
        Self {
            pid: from.pid,
            command_line: from.commandLine,
            status: from.status.map(|s| s.into()),
            uptime: from.uptime,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub enum Dtype {
    Unknown = 0,
    Fifo = 1,
    Char = 2,
    Dir = 4,
    Block = 6,
    Regular = 8,
    Link = 10,
    Socket = 12,
    Whiteout = 14,
    Undefined = -1,
}

impl From<thrift_types::edenfs::Dtype> for Dtype {
    fn from(from: thrift_types::edenfs::Dtype) -> Self {
        match from {
            thrift_types::edenfs::Dtype::UNKNOWN => Self::Unknown,
            thrift_types::edenfs::Dtype::FIFO => Self::Fifo,
            thrift_types::edenfs::Dtype::CHAR => Self::Char,
            thrift_types::edenfs::Dtype::DIR => Self::Dir,
            thrift_types::edenfs::Dtype::BLOCK => Self::Block,
            thrift_types::edenfs::Dtype::REGULAR => Self::Regular,
            thrift_types::edenfs::Dtype::LINK => Self::Link,
            thrift_types::edenfs::Dtype::SOCKET => Self::Socket,
            thrift_types::edenfs::Dtype::WHITEOUT => Self::Whiteout,
            _ => Self::Undefined,
        }
    }
}

impl fmt::Display for Dtype {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display_str = match *self {
            Dtype::Unknown => "Unknown",
            Dtype::Fifo => "Fifo",
            Dtype::Char => "Char",
            Dtype::Dir => "Dir",
            Dtype::Block => "Block",
            Dtype::Regular => "Regular",
            Dtype::Link => "Link",
            Dtype::Socket => "Socket",
            Dtype::Whiteout => "Whiteout",
            _ => "Undefined",
        };
        write!(f, "{}", display_str)
    }
}

impl PartialEq<i32> for Dtype {
    fn eq(&self, other: &i32) -> bool {
        (*self as i32) == *other
    }
}

impl PartialEq<i16> for Dtype {
    fn eq(&self, other: &i16) -> bool {
        (*self as i16) == *other
    }
}

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
