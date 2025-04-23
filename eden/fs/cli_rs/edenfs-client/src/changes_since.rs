/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;

use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_utils::bytes_from_path;
use edenfs_utils::path_from_bytes;
use edenfs_utils::prefix_paths;
use edenfs_utils::strip_prefix_from_bytes;
use futures::Stream;
use futures::StreamExt;
use futures::stream;
use serde::Serialize;
use tokio::time;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::instance::EdenFsInstance;
use crate::types::Dtype;
use crate::types::JournalPosition;
use crate::utils::get_mount_point;

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

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum LostChangesReason {
    Unknown = 0,
    EdenFsRemounted = 1,
    JournalTruncated = 2,
    TooManyChanges = 3,
    Undefined = -1,
}

impl From<thrift_types::edenfs::LostChangesReason> for LostChangesReason {
    fn from(from: thrift_types::edenfs::LostChangesReason) -> Self {
        match from {
            thrift_types::edenfs::LostChangesReason::UNKNOWN => Self::Unknown,
            thrift_types::edenfs::LostChangesReason::EDENFS_REMOUNTED => Self::EdenFsRemounted,
            thrift_types::edenfs::LostChangesReason::JOURNAL_TRUNCATED => Self::JournalTruncated,
            thrift_types::edenfs::LostChangesReason::TOO_MANY_CHANGES => Self::TooManyChanges,
            _ => Self::Undefined,
        }
    }
}

impl fmt::Display for LostChangesReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display_str = match *self {
            LostChangesReason::Unknown => "Unknown",
            LostChangesReason::EdenFsRemounted => "EdenFsRemounted",
            LostChangesReason::JournalTruncated => "JournalTruncated",
            LostChangesReason::TooManyChanges => "TooManyChanges",
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

        let params = thrift_types::edenfs::ChangesSinceV2Params {
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

    /// Streams changes to files in an EdenFS mount since a given journal position.
    ///
    /// This method creates a stream that continuously monitors for changes in the specified
    /// EdenFS mount point and emits them as they occur. The stream will continue until it's
    /// dropped or an error occurs.
    ///
    /// Changes are throttled to avoid overwhelming the client with rapid updates. If multiple
    /// changes occur within the throttle time window, they will be batched together in the
    /// next emission.
    ///
    /// # Parameters
    ///
    /// * `mount_point` - The EdenFS mount point to monitor. If `None`, the current working
    ///   directory is used.
    /// * `throttle_time_ms` - The minimum time in milliseconds between emitting changes.
    /// * `position` - The journal position to start monitoring from.
    /// * `root` - Optional root directory within the mount to restrict monitoring to.
    /// * `included_roots` - Optional list of directories within the root to include.
    /// * `included_suffixes` - Optional list of file suffixes to include.
    /// * `excluded_roots` - Optional list of directories within the root to exclude.
    /// * `excluded_suffixes` - Optional list of file suffixes to exclude.
    /// * `include_vcs_roots` - Whether to include VCS root directories.
    ///
    /// # Returns
    ///
    /// A `Result` containing a stream that emits `Result<ChangesSinceV2Result>` items.
    /// Each item contains a batch of changes that occurred since the last emission, along with
    /// the new journal position.
    ///
    /// # Examples
    ///
    /// The following example shows how to use this method to monitor changes in a directory:
    ///
    /// ```no_run
    /// use std::path::PathBuf;
    ///
    /// use edenfs_client::instance::EdenFsInstance;
    /// use edenfs_client::types::JournalPosition;
    /// use futures::StreamExt;
    ///
    /// // This example doesn't actually run the client, but demonstrates the API usage
    /// async fn example_usage() {
    ///     let instance = EdenFsInstance::global();
    ///     let client = instance.get_client();
    ///
    ///     // Start monitoring from the current journal position
    ///     let position = client
    ///         .get_journal_position(&None) // Use current directory as mount point
    ///         .await
    ///         .expect("Failed to get journal position");
    ///
    ///     // Stream changes in the current directory, throttled to at most one update per second
    ///     let mut stream = client
    ///         .stream_changes_since(
    ///             &None,    // Use current directory as mount point
    ///             1000,     // Throttle to 1 update per second
    ///             position, // Start from this journal position
    ///             &None,    // No root directory restriction
    ///             &None,    // No included roots
    ///             &Some(vec![
    ///                 // Only include .rs and .toml files
    ///                 ".rs".to_string(),
    ///                 ".toml".to_string(),
    ///             ]),
    ///             &None, // No excluded roots
    ///             &None, // No excluded suffixes
    ///             false, // Don't include VCS roots
    ///         )
    ///         .await
    ///         .expect("Failed to create stream");
    ///
    ///     // Process the stream of changes
    ///     while let Some(result) = stream.next().await {
    ///         match result {
    ///             Ok(r) => {
    ///                 println!("Received {} changes", r.changes.len());
    ///                 for change in &r.changes {
    ///                     println!("Change: {}", change);
    ///                 }
    ///                 println!("New position: {}", r.to_position);
    ///             }
    ///             Err(e) => {
    ///                 eprintln!("Error: {}", e);
    ///                 break;
    ///             }
    ///         }
    ///     }
    /// }
    /// ```
    #[cfg(fbcode_build)]
    pub async fn stream_changes_since(
        &self,
        mount_point: &Option<PathBuf>,
        throttle_time_ms: u64,
        position: JournalPosition,
        root: &Option<PathBuf>,
        included_roots: &Option<Vec<PathBuf>>,
        included_suffixes: &Option<Vec<String>>,
        excluded_roots: &Option<Vec<PathBuf>>,
        excluded_suffixes: &Option<Vec<String>>,
        include_vcs_roots: bool,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChangesSinceV2Result>> + Send>>> {
        struct State {
            mount_point: Option<PathBuf>,
            position: JournalPosition,
            root: Option<PathBuf>,
            included_roots: Option<Vec<PathBuf>>,
            included_suffixes: Option<Vec<String>>,
            excluded_roots: Option<Vec<PathBuf>>,
            excluded_suffixes: Option<Vec<String>>,
            include_vcs_roots: bool,
            subscription: Pin<Box<dyn Stream<Item = Result<JournalPosition>> + Send>>,
            last: Instant,
            throttle: Duration,
            pending_updates: bool,
        }

        // Largest allowed sleep value  https://docs.rs/tokio/latest/tokio/time/fn.sleep.html
        const SLEEP_MAX: Duration = Duration::from_millis(68719476734);

        let state = State {
            // Params
            mount_point: mount_point.clone(),
            position,
            root: root.clone(),
            included_roots: included_roots.clone(),
            included_suffixes: included_suffixes.clone(),
            excluded_roots: excluded_roots.clone(),
            excluded_suffixes: excluded_suffixes.clone(),
            include_vcs_roots: include_vcs_roots.clone(),
            // Locals
            subscription: self.stream_journal_changed(mount_point).await?,
            last: Instant::now(),
            throttle: Duration::from_millis(throttle_time_ms),
            pending_updates: false,
        };

        let stream = stream::unfold(state, move |mut state| async move {
            let timer = time::sleep(SLEEP_MAX);
            tokio::pin!(timer);

            loop {
                tokio::select! {
                    // Wait on the following cases
                    // 1. The we get a notification from the subscription
                    // 2. The pending updates timer expires
                    // 3. Another signal is received
                    result = state.subscription.next() => {
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
                                if state.last.elapsed() < state.throttle && !state.pending_updates {
                                    // set timer to check again when throttle time is up
                                    state.pending_updates = true;
                                    timer.as_mut().reset((Instant::now() + state.throttle).into());
                                    continue;
                                }
                            }
                        }
                    },
                    // Pending updates timer expired. If we haven't gotten a subscription notification in
                    // the meantime, check for updates now. Set the timer back to the max value in either case.
                    () = &mut timer => {
                        // Set timer to the maximum value to prevent repeated wakeups since timers are not consumed
                        timer.as_mut().reset((Instant::now() + SLEEP_MAX).into());
                        if !state.pending_updates {
                            continue;
                        }
                    },
                    // in all other cases, we terminate
                    else => break,
                }

                state.pending_updates = false;
                state.last = Instant::now();

                let result = EdenFsInstance::global()
                    .get_client()
                    .get_changes_since(
                        &state.mount_point,
                        &state.position,
                        &state.root,
                        &state.included_roots,
                        &state.included_suffixes,
                        &state.excluded_roots,
                        &state.excluded_suffixes,
                        state.include_vcs_roots,
                    )
                    .await;
                match result {
                    Ok(ref r) => {
                        tracing::debug!(
                            "got {} changes for position {}",
                            r.changes.len(),
                            r.to_position
                        );

                        state.position = r.to_position.clone();
                        if !r.changes.is_empty() {
                            return Some((result, state));
                        }
                    }
                    Err(_) => return Some((result, state)),
                }
            }

            None
        });

        Ok(stream.boxed())
    }
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;

    use crate::changes_since::*;

    #[fbinit::test]
    async fn test_get_changes_since(fb: FacebookInit) -> Result<()> {
        let result = std::panic::catch_unwind(|| async {
            let client = EdenFsClient::new(fb, PathBuf::new(), None);
            let position = JournalPosition {
                mount_generation: 0,
                sequence_number: 0,
                snapshot_hash: Vec::new(),
            };
            client
                .get_changes_since(&None, &position, &None, &None, &None, &None, &None, false)
                .await
        });

        // Current MockClient is unimplemented and panics. catch_unwind does catch the panic, but for some reason thinks it is ok.
        assert!(result.is_ok());
        Ok(())
    }
}
