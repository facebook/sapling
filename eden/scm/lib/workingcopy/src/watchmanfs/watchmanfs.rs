/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use io::IO;
use manifest_tree::ReadTreeManifest;
use parking_lot::Mutex;
use pathmatcher::Matcher;
use progress_model::ProgressBar;
use repolock::RepoLocker;
use serde::Deserialize;
use treestate::treestate::TreeState;
use types::path::ParseError;
use types::RepoPathBuf;
use vfs::VFS;
use watchman_client::prelude::*;

use super::treestate::clear_needs_check;
use super::treestate::mark_needs_check;
use super::treestate::set_clock;
use crate::filechangedetector::ArcReadFileContents;
use crate::filechangedetector::FileChangeDetector;
use crate::filechangedetector::FileChangeDetectorTrait;
use crate::filechangedetector::FileChangeResult;
use crate::filechangedetector::ResolvedFileChangeResult;
use crate::filesystem::PendingChangeResult;
use crate::filesystem::PendingChanges;
use crate::watchmanfs::treestate::get_clock;
use crate::watchmanfs::treestate::list_needs_check;
use crate::watchmanfs::treestate::maybe_flush_treestate;
use crate::workingcopy::WorkingCopy;

type ArcReadTreeManifest = Arc<dyn ReadTreeManifest + Send + Sync>;

pub struct WatchmanFileSystem {
    vfs: VFS,
    treestate: Arc<Mutex<TreeState>>,
    tree_resolver: ArcReadTreeManifest,
    store: ArcReadFileContents,
    locker: Arc<RepoLocker>,
}

struct WatchmanConfig {
    clock: Option<Clock>,
    sync_timeout: std::time::Duration,
}

query_result_type! {
    pub struct StatusQuery {
        name: BytesNameField,
        exists: ExistsField,
    }
}

impl WatchmanFileSystem {
    pub fn new(
        vfs: VFS,
        treestate: Arc<Mutex<TreeState>>,
        tree_resolver: ArcReadTreeManifest,
        store: ArcReadFileContents,
        locker: Arc<RepoLocker>,
    ) -> Result<Self> {
        Ok(WatchmanFileSystem {
            vfs,
            treestate,
            tree_resolver,
            store,
            locker,
        })
    }

    #[tracing::instrument(skip_all, err)]
    async fn query_result(&self, config: WatchmanConfig) -> Result<QueryResult<StatusQuery>> {
        let start = std::time::Instant::now();

        let _bar = ProgressBar::register_new("querying watchman", 0, "");

        let client = Connector::new().connect().await?;
        let resolved = client
            .resolve_root(CanonicalPath::canonicalize(self.vfs.root())?)
            .await?;

        let ident = identity::must_sniff_dir(self.vfs.root())?;
        let excludes = Expr::Any(vec![Expr::DirName(DirNameTerm {
            path: PathBuf::from(ident.dot_dir()),
            depth: None,
        })]);

        let result = client
            .query::<StatusQuery>(
                &resolved,
                QueryRequestCommon {
                    since: config.clock,
                    expression: Some(Expr::Not(Box::new(excludes))),
                    sync_timeout: config.sync_timeout.into(),
                    ..Default::default()
                },
            )
            .await?;

        tracing::trace!(target: "measuredtimes", watchmanquery_time=start.elapsed().as_millis());

        Ok(result)
    }
}

impl PendingChanges for WatchmanFileSystem {
    #[tracing::instrument(skip_all)]
    fn pending_changes(
        &self,
        matcher: Arc<dyn Matcher + Send + Sync + 'static>,
        last_write: SystemTime,
        config: &dyn Config,
        io: &IO,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChangeResult>>>> {
        let ts = &mut *self.treestate.lock();

        let prev_clock = get_clock(ts)?;

        let result = async_runtime::block_on(self.query_result(WatchmanConfig {
            clock: prev_clock.clone(),
            sync_timeout:
                config.get_or::<Duration>("fsmonitor", "timeout", || Duration::from_secs(10))?,
        }))?;

        tracing::debug!(
            target: "watchman_info",
            watchmanfreshinstances= if result.is_fresh_instance { 1 } else { 0 },
            watchmanfilecount=result.files.as_ref().map_or(0, |f| f.len()),
        );

        let should_warn = config.get_or_default("fsmonitor", "warn-fresh-instance")?;
        if result.is_fresh_instance && should_warn {
            let _ = warn_about_fresh_instance(
                io,
                parse_watchman_pid(prev_clock.as_ref()),
                parse_watchman_pid(Some(&result.clock)),
            );
        }

        let file_change_threshold =
            config.get_or("fsmonitor", "watchman-changed-file-threshold", || 200)?;
        let should_update_clock = result.is_fresh_instance
            || result
                .files
                .as_ref()
                .map_or(false, |f| f.len() > file_change_threshold);

        let manifests = WorkingCopy::current_manifests(ts, &self.tree_resolver)?;

        let file_change_detector = FileChangeDetector::new(
            self.vfs.clone(),
            last_write.try_into()?,
            manifests[0].clone(),
            self.store.clone(),
        );

        let mut wm_errors: Vec<ParseError> = Vec::new();
        let wm_needs_check: Vec<RepoPathBuf> = result
            .files
            .unwrap_or_default()
            .into_iter()
            .filter_map(|query| {
                match RepoPathBuf::from_utf8(query.name.into_inner().into_bytes()) {
                    Ok(path) => Some(path),
                    Err(err) => {
                        wm_errors.push(err);
                        None
                    }
                }
            })
            .collect();

        let mut pending_changes =
            detect_changes(matcher, file_change_detector, ts, wm_needs_check)?;

        // Add back path errors into the pending changes. The caller
        // of pending_changes must choose how to handle these.
        pending_changes
            .pending_changes
            .extend(wm_errors.into_iter().map(|e| Err(anyhow!(e))));

        let did_something = pending_changes.update_treestate(ts)?;
        if did_something || should_update_clock {
            // If we had something to update in the treestate, make sure clock is updated as well.
            set_clock(ts, result.clock)?;
        }

        maybe_flush_treestate(self.vfs.root(), ts, &self.locker)?;

        Ok(Box::new(pending_changes.into_iter()))
    }
}

fn warn_about_fresh_instance(io: &IO, old_pid: Option<u32>, new_pid: Option<u32>) -> Result<()> {
    let mut output = io.error();
    match (old_pid, new_pid) {
        (Some(old_pid), Some(new_pid)) if old_pid != new_pid => {
            writeln!(
                &mut output,
                "warning: watchman has recently restarted (old pid {}, new pid {}) - operation will be slower than usual",
                old_pid, new_pid
            )?;
        }
        (None, Some(new_pid)) => {
            writeln!(
                &mut output,
                "warning: watchman has recently started (pid {}) - operation will be slower than usual",
                new_pid
            )?;
        }
        _ => {
            writeln!(
                &mut output,
                "warning: watchman failed to catch up with file change events and requires a full scan - operation will be slower than usual"
            )?;
        }
    }

    Ok(())
}

// Given the existing treestate and files watchman says to check,
// figure out all the files that may have changed and check them for
// changes. Also track paths we need to mark or unmark as NEED_CHECK
// in the treestate.
pub fn detect_changes(
    matcher: Arc<dyn Matcher + Send + Sync + 'static>,
    mut file_change_detector: impl FileChangeDetectorTrait + 'static,
    ts: &mut TreeState,
    wm_need_check: Vec<RepoPathBuf>,
) -> Result<WatchmanPendingChanges> {
    let (ts_need_check, ts_errors) = list_needs_check(ts, matcher)?;

    let ts_need_check: HashSet<_> = ts_need_check.into_iter().collect();

    let mut pending_changes: Vec<Result<PendingChangeResult>> =
        ts_errors.into_iter().map(|e| Err(anyhow!(e))).collect();
    let mut needs_clear = Vec::new();
    let mut needs_mark = Vec::new();

    tracing::debug!(
        watchman_needs_check = wm_need_check.len(),
        treestate_needs_check = ts_need_check.len(),
    );

    for needs_check in ts_need_check
        .iter()
        .chain(wm_need_check.iter().filter(|p| !ts_need_check.contains(*p)))
    {
        match file_change_detector.has_changed(ts, needs_check) {
            Ok(FileChangeResult::Yes(change)) => {
                pending_changes.push(Ok(PendingChangeResult::File(change)));
                if !ts_need_check.contains(needs_check) {
                    needs_mark.push(needs_check.clone());
                }
            }
            Ok(FileChangeResult::No) => {
                if ts_need_check.contains(needs_check) {
                    needs_clear.push(needs_check.clone());
                }
            }
            // Handled in below in next loop.
            Ok(FileChangeResult::Maybe) => {}
            Err(e) => pending_changes.push(Err(e)),
        }
    }

    for result in file_change_detector.resolve_maybes() {
        match result {
            Ok(ResolvedFileChangeResult::Yes(change)) => {
                let path = change.get_path();
                if !ts_need_check.contains(path) {
                    needs_mark.push(path.clone());
                }
                pending_changes.push(Ok(PendingChangeResult::File(change)));
            }
            Ok(ResolvedFileChangeResult::No(path)) => {
                if ts_need_check.contains(&path) {
                    needs_clear.push(path);
                }
            }
            Err(e) => pending_changes.push(Err(e)),
        }
    }

    Ok(WatchmanPendingChanges {
        pending_changes,
        needs_clear,
        needs_mark,
    })
}

pub struct WatchmanPendingChanges {
    pending_changes: Vec<Result<PendingChangeResult>>,
    needs_clear: Vec<RepoPathBuf>,
    needs_mark: Vec<RepoPathBuf>,
}

impl WatchmanPendingChanges {
    #[tracing::instrument(skip_all)]
    pub fn update_treestate(&mut self, ts: &mut TreeState) -> Result<bool> {
        let mut wrote = false;
        for path in self.needs_clear.iter() {
            match clear_needs_check(ts, path) {
                Ok(v) => wrote |= v,
                Err(e) =>
                // We can still build a valid result if we fail to clear the
                // needs check flag. Propagate the error to the caller but allow
                // the persist to continue.
                {
                    self.pending_changes.push(Err(e))
                }
            }
        }

        for path in self.needs_mark.iter() {
            wrote |= mark_needs_check(ts, path)?;
        }

        Ok(wrote)
    }
}

impl IntoIterator for WatchmanPendingChanges {
    type Item = Result<PendingChangeResult>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.pending_changes.into_iter()
    }
}

fn parse_watchman_pid(clock: Option<&Clock>) -> Option<u32> {
    match clock {
        Some(Clock::Spec(ClockSpec::StringClock(clock_str))) => match clock_str.split(':').nth(2) {
            None => None,
            Some(pid) => pid.parse().ok(),
        },
        _ => None,
    }
}
