/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This module contains types that are used by the EdenFS client. These are
//! mostly wrappers around Thrift types, but some are custom types that are
//! commonly used by the client and EdenFS CLI.
//!
//! # Core Types
//!
//! ## Fb303Status
//! An enum representing the status of a service, with variants such as `Dead`, `Starting`, `Alive`, etc.
//! Used to indicate the operational state of the EdenFS daemon.
//!
//! ## DaemonInfo
//! A struct that holds information about a daemon, including its process ID, command line arguments,
//! current status, and uptime in seconds.
//!
//! ## Dtype
//! An enum representing different file types in the filesystem, such as `Unknown`, `Regular`, `Link`,
//! `Socket`, `Char`, `Dir`, etc. Maps to standard POSIX file types.
//!
//! ## JournalPosition
//! A struct that represents a position in EdenFS' journal, including mount generation, sequence number,
//! and snapshot hash. Used for tracking changes and synchronization.
//!
//! ## RootIdOptions
//! A struct that contains additional RootID information, currently only an optional filter ID.
//! Used to customize root ID behavior.
//!
//! ## OSName
//! An enum representing the operating system name, with variants like `Windows`, `Darwin`, `Linux`,
//! and `Unknown`. Provides platform-specific behavior.
//!
//! ## SyncBehavior
//! A struct that defines synchronization behavior, with an optional sync timeout in seconds.
//! Controls how filesystem synchronization is performed.
//!
//! ## FileAttributes
//! An enum representing file attributes, such as `None`, `Sha1`, `FileSize`, etc. There are also
//! convenience methods for converting FileAttributes to a bitmask and vice versa.
//!
//! # Examples
//!
//! ## Getting all available attribute names
//!
//! ```
//! use edenfs_client::types;
//!
//! // Get a list of all available attribute names
//! let all_attrs = types::all_attributes();
//! println!("Available attributes: {:?}", all_attrs);
//! ```
//!
//! ## Converting attribute names to a bitmask
//!
//! ```
//! use edenfs_client::types;
//!
//! // Convert a list of attribute names to a bitmask
//! let attrs = ["Sha1", "FileSize", "SourceControlType"];
//! match types::file_attributes_from_strings(&attrs) {
//!     Ok(bitmask) => println!("Attribute bitmask: {}", bitmask),
//!     Err(e) => eprintln!("Error: {}", e),
//! }
//! ```
//!
//! ## Getting a bitmask for all attributes
//!
//! ```
//! use edenfs_client::types;
//!
//! // Get a bitmask representing all available attributes
//! let all_attrs_bitmask = types::all_attributes_as_bitmask();
//! println!("All attributes bitmask: {}", all_attrs_bitmask);
//! ```

use std::fmt;
use std::fmt::Display;
use std::str::FromStr;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use serde::Serialize;
use thrift_types::fbthrift::ThriftEnum;

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

#[repr(i32)]
#[derive(Debug, Clone, PartialEq)]
pub enum FileAttributes {
    None = 0,
    Sha1 = 1,
    FileSize = 2,
    SourceControlType = 4,
    ObjectId = 8,
    Blake3 = 16,
    DigestSize = 32,
    DigestHash = 64,
}

impl From<FileAttributes> for thrift_types::edenfs::FileAttributes {
    fn from(from: FileAttributes) -> Self {
        match from {
            FileAttributes::None => Self::NONE,
            FileAttributes::Sha1 => Self::SHA1_HASH,
            FileAttributes::FileSize => Self::FILE_SIZE,
            FileAttributes::SourceControlType => Self::SOURCE_CONTROL_TYPE,
            FileAttributes::ObjectId => Self::OBJECT_ID,
            FileAttributes::Blake3 => Self::BLAKE3_HASH,
            FileAttributes::DigestSize => Self::DIGEST_SIZE,
            FileAttributes::DigestHash => Self::DIGEST_HASH,
        }
    }
}

impl From<thrift_types::edenfs::FileAttributes> for FileAttributes {
    fn from(from: thrift_types::edenfs::FileAttributes) -> Self {
        match from {
            thrift_types::edenfs::FileAttributes::NONE => Self::None,
            thrift_types::edenfs::FileAttributes::SHA1_HASH => Self::Sha1,
            thrift_types::edenfs::FileAttributes::FILE_SIZE => Self::FileSize,
            thrift_types::edenfs::FileAttributes::SOURCE_CONTROL_TYPE => Self::SourceControlType,
            thrift_types::edenfs::FileAttributes::OBJECT_ID => Self::ObjectId,
            thrift_types::edenfs::FileAttributes::BLAKE3_HASH => Self::Blake3,
            thrift_types::edenfs::FileAttributes::DIGEST_SIZE => Self::DigestSize,
            thrift_types::edenfs::FileAttributes::DIGEST_HASH => Self::DigestHash,
            _ => Self::None,
        }
    }
}

impl FromStr for FileAttributes {
    type Err = EdenFsError;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "None" => Ok(Self::None),
            "Sha1" => Ok(Self::Sha1),
            "FileSize" => Ok(Self::FileSize),
            "SourceControlType" => Ok(Self::SourceControlType),
            "ObjectId" => Ok(Self::ObjectId),
            "Blake3" => Ok(Self::Blake3),
            "DigestSize" => Ok(Self::DigestSize),
            "DigestHash" => Ok(Self::DigestHash),
            _ => Err(EdenFsError::Other(anyhow!(
                "invalid file attribute: {:?}",
                s
            ))),
        }
    }
}

/// Converts a slice of `FileAttributes` to a bitmask.
///
/// This function takes a slice of `FileAttributes` and returns a bitmask
/// representing those attributes.
///
/// # Parameters
///
/// * `attrs` - A slice of `FileAttributes` to convert to a bitmask.
///
/// # Returns
///
/// A bitmask representing the given attributes.
pub fn attributes_as_bitmask(attrs: &[FileAttributes]) -> i64 {
    attrs.iter().fold(0, |acc, x| acc | x.clone() as i64)
}

/// Returns a bitmask representing all available file attributes.
///
/// This function returns a bitmask that includes all available file attributes.
///
/// # Returns
///
/// A bitmask representing all available file attributes.
///
/// # Examples
///
/// ```
/// use edenfs_client::types;
///
/// let all_attrs_bitmask = types::all_attributes_as_bitmask();
/// println!("All attributes bitmask: {}", all_attrs_bitmask);
/// ```
pub fn all_attributes_as_bitmask() -> i64 {
    let vals: Vec<FileAttributes> = thrift_types::edenfs::FileAttributes::variant_values()
        .iter()
        .map(|v| v.clone().into())
        .collect();
    attributes_as_bitmask(&vals)
}

/// Returns a slice of all available file attribute names.
///
/// This function returns a slice containing the names of all available file attributes.
///
/// # Returns
///
/// A slice of strings representing all available file attribute names.
///
/// # Examples
///
/// ```
/// use edenfs_client::types;
///
/// let all_attrs = types::all_attributes();
/// println!("Available attributes: {:?}", all_attrs);
/// ```
pub fn all_attributes() -> &'static [&'static str] {
    thrift_types::edenfs::FileAttributes::variants()
}

/// Converts a slice of attribute names to a bitmask.
///
/// This function takes a slice of attribute names and returns a bitmask
/// representing those attributes.
///
/// # Parameters
///
/// * `attrs` - A slice of attribute names to convert to a bitmask.
///
/// # Returns
///
/// A `Result` containing a bitmask representing the given attributes, or an error
/// if any of the attribute names are invalid.
///
/// # Examples
///
/// ```
/// use edenfs_client::types;
///
/// // Convert a list of attribute names to a bitmask
/// let attrs = ["Sha1", "Size", "SourceControlType"];
/// match types::file_attributes_from_strings(&attrs) {
///     Ok(bitmask) => println!("Attribute bitmask: {}", bitmask),
///     Err(e) => eprintln!("Error: {}", e),
/// }
///
/// // Invalid attribute names will result in an error
/// let invalid_attrs = ["Invalid"];
/// assert!(types::file_attributes_from_strings(&invalid_attrs).is_err());
/// ```
pub fn file_attributes_from_strings<T>(attrs: &[T]) -> Result<i64>
where
    T: AsRef<str> + Display,
{
    let attrs: Result<Vec<FileAttributes>, _> = attrs
        .iter()
        .map(|attr| {
            FileAttributes::from_str(attr.as_ref())
                .map_err(|e| EdenFsError::Other(anyhow!("invalid file attribute: {:?}", e)))
        })
        .collect();
    Ok(attributes_as_bitmask(attrs?.as_slice()))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_attributes_from_strings() -> Result<()> {
        assert_eq!(file_attributes_from_strings::<String>(&[])?, 0);
        assert_eq!(
            file_attributes_from_strings(&["Sha1", "SourceControlType"])?,
            FileAttributes::Sha1 as i64 | FileAttributes::SourceControlType as i64
        );
        assert!(file_attributes_from_strings(&["Invalid"]).is_err());
        Ok(())
    }
}
