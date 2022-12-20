/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use io::IO;
use manifest_tree::ReadTreeManifest;
use parking_lot::Mutex;
use pathmatcher::Matcher;
use progress_model::ProgressBar;
use repolock::RepoLocker;
use treestate::treestate::TreeState;
use vfs::VFS;
use watchman_client::prelude::*;

use super::state::StatusQuery;
use super::state::WatchmanState;
use super::treestate::WatchmanTreeState;
use crate::filechangedetector::ArcReadFileContents;
use crate::filechangedetector::FileChangeDetector;
use crate::filesystem::PendingChangeResult;
use crate::filesystem::PendingChanges;
use crate::workingcopy::WorkingCopy;

type ArcReadTreeManifest = Arc<dyn ReadTreeManifest + Send + Sync>;

pub struct WatchmanFileSystem {
    vfs: VFS,
    treestate: Arc<Mutex<TreeState>>,
    tree_resolver: ArcReadTreeManifest,
    store: ArcReadFileContents,
    locker: Arc<RepoLocker>,
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
    async fn query_result(&self, state: &WatchmanState) -> Result<QueryResult<StatusQuery>> {
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
                    since: state.get_clock(),
                    expression: Some(Expr::Not(Box::new(excludes))),
                    sync_timeout: state.sync_timeout(),
                    ..Default::default()
                },
            )
            .await?;

        tracing::trace!(target: "measuredtimes", watchmanquery_time=start.elapsed().as_millis());

        Ok(result)
    }
}

impl PendingChanges for WatchmanFileSystem {
    fn pending_changes(
        &self,
        _matcher: Arc<dyn Matcher + Send + Sync + 'static>,
        last_write: SystemTime,
        config: &dyn Config,
        io: &IO,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChangeResult>>>> {
        let state = WatchmanState::new(
            config,
            WatchmanTreeState {
                treestate: self.treestate.clone(),
                root: self.vfs.root(),
            },
        )?;

        let result = async_runtime::block_on(self.query_result(&state))?;

        tracing::debug!(
            target: "watchman_info",
            watchmanfreshinstances= if result.is_fresh_instance { 1 } else { 0 },
            watchmanfilecount=result.files.as_ref().map_or(0, |f| f.len()),
        );

        let should_warn = config.get_or_default("fsmonitor", "warn-fresh-instance")?;
        if result.is_fresh_instance && should_warn {
            let old_pid = parse_watchman_pid(state.get_clock().as_ref());
            let new_pid = parse_watchman_pid(Some(&result.clock));
            let mut output = io.output();
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
        }

        let file_change_threshold =
            config.get_or("fsmonitor", "watchman-changed-file-threshold", || 200)?;
        let should_update_clock = result.is_fresh_instance
            || result
                .files
                .as_ref()
                .map_or(false, |f| f.len() > file_change_threshold);

        let manifests =
            WorkingCopy::current_manifests(&self.treestate.lock(), &self.tree_resolver)?;

        let file_change_detector = FileChangeDetector::new(
            self.treestate.clone(),
            self.vfs.clone(),
            last_write.try_into()?,
            manifests[0].clone(),
            self.store.clone(),
        );
        let mut pending_changes = state.merge(result, file_change_detector)?;

        pending_changes.persist(
            WatchmanTreeState {
                treestate: self.treestate.clone(),
                root: self.vfs.root(),
            },
            should_update_clock,
            &self.locker,
        )?;

        Ok(Box::new(pending_changes.into_iter()))
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
