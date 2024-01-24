/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::AlwaysMatcher;
use pathmatcher::DynMatcher;
use pathmatcher::Matcher;
use progress_model::ProgressBar;
use repolock::RepoLocker;
use serde::Deserialize;
use serde::Serialize;
use termlogger::TermLogger;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use types::path::ParseError;
use types::RepoPath;
use types::RepoPathBuf;
use vfs::VFS;
use watchman_client::prelude::*;

use super::treestate::clear_needs_check;
use super::treestate::mark_needs_check;
use super::treestate::set_clock;
use crate::filechangedetector::ArcFileStore;
use crate::filechangedetector::FileChangeDetector;
use crate::filechangedetector::FileChangeDetectorTrait;
use crate::filechangedetector::ResolvedFileChangeResult;
use crate::filesystem::FileSystem;
use crate::filesystem::PendingChange;
use crate::metadata;
use crate::metadata::Metadata;
use crate::physicalfs::PhysicalFileSystem;
use crate::util::dirstate_write_time_override;
use crate::util::maybe_flush_treestate;
use crate::util::walk_treestate;
use crate::watchmanfs::treestate::get_clock;
use crate::watchmanfs::treestate::list_needs_check;
use crate::workingcopy::WorkingCopy;

type ArcReadTreeManifest = Arc<dyn ReadTreeManifest + Send + Sync>;

pub struct WatchmanFileSystem {
    inner: PhysicalFileSystem,
}

struct WatchmanConfig {
    clock: Option<Clock>,
    sync_timeout: std::time::Duration,
}

query_result_type! {
    pub struct StatusQuery {
        name: BytesNameField,
        mode: ModeAndPermissionsField,
        size: SizeField,
        mtime: MTimeField,
        exists: ExistsField,
    }
}

#[derive(Deserialize, Debug)]
struct DebugRootStatusResponse {
    pub root_status: Option<RootStatus>,
}

#[derive(Deserialize, Debug)]
struct RootStatus {
    pub recrawl_info: Option<RecrawlInfo>,
}

#[derive(Deserialize, Debug)]
pub struct RecrawlInfo {
    pub stats: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DebugRootStatusRequest(pub &'static str, pub PathBuf);

impl WatchmanFileSystem {
    pub fn new(
        vfs: VFS,
        tree_resolver: ArcReadTreeManifest,
        store: ArcFileStore,
        treestate: Arc<Mutex<TreeState>>,
        locker: Arc<RepoLocker>,
    ) -> Result<Self> {
        Ok(WatchmanFileSystem {
            inner: PhysicalFileSystem::new(vfs, tree_resolver, store, treestate, locker)?,
        })
    }

    async fn query_files(
        &self,
        config: WatchmanConfig,
        ignore_dirs: Vec<PathBuf>,
    ) -> Result<QueryResult<StatusQuery>> {
        let start = std::time::Instant::now();

        // This starts watchman if it isn't already started.
        let client = Connector::new().connect().await?;

        // This blocks until the recrawl (if required) is done. Progress is
        // shown by the crawl_progress task.
        let resolved = client
            .resolve_root(CanonicalPath::canonicalize(self.inner.vfs.root())?)
            .await?;

        let mut not_exprs = vec![
            // This files under nested ".hg" directories. Note that we don't have a good
            // way to ignore regular files in the nested repo (e.g. we can ignore
            // "dir/.hg/file", but not "dir/file".
            Expr::Match(MatchTerm {
                glob: format!("**/{}/**", self.inner.dot_dir),
                wholename: true,
                include_dot_files: true,
                ..Default::default()
            }),
        ];

        not_exprs.extend(ignore_dirs.into_iter().map(|p| {
            Expr::DirName(DirNameTerm {
                path: p,
                depth: None,
            })
        }));

        // The crawl is done - display a generic "we're querying" spinner.
        let _bar = ProgressBar::new_adhoc("querying watchman", 0, "");

        let result = client
            .query::<StatusQuery>(
                &resolved,
                QueryRequestCommon {
                    since: config.clock,
                    expression: Some(Expr::Not(Box::new(Expr::Any(not_exprs)))),
                    sync_timeout: config.sync_timeout.into(),
                    ..Default::default()
                },
            )
            .await?;

        tracing::trace!(target: "measuredtimes", watchmanquery_time=start.elapsed().as_millis());

        Ok(result)
    }

    #[tracing::instrument(skip_all)]
    fn pending_changes(
        &self,
        matcher: DynMatcher,
        ignore_matcher: DynMatcher,
        ignore_dirs: Vec<PathBuf>,
        include_ignored: bool,
        config: &dyn Config,
        lgr: &TermLogger,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChange>>>> {
        let ts = &mut *self.inner.treestate.lock();

        let treestate_started_dirty = ts.dirty();

        let ts_metadata = ts.metadata()?;
        let mut prev_clock = get_clock(&ts_metadata)?;

        let track_ignored = config.get_or_default::<bool>("fsmonitor", "track-ignore-files")?;
        let ts_track_ignored = ts_metadata.get("track-ignored").map(|v| v.as_ref()) == Some("1");
        if track_ignored != ts_track_ignored {
            // If track-ignore-files has changed, trigger a migration by
            // unsetting the clock. Watchman will do a full crawl and report
            // fresh instance.
            prev_clock = None;

            // Store new value of track ignored so we don't migrate again.
            let md_value = if track_ignored {
                "1".to_string()
            } else {
                "0".to_string()
            };
            tracing::info!(track_ignored = md_value, "migrating track-ignored");
            ts.update_metadata(&[("track-ignored".to_string(), Some(md_value))])?;
        }

        if include_ignored && !track_ignored {
            // TODO: give user a hint about fsmonitor.track-ignore-files
            prev_clock = None;
        }

        if config.get_or_default("devel", "watchman-reset-clock")? {
            prev_clock = None;
        }

        let progress_handle = async_runtime::spawn(crawl_progress(
            self.inner.vfs.root().to_path_buf(),
            ts.len() as u64,
        ));

        let result = {
            // Instrument query_files() from outside to avoid async weirdness.
            let _span = tracing::info_span!("query_files").entered();

            async_runtime::block_on(self.query_files(
                WatchmanConfig {
                    clock: prev_clock.clone(),
                    sync_timeout:
                        config.get_or::<Duration>("fsmonitor", "timeout", || {
                            Duration::from_secs(10)
                        })?,
                },
                ignore_dirs,
            ))
        };

        // Make sure we always abort - even in case of error.
        progress_handle.abort();

        let result = result?;

        tracing::debug!(
            target: "watchman_info",
            watchmanfreshinstances= if result.is_fresh_instance { 1 } else { 0 },
            watchmanfilecount=result.files.as_ref().map_or(0, |f| f.len()),
        );

        let should_warn = config.get_or_default("fsmonitor", "warn-fresh-instance")?;
        if result.is_fresh_instance && should_warn {
            let _ = warn_about_fresh_instance(
                lgr,
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

        let manifests = WorkingCopy::current_manifests(ts, &self.inner.tree_resolver)?;

        let mut wm_errors: Vec<ParseError> = Vec::new();
        let use_watchman_metadata =
            config.get_or::<bool>("workingcopy", "use-watchman-metadata", || true)?;
        let wm_needs_check: Vec<metadata::File> = result
            .files
            .unwrap_or_default()
            .into_iter()
            .filter_map(
                |file| match RepoPathBuf::from_utf8(file.name.into_inner().into_bytes()) {
                    Ok(path) => {
                        tracing::trace!(
                            ?path,
                            mode = *file.mode,
                            size = *file.size,
                            mtime = *file.mtime,
                            exists = *file.exists,
                            "watchman file"
                        );

                        let meta = Metadata::from_stat(
                            file.mode.into_inner() as u32,
                            file.size.into_inner(),
                            file.mtime.into_inner(),
                        );

                        let fs_meta = if *file.exists {
                            if use_watchman_metadata {
                                Some(Some(meta))
                            } else {
                                None
                            }
                        } else {
                            // If watchman says the file doesn't exist, indicate
                            // that via the metadata being None. This is
                            // important when a file moves behind a symlink;
                            // Watchman will report it as deleted, but a naive
                            // lstat() call would show the file to still exist.
                            Some(None)
                        };

                        Some(metadata::File {
                            path,
                            fs_meta,
                            ts_state: None,
                        })
                    }
                    Err(err) => {
                        wm_errors.push(err);
                        None
                    }
                },
            )
            .collect();

        let detector = FileChangeDetector::new(
            self.inner.vfs.clone(),
            manifests[0].clone(),
            self.inner.store.clone(),
            config.get_opt("workingcopy", "worker-count")?,
        );
        let mut pending_changes = detect_changes(
            matcher,
            ignore_matcher,
            track_ignored,
            include_ignored,
            detector,
            ts,
            wm_needs_check,
            result.is_fresh_instance,
            self.inner.vfs.case_sensitive(),
        )?;

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

        // Don't flush treestate if it was already dirty. If we are inside a
        // Python transaction with uncommitted, substantial dirstate changes,
        // those changes should not be written out until the transaction
        // finishes.
        if treestate_started_dirty {
            tracing::debug!("treestate was dirty - skipping flush");
        } else {
            maybe_flush_treestate(
                self.inner.vfs.root(),
                ts,
                &self.inner.locker,
                dirstate_write_time_override(config),
            )?;
        }

        Ok(Box::new(pending_changes.into_iter()))
    }
}

async fn crawl_progress(root: PathBuf, approx_file_count: u64) -> Result<()> {
    let client = {
        let _bar = ProgressBar::new_detached("connecting watchman", 0, "");

        // If watchman just started (and we issued "watch-project" from
        // query_files), this connect gets stuck indefinitely. Work around by
        // timing out and retrying until we get through.
        loop {
            match tokio::time::timeout(Duration::from_secs(1), Connector::new().connect()).await {
                Ok(client) => break client?,
                Err(_) => {}
            };

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    let mut bar = None;

    let req = DebugRootStatusRequest(
        "debug-root-status",
        CanonicalPath::canonicalize(root)?.into_path_buf(),
    );

    loop {
        let response: DebugRootStatusResponse = client.generic_request(req.clone()).await?;

        if let Some(RootStatus {
            recrawl_info: Some(RecrawlInfo { stats: Some(stats) }),
        }) = response.root_status
        {
            bar.get_or_insert_with(|| {
                ProgressBar::new_detached("crawling", approx_file_count, "files (approx)")
            })
            .set_position(stats);
        } else if bar.is_some() {
            return Ok(());
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

impl FileSystem for WatchmanFileSystem {
    fn pending_changes(
        &self,
        matcher: DynMatcher,
        ignore_matcher: DynMatcher,
        ignore_dirs: Vec<PathBuf>,
        include_ignored: bool,
        config: &dyn Config,
        lgr: &TermLogger,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChange>>>> {
        let result = self.pending_changes(
            matcher.clone(),
            ignore_matcher.clone(),
            ignore_dirs.clone(),
            include_ignored,
            config,
            lgr,
        );

        match result {
            Ok(result) => Ok(result),
            Err(err) if err.is::<watchman_client::Error>() => {
                if !config.get_or("fsmonitor", "fallback-on-watchman-exception", || true)? {
                    return Err(err);
                }

                // On watchman error, fall back to manual walk. This is important for errors such as:
                //   - "watchman" binary not in PATH
                //   - unsupported filesystem (e.g. NFS)
                //
                // A better approach might be an allowlist of errors to fall
                // back on so we can fail hard in cases where watchman "should"
                // work, but that is probably still an unacceptable UX in general.

                tracing::debug!(target: "watchman_info", watchmanfallback=1);
                tracing::warn!(?err, "watchman error - falling back to slow crawl");
                self.inner.pending_changes(
                    matcher,
                    ignore_matcher,
                    ignore_dirs,
                    include_ignored,
                    config,
                    lgr,
                )
            }
            Err(err) => Err(err),
        }
    }

    fn sparse_matcher(
        &self,
        manifests: &[Arc<TreeManifest>],
        dot_dir: &'static str,
    ) -> Result<Option<DynMatcher>> {
        self.inner.sparse_matcher(manifests, dot_dir)
    }
}

fn warn_about_fresh_instance(
    lgr: &TermLogger,
    old_pid: Option<u32>,
    new_pid: Option<u32>,
) -> Result<()> {
    match (old_pid, new_pid) {
        (Some(old_pid), Some(new_pid)) if old_pid != new_pid => {
            lgr.warn(format!(
                "warning: watchman has recently restarted (old pid {}, new pid {}) - operation will be slower than usual",
                old_pid, new_pid
            ));
        }
        (None, Some(new_pid)) => {
            lgr.warn(format!(
                "warning: watchman has recently started (pid {}) - operation will be slower than usual",
                new_pid
            ));
        }
        _ => {
            lgr.warn(
                "warning: watchman failed to catch up with file change events and requires a full scan - operation will be slower than usual");
        }
    }

    Ok(())
}

// Given the existing treestate and files watchman says to check,
// figure out all the files that may have changed and check them for
// changes. Also track paths we need to mark or unmark as NEED_CHECK
// in the treestate.
#[tracing::instrument(skip_all)]
pub(crate) fn detect_changes(
    matcher: DynMatcher,
    ignore_matcher: DynMatcher,
    track_ignored: bool,
    include_ignored: bool,
    mut file_change_detector: impl FileChangeDetectorTrait + 'static,
    ts: &mut TreeState,
    wm_need_check: Vec<metadata::File>,
    wm_fresh_instance: bool,
    fs_case_sensitive: bool,
) -> Result<WatchmanPendingChanges> {
    let _span = tracing::info_span!("prepare stuff").entered();

    let (ts_need_check, ts_errors) = list_needs_check(ts, matcher)?;

    // NB: ts_need_check is filtered by the matcher, so it does not
    // necessarily contain all NEED_CHECK entries in the treestate.
    let ts_need_check: HashSet<_> = ts_need_check.into_iter().collect();

    let mut pending_changes: Vec<Result<PendingChange>> =
        ts_errors.into_iter().map(|e| Err(anyhow!(e))).collect();
    let mut needs_clear: Vec<(RepoPathBuf, Option<Metadata>)> = Vec::new();
    let mut needs_mark = Vec::new();

    let wm_need_check = normalize_watchman_files(ts, wm_need_check, fs_case_sensitive)?;

    tracing::debug!(
        watchman_needs_check = wm_need_check.len(),
        treestate_needs_check = ts_need_check.len(),
    );

    let total_needs_check = ts_need_check.len()
        + wm_need_check
            .iter()
            .filter(|(p, _)| !ts_need_check.contains(*p))
            .count();

    // This is to set "total" for progress bar.
    file_change_detector.total_work_hint(total_needs_check as u64);

    drop(_span);

    let _span = tracing::info_span!("submit ts_need_check").entered();

    for ts_needs_check in ts_need_check.iter() {
        // Prefer to kick off file check using watchman data since that already
        // includes disk metadata.
        if wm_need_check.contains_key(ts_needs_check) {
            continue;
        }

        // This check is important when we are tracking ignored files.
        // We won't do a fresh watchman query, so we must get the list
        // of ignored files from the treestate.
        if include_ignored && ignore_matcher.matches_file(ts_needs_check)? {
            pending_changes.push(Ok(PendingChange::Ignored(ts_needs_check.clone())));
            continue;
        }

        // We don't need the ignore check since ts_need_check was filtered by
        // the full matcher, which incorporates the ignore matcher.
        file_change_detector.submit(metadata::File {
            path: ts_needs_check.clone(),
            ts_state: ts.normalized_get(ts_needs_check)?,
            fs_meta: None,
        });
    }

    drop(_span);

    let mut deletes = Vec::new();

    if wm_fresh_instance {
        let _span =
            tracing::info_span!("fresh_instance work", wm_len = wm_need_check.len()).entered();

        // On fresh instance, watchman returns all files present on
        // disk. We need to catch the case where a tracked file has been
        // deleted while watchman wasn't running. To do that, report a
        // pending "delete" change for all EXIST_NEXT files that were
        // _not_ in the list we got from watchman.
        walk_treestate(
            ts,
            Arc::new(AlwaysMatcher::new()),
            StateFlags::EXIST_NEXT,
            StateFlags::empty(),
            StateFlags::NEED_CHECK,
            |path, _state| {
                if !wm_need_check.contains_key(&path) {
                    deletes.push(path);
                }
                Ok(())
            },
        )?;

        // Clear out ignored/untracked files that have been deleted.
        walk_treestate(
            ts,
            Arc::new(AlwaysMatcher::new()),
            StateFlags::NEED_CHECK,
            StateFlags::empty(),
            StateFlags::EXIST_NEXT | StateFlags::EXIST_P1 | StateFlags::EXIST_P2,
            |path, _state| {
                if !wm_need_check.contains_key(&path) {
                    needs_clear.push((path, None));
                }
                Ok(())
            },
        )?;
    }

    let _span = tracing::info_span!("submit wm_need_check").entered();

    for (_, wm_needs_check) in wm_need_check {
        // is_tracked is used to short circuit invocations of the ignore
        // matcher, which can be expensive.
        let is_tracked = match &wm_needs_check.ts_state {
            Some(state) => state
                .state
                .intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2 | StateFlags::EXIST_NEXT),
            None => false,
        };

        if !is_tracked {
            if let Some(Some(fs_meta)) = &wm_needs_check.fs_meta {
                if fs_meta.is_dir() {
                    continue;
                }
            }

            let ignored = ignore_matcher.matches_file(&wm_needs_check.path)?;
            if include_ignored && ignored {
                pending_changes.push(Ok(PendingChange::Ignored(wm_needs_check.path.clone())));
            }
            if !track_ignored && ignored {
                continue;
            }
        }

        file_change_detector.submit(wm_needs_check);
    }

    drop(_span);

    let _span = tracing::info_span!("handle results").entered();

    for result in file_change_detector {
        match result {
            Ok(ResolvedFileChangeResult::Yes(change)) => {
                let path = change.get_path();
                if let PendingChange::Deleted(path) = change {
                    deletes.push(path);
                } else {
                    if !ts_need_check.contains(path) {
                        needs_mark.push(path.clone());
                    }
                    pending_changes.push(Ok(change));
                }
            }
            Ok(ResolvedFileChangeResult::No((path, fs_meta))) => {
                // File is clean. Update treestate entry if it was marked
                // NEED_CHECK, or if we have fs_meta which implies treestate
                // metadata (e.g. mtime, size, etc.) is out of date.
                if ts_need_check.contains(&path) || fs_meta.is_some() {
                    needs_clear.push((path, fs_meta));
                }
            }
            Err(e) => pending_changes.push(Err(e)),
        }
    }

    drop(_span);

    for d in deletes {
        if !ts_need_check.contains(&d) {
            needs_mark.push(d.clone());
        }
        pending_changes.push(Ok(PendingChange::Deleted(d)));
    }

    Ok(WatchmanPendingChanges {
        pending_changes,
        needs_clear,
        needs_mark,
    })
}

fn normalize_watchman_files(
    ts: &mut TreeState,
    wm_files: Vec<metadata::File>,
    fs_case_sensitive: bool,
) -> Result<HashMap<RepoPathBuf, metadata::File>> {
    let mut wm_need_check = HashMap::with_capacity(wm_files.len());

    for mut file in wm_files {
        let (normalized_path, state) = ts.normalize_path_and_get(file.path.as_ref())?;

        let normalized_path = RepoPath::from_utf8(&normalized_path)?;

        let path_differs = normalized_path != file.path.as_ref();

        if path_differs
            && state.as_ref().is_some_and(|state| {
                state.state.intersects(StateFlags::EXIST_P1)
                    && !state.state.intersects(StateFlags::EXIST_NEXT)
            })
        {
            // Don't normalize into a pending "remove". This is the one case we
            // allow case colliding paths on case insensitive filesystems.
            tracing::trace!(
                "not normalizing {:?} since {:?} is removed",
                file.path,
                normalized_path
            );
            wm_need_check.insert(file.path.clone(), file);
            continue;
        }

        if !fs_case_sensitive {
            if let Some(existing) = wm_need_check.get(normalized_path) {
                if matches!(existing.fs_meta, Some(Some(_))) {
                    // After a case sensitive file rename on a case insensitive
                    // filesystem, watchman reports a delete and an add. We need
                    // to fold those two events together, which we do here by
                    // preserving the add.
                    tracing::trace!(path = ?file.path, "dropping in favor of exists-on-disk");
                    continue;
                }
            }
        }

        if path_differs {
            file.path = normalized_path.to_owned();
        }

        file.ts_state = state;

        wm_need_check.insert(file.path.clone(), file);
    }

    Ok(wm_need_check)
}

pub struct WatchmanPendingChanges {
    pending_changes: Vec<Result<PendingChange>>,
    needs_clear: Vec<(RepoPathBuf, Option<Metadata>)>,
    needs_mark: Vec<RepoPathBuf>,
}

impl WatchmanPendingChanges {
    #[tracing::instrument(skip_all)]
    pub fn update_treestate(&mut self, ts: &mut TreeState) -> Result<bool> {
        let bar = ProgressBar::new_adhoc(
            "recording files",
            (self.needs_clear.len() + self.needs_mark.len()) as u64,
            "entries",
        );

        let mut wrote = false;
        for (path, fs_meta) in self.needs_clear.drain(..) {
            match clear_needs_check(ts, &path, fs_meta) {
                Ok(v) => wrote |= v,
                Err(e) =>
                // We can still build a valid result if we fail to clear the
                // needs check flag. Propagate the error to the caller but allow
                // the persist to continue.
                {
                    self.pending_changes.push(Err(e))
                }
            }

            bar.increase_position(1);
        }

        for path in self.needs_mark.iter() {
            wrote |= mark_needs_check(ts, path)?;
            bar.increase_position(1);
        }

        Ok(wrote)
    }
}

impl IntoIterator for WatchmanPendingChanges {
    type Item = Result<PendingChange>;
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
