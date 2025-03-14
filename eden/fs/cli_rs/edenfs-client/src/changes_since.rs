/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_utils::bytes_from_path;
use edenfs_utils::path_from_bytes;
use futures::StreamExt;
use serde::Serialize;
use thrift_types::edenfs::ChangesSinceV2Params;
use tokio::time;

use crate::client::EdenFsClient;
use crate::journal::JournalPosition;
use crate::utils::get_mount_point;

#[derive(Debug, PartialEq, Serialize)]
pub struct Dtype(pub i32);

impl Dtype {
    pub const UNKNOWN: Self = Dtype(0);
    pub const FIFO: Self = Dtype(1);
    pub const CHAR: Self = Dtype(2);
    pub const DIR: Self = Dtype(4);
    pub const BLOCK: Self = Dtype(6);
    pub const REGULAR: Self = Dtype(8);
    pub const LINK: Self = Dtype(10);
    pub const SOCKET: Self = Dtype(12);
    pub const WHITEOUT: Self = Dtype(14);
}

impl From<Dtype> for i32 {
    fn from(x: Dtype) -> Self {
        x.0
    }
}

impl From<i32> for Dtype {
    fn from(x: i32) -> Self {
        Self(x)
    }
}

impl From<thrift_types::edenfs::Dtype> for Dtype {
    fn from(x: thrift_types::edenfs::Dtype) -> Self {
        Self(x.0)
    }
}

impl fmt::Display for Dtype {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display_str = match *self {
            Dtype::UNKNOWN => "Unknown",
            Dtype::FIFO => "Fifo",
            Dtype::CHAR => "Char",
            Dtype::DIR => "Dir",
            Dtype::BLOCK => "Block",
            Dtype::REGULAR => "Regular",
            Dtype::LINK => "Link",
            Dtype::SOCKET => "Socket",
            Dtype::WHITEOUT => "Whiteout",
            _ => "Undefined",
        };
        write!(f, "{}", display_str)
    }
}

#[derive(Debug, Serialize)]
pub struct Added {
    pub file_type: Dtype,
    pub path: Vec<u8>,
}

impl fmt::Display for Added {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "({}): '{}'",
            self.file_type,
            path_from_bytes(&self.path)
                .expect("Invalid path.")
                .to_string_lossy()
        )
    }
}

impl From<thrift_types::edenfs::Added> for Added {
    fn from(from: thrift_types::edenfs::Added) -> Self {
        Added {
            file_type: from.fileType.into(),
            path: from.path,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Modified {
    pub file_type: Dtype,
    pub path: Vec<u8>,
}

impl fmt::Display for Modified {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "({}): '{}'",
            self.file_type,
            path_from_bytes(&self.path)
                .expect("Invalid path.")
                .to_string_lossy()
        )
    }
}

impl From<thrift_types::edenfs::Modified> for Modified {
    fn from(from: thrift_types::edenfs::Modified) -> Self {
        Modified {
            file_type: from.fileType.into(),
            path: from.path,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Renamed {
    pub file_type: Dtype,
    pub from: Vec<u8>,
    pub to: Vec<u8>,
}

impl fmt::Display for Renamed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "({}): '{}' -> '{}'",
            self.file_type,
            path_from_bytes(&self.from)
                .expect("Invalid path.")
                .to_string_lossy(),
            path_from_bytes(&self.to)
                .expect("Invalid path.")
                .to_string_lossy()
        )
    }
}

impl From<thrift_types::edenfs::Renamed> for Renamed {
    fn from(from: thrift_types::edenfs::Renamed) -> Self {
        Renamed {
            file_type: from.fileType.into(),
            from: from.from,
            to: from.to,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Replaced {
    pub file_type: Dtype,
    pub from: Vec<u8>,
    pub to: Vec<u8>,
}

impl fmt::Display for Replaced {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "({}): '{}' -> '{}'",
            self.file_type,
            path_from_bytes(&self.from)
                .expect("Invalid path.")
                .to_string_lossy(),
            path_from_bytes(&self.to)
                .expect("Invalid path.")
                .to_string_lossy()
        )
    }
}

impl From<thrift_types::edenfs::Replaced> for Replaced {
    fn from(from: thrift_types::edenfs::Replaced) -> Self {
        Replaced {
            file_type: from.fileType.into(),
            from: from.from,
            to: from.to,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Removed {
    pub file_type: Dtype,
    pub path: Vec<u8>,
}

impl fmt::Display for Removed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "({}): '{}'",
            self.file_type,
            path_from_bytes(&self.path)
                .expect("Invalid path.")
                .to_string_lossy()
        )
    }
}

impl From<thrift_types::edenfs::Removed> for Removed {
    fn from(from: thrift_types::edenfs::Removed) -> Self {
        Removed {
            file_type: from.fileType.into(),
            path: from.path,
        }
    }
}

#[derive(Debug, Serialize)]
pub enum SmallChangeNotification {
    Added(Added),
    Modified(Modified),
    Renamed(Renamed),
    Replaced(Replaced),
    Removed(Removed),
}

impl fmt::Display for SmallChangeNotification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SmallChangeNotification::Added(added) => write!(f, "added {}", added),
            SmallChangeNotification::Modified(modified) => write!(f, "modified {}", modified),
            SmallChangeNotification::Renamed(renamed) => write!(f, "renamed {}", renamed),
            SmallChangeNotification::Replaced(replaced) => write!(f, "replaced {}", replaced),
            SmallChangeNotification::Removed(removed) => write!(f, "removed {}", removed),
        }
    }
}

impl From<thrift_types::edenfs::SmallChangeNotification> for SmallChangeNotification {
    fn from(from: thrift_types::edenfs::SmallChangeNotification) -> Self {
        match from {
            thrift_types::edenfs::SmallChangeNotification::added(added) => {
                SmallChangeNotification::Added(added.into())
            }
            thrift_types::edenfs::SmallChangeNotification::modified(modified) => {
                SmallChangeNotification::Modified(modified.into())
            }
            thrift_types::edenfs::SmallChangeNotification::renamed(renamed) => {
                SmallChangeNotification::Renamed(renamed.into())
            }
            thrift_types::edenfs::SmallChangeNotification::replaced(replaced) => {
                SmallChangeNotification::Replaced(replaced.into())
            }
            thrift_types::edenfs::SmallChangeNotification::removed(removed) => {
                SmallChangeNotification::Removed(removed.into())
            }
            _ => panic!("Unknown SmallChangeNotification"),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DirectoryRenamed {
    pub from: Vec<u8>,
    pub to: Vec<u8>,
}

impl fmt::Display for DirectoryRenamed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "'{}' -> '{}'",
            path_from_bytes(&self.from)
                .expect("Invalid path.")
                .to_string_lossy(),
            path_from_bytes(&self.to)
                .expect("Invalid path.")
                .to_string_lossy()
        )
    }
}

impl From<thrift_types::edenfs::DirectoryRenamed> for DirectoryRenamed {
    fn from(from: thrift_types::edenfs::DirectoryRenamed) -> Self {
        DirectoryRenamed {
            from: from.from,
            to: from.to,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CommitTransition {
    pub from: Vec<u8>,
    pub to: Vec<u8>,
}

impl fmt::Display for CommitTransition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "'{}' -> '{}'",
            hex::encode(&self.from),
            hex::encode(&self.to)
        )
    }
}

impl From<thrift_types::edenfs::CommitTransition> for CommitTransition {
    fn from(from: thrift_types::edenfs::CommitTransition) -> Self {
        CommitTransition {
            from: from.from,
            to: from.to,
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub struct LostChangesReason(pub i32);

impl LostChangesReason {
    pub const UNKNOWN: Self = LostChangesReason(0);
    pub const EDENFS_REMOUNTED: Self = LostChangesReason(1);
    pub const JOURNAL_TRUNCATED: Self = LostChangesReason(2);
    pub const TOO_MANY_CHANGES: Self = LostChangesReason(3);
}

impl From<LostChangesReason> for i32 {
    fn from(x: LostChangesReason) -> Self {
        x.0
    }
}

impl From<i32> for LostChangesReason {
    fn from(x: i32) -> Self {
        Self(x)
    }
}

impl From<thrift_types::edenfs::LostChangesReason> for LostChangesReason {
    fn from(x: thrift_types::edenfs::LostChangesReason) -> Self {
        Self(x.0)
    }
}

impl fmt::Display for LostChangesReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display_str = match *self {
            LostChangesReason::UNKNOWN => "Unknown",
            LostChangesReason::EDENFS_REMOUNTED => "EdenFSRemounted",
            LostChangesReason::JOURNAL_TRUNCATED => "JournalTruncated",
            LostChangesReason::TOO_MANY_CHANGES => "TooManyChanges",
            _ => "Undefined",
        };
        write!(f, "{}", display_str)
    }
}

#[derive(Debug, Serialize)]
pub struct LostChanges {
    pub reason: LostChangesReason,
}

impl fmt::Display for LostChanges {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.reason)
    }
}

impl From<thrift_types::edenfs::LostChanges> for LostChanges {
    fn from(from: thrift_types::edenfs::LostChanges) -> Self {
        LostChanges {
            reason: from.reason.into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub enum LargeChangeNotification {
    DirectoryRenamed(DirectoryRenamed),
    CommitTransition(CommitTransition),
    LostChanges(LostChanges),
}

impl fmt::Display for LargeChangeNotification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LargeChangeNotification::DirectoryRenamed(directory_renamed) => {
                write!(f, "directory_renamed {}", directory_renamed)
            }
            LargeChangeNotification::CommitTransition(commit_transition) => {
                write!(f, "commit_transition {}", commit_transition)
            }
            LargeChangeNotification::LostChanges(lost_changes) => {
                write!(f, "lost_changes {}", lost_changes)
            }
        }
    }
}

impl From<thrift_types::edenfs::LargeChangeNotification> for LargeChangeNotification {
    fn from(from: thrift_types::edenfs::LargeChangeNotification) -> Self {
        match from {
            thrift_types::edenfs::LargeChangeNotification::directoryRenamed(directory_renamed) => {
                LargeChangeNotification::DirectoryRenamed(directory_renamed.into())
            }
            thrift_types::edenfs::LargeChangeNotification::commitTransition(commit_transition) => {
                LargeChangeNotification::CommitTransition(commit_transition.into())
            }
            thrift_types::edenfs::LargeChangeNotification::lostChanges(lost_changes) => {
                LargeChangeNotification::LostChanges(lost_changes.into())
            }
            _ => panic!("Unknown LargeChangeNotification"),
        }
    }
}

#[derive(Debug, Serialize)]
pub enum ChangeNotification {
    SmallChange(SmallChangeNotification),
    LargeChange(LargeChangeNotification),
}

impl fmt::Display for ChangeNotification {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ChangeNotification::SmallChange(small_change) => {
                write!(f, "small: {}", small_change)
            }
            ChangeNotification::LargeChange(large_change) => {
                write!(f, "large: {}", large_change)
            }
        }
    }
}

impl From<thrift_types::edenfs::ChangeNotification> for ChangeNotification {
    fn from(from: thrift_types::edenfs::ChangeNotification) -> Self {
        match from {
            thrift_types::edenfs::ChangeNotification::smallChange(small_change) => {
                ChangeNotification::SmallChange(small_change.into())
            }
            thrift_types::edenfs::ChangeNotification::largeChange(large_change) => {
                ChangeNotification::LargeChange(large_change.into())
            }
            _ => panic!("Unknown ChangeNotification"),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ChangesSinceV2Result {
    pub to_position: JournalPosition,
    pub changes: Vec<ChangeNotification>,
}

impl fmt::Display for ChangesSinceV2Result {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for change in self.changes.iter() {
            writeln!(f, "{change}")?;
        }
        writeln!(f, "position: {}", self.to_position)
    }
}

impl From<thrift_types::edenfs::ChangesSinceV2Result> for ChangesSinceV2Result {
    fn from(from: thrift_types::edenfs::ChangesSinceV2Result) -> Self {
        ChangesSinceV2Result {
            to_position: from.toPosition.into(),
            changes: from.changes.into_iter().map(|c| c.into()).collect(),
        }
    }
}

impl EdenFsClient {
    #[cfg(fbcode_build)]
    pub async fn get_changes_since_with_includes(
        &self,
        mount_point: &Option<PathBuf>,
        from_position: &JournalPosition,
        root: &Option<PathBuf>,
        included_roots: &Option<Vec<PathBuf>>,
        included_suffixes: &Option<Vec<String>>,
    ) -> Result<ChangesSinceV2Result> {
        self.get_changes_since(
            mount_point,
            from_position,
            root,
            included_roots,
            included_suffixes,
            &None,
            &None,
            false,
        )
        .await
    }

    #[cfg(fbcode_build)]
    pub async fn get_changes_since(
        &self,
        mount_point: &Option<PathBuf>,
        from_position: &JournalPosition,
        root: &Option<PathBuf>,
        included_roots: &Option<Vec<PathBuf>>,
        included_suffixes: &Option<Vec<String>>,
        excluded_roots: &Option<Vec<PathBuf>>,
        excluded_suffixes: &Option<Vec<String>>,
        include_vcs_roots: bool,
    ) -> Result<ChangesSinceV2Result> {
        // Temporary code to prefix from roots - will be removed when implemented in daemon
        let included_roots = prefix_paths(root, included_roots, |p| {
            bytes_from_path(p).expect("Failed to convert path to bytes")
        })
        .or_else(|| {
            root.clone()
                .map(|r| vec![bytes_from_path(r).expect("Failed to convert path to bytes")])
        });
        let excluded_roots = prefix_paths(root, excluded_roots, |p| {
            bytes_from_path(p).expect("Failed to convert path to bytes")
        });

        let params = ChangesSinceV2Params {
            mountPoint: bytes_from_path(get_mount_point(mount_point)?)?,
            fromPosition: from_position.clone().into(),
            includeVCSRoots: Some(include_vcs_roots),
            includedRoots: included_roots,
            includedSuffixes: included_suffixes.clone(),
            excludedRoots: excluded_roots,
            excludedSuffixes: excluded_suffixes.clone(),
            ..Default::default()
        };
        let mut result: ChangesSinceV2Result = self
            .with_thrift(|thrift| thrift.changesSinceV2(&params))
            .await
            .map(|r| r.into())
            .from_err()?;
        // Temporary code to strip prefix from paths - will be removed when implemented in daemon
        if root.is_some() {
            result.changes.iter_mut().for_each(|c| match c {
                ChangeNotification::LargeChange(LargeChangeNotification::DirectoryRenamed(
                    ref mut d,
                )) => {
                    d.from = strip_prefix_from_bytes(root, &d.from);
                    d.to = strip_prefix_from_bytes(root, &d.to);
                }
                ChangeNotification::SmallChange(SmallChangeNotification::Added(a)) => {
                    a.path = strip_prefix_from_bytes(root, &a.path);
                }
                ChangeNotification::SmallChange(SmallChangeNotification::Modified(m)) => {
                    m.path = strip_prefix_from_bytes(root, &m.path);
                }
                ChangeNotification::SmallChange(SmallChangeNotification::Removed(r)) => {
                    r.path = strip_prefix_from_bytes(root, &r.path);
                }
                ChangeNotification::SmallChange(SmallChangeNotification::Renamed(r)) => {
                    r.from = strip_prefix_from_bytes(root, &r.from);
                    r.to = strip_prefix_from_bytes(root, &r.to);
                }
                ChangeNotification::SmallChange(SmallChangeNotification::Replaced(r)) => {
                    r.from = strip_prefix_from_bytes(root, &r.from);
                    r.to = strip_prefix_from_bytes(root, &r.to);
                }
                _ => {}
            });
        }
        Ok(result)
    }

    pub async fn subscribe(
        &self,
        mount_point: &Option<PathBuf>,
        throttle_time_ms: u64,
        position: Option<JournalPosition>,
        root: &Option<PathBuf>,
        included_roots: &Option<Vec<PathBuf>>,
        included_suffixes: &Option<Vec<String>>,
        excluded_roots: &Option<Vec<PathBuf>>,
        excluded_suffixes: &Option<Vec<String>>,
        include_vcs_roots: bool,
        handle_results: impl Fn(&ChangesSinceV2Result) -> Result<(), EdenFsError>,
    ) -> Result<(), anyhow::Error> {
        let mut position = position.unwrap_or(self.get_journal_position(mount_point).await?);
        let mut subscription = self.stream_journal_changed(mount_point).await?;

        let mut last = Instant::now();
        let throttle = Duration::from_millis(throttle_time_ms);

        let mut pending_updates = false;

        // Largest allowed sleep value  https://docs.rs/tokio/latest/tokio/time/fn.sleep.html
        let sleep_max = Duration::from_millis(68719476734);
        let timer = time::sleep(sleep_max);
        tokio::pin!(timer);

        loop {
            tokio::select! {
                // Wait on the following cases
                // 1. The we get a notification from the subscription
                // 2. The pending updates timer expires
                // 3. Another signal is received
                result = subscription.next() => {
                    match result {
                        // if the stream is ended somehow, we terminate as well
                        None => break,
                        // if any error happened during the stream, log them
                        Some(Err(e)) => {
                            tracing::error!(?e, "error while processing subscription");
                            continue;
                        },
                        // If we have recently(within throttle ms) sent an update, set a
                        // timer to check again when throttle time is up if we aren't already
                        // waiting on a timer
                        Some(Ok(_)) => {
                            if last.elapsed() < throttle && !pending_updates {
                                // set timer to check again when throttle time is up
                                pending_updates = true;
                                timer.as_mut().reset((Instant::now() + throttle).into());
                                continue;
                            }
                        }
                    }
                },
                // Pending updates timer expired. If we haven't gotten a subscription notification in
                // the meantime, check for updates now. Set the timer back to the max value in either case.
                () = &mut timer => {
                    // Set timer to the maximum value to prevent repeated wakeups since timers are not consumed
                    timer.as_mut().reset((Instant::now() + sleep_max).into());
                    if !pending_updates {
                        continue;
                    }
                },
                // in all other cases, we terminate
                else => break,
            }

            let result = self
                .get_changes_since(
                    mount_point,
                    &position,
                    root,
                    included_roots,
                    included_suffixes,
                    excluded_roots,
                    excluded_suffixes,
                    include_vcs_roots,
                )
                .await
                .map_err(anyhow::Error::msg)?;

            tracing::debug!(
                "got {} changes for position {}",
                result.changes.len(),
                result.to_position
            );

            if !result.changes.is_empty() {
                // Error in handle results will terminate the loop
                handle_results(&result)?;
            }

            pending_updates = false;
            position = result.to_position;

            last = Instant::now();
        }

        Ok(())
    }
}

/// Given a prefix and a list of paths, return a list of paths with the prefix prepended to each path.
///
/// If the prefix is None the paths are processed as-is.
/// All paths are post-processed with the provided function.
pub(crate) fn prefix_paths<F, T>(
    prefix: &Option<PathBuf>,
    paths: &Option<Vec<PathBuf>>,
    f: F,
) -> Option<Vec<T>>
where
    F: Fn(PathBuf) -> T,
{
    if let Some(prefix) = prefix {
        paths
            .as_ref()
            .map(|ps| ps.iter().map(|p| f(prefix.join(p))).collect::<Vec<_>>())
    } else {
        paths
            .as_ref()
            .map(|ps| ps.iter().map(|p| f(p.to_path_buf())).collect::<Vec<_>>())
    }
}

pub(crate) fn strip_prefix_from_bytes(prefix: &Option<PathBuf>, path: &[u8]) -> Vec<u8> {
    if let Some(prefix) = prefix {
        let path = Path::new(std::str::from_utf8(path).expect("Failed to convert path to string"));
        path.strip_prefix(prefix)
            .map_or(path, |stripped_path| stripped_path)
            .to_string_lossy()
            .to_string()
            .into_bytes()
    } else {
        path.to_vec()
    }
}
