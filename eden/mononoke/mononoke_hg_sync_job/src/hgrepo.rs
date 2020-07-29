/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, format_err, Error, Result};
use bookmarks::BookmarkName;
use cloned::cloned;
use failure_ext::FutureFailureErrorExt;
use futures::future::{FutureExt as _, TryFutureExt};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use futures_old::future::{self, err, ok, Either, Future, IntoFuture};
use mercurial_types::HgChangesetId;
use mononoke_hg_sync_job_helper_lib::{lines_after, read_file_contents, wait_till_more_lines};
use parking_lot::Mutex;
use slog::{debug, info, Logger};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio_io::io::{flush, write_all};
use tokio_old::prelude::FutureExt as TokioFutureExt;
use tokio_old::timer::timeout::Error as TimeoutError;
use tokio_process::{Child, ChildStdin, CommandExt};
use tokio_timer::sleep;

const BOOKMARK_LOCATION_LOOKUP_TIMEOUT_MS: u64 = 10_000;
const LIST_SERVER_BOOKMARKS_EXTENSION: &str = include_str!("listserverbookmarks.py");
const SEND_UNBUNDLE_REPLAY_EXTENSION: &str = include_str!("sendunbundlereplay.py");

pub fn list_hg_server_bookmarks(
    hg_repo_path: String,
) -> BoxFuture<HashMap<BookmarkName, HgChangesetId>, Error> {
    let extension_file = try_boxfuture!(NamedTempFile::new());
    let file_path = try_boxfuture!(extension_file
        .path()
        .to_str()
        .ok_or(Error::msg("Temp file path contains non-unicode chars")));
    try_boxfuture!(fs::write(file_path, LIST_SERVER_BOOKMARKS_EXTENSION));
    let ext = format!("extensions.listserverbookmarks={}", file_path);

    let full_args = vec![
        "--config",
        &ext,
        "listserverbookmarks",
        "--path",
        &hg_repo_path,
    ];
    let cmd = Command::new("hg").args(&full_args).output();

    cmd.into_future()
        .from_err()
        .into_future()
        .and_then(|output| {
            let mut res = HashMap::new();
            for keyvalue in output.stdout.split(|x| x == &0) {
                if keyvalue.is_empty() {
                    continue;
                }
                let mut iter = keyvalue.split(|x| x == &1);
                match (iter.next(), iter.next()) {
                    (Some(key), Some(value)) => {
                        let key = String::from_utf8(key.to_vec()).map_err(Error::from)?;
                        let value = String::from_utf8(value.to_vec()).map_err(Error::from)?;
                        res.insert(BookmarkName::new(key)?, HgChangesetId::from_str(&value)?);
                    }
                    _ => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        bail!("invalid format returned from server: {}", stdout);
                    }
                }
            }
            Ok(res)
        })
        .context("While listing server bookmarks")
        .from_err()
        .boxify()
}

fn expected_location_string_arg(maybe_hgcsid: Option<HgChangesetId>) -> String {
    match maybe_hgcsid {
        Some(hash) => hash.to_string(),
        None => "DELETED".into(),
    }
}

fn get_hg_command<I, S>(args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let full_args = vec![
        "--config",
        "extensions.clienttelemetry=",
        "--config",
        "clienttelemetry.announceremotehostname=on",
    ]
    .into_iter()
    .map(|item| item.into())
    .chain(args.into_iter().map(|item| item.as_ref().to_os_string()));
    let mut child = Command::new(&"hg");
    child.args(full_args);
    child
}

#[derive(Clone)]
struct AsyncProcess {
    child: Arc<Mutex<Option<Child>>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    can_be_used: Arc<AtomicBool>,
}

impl AsyncProcess {
    pub fn new<I, S>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Self::from_command(get_hg_command(args))
    }

    fn from_command(mut command: Command) -> Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .spawn_async()
            .map_err(|e| format_err!("Couldn't spawn hg command: {:?}", e))?;
        let stdin = child
            .stdin()
            .take()
            .ok_or(Error::msg("ChildStdin unexpectedly not captured"))?;
        Ok(Self {
            child: Arc::new(Mutex::new(Some(child))),
            stdin: Arc::new(Mutex::new(Some(stdin))),
            can_be_used: Arc::new(AtomicBool::new(true)),
        })
    }

    pub fn write_line(&self, line: Vec<u8>) -> BoxFuture<(), Error> {
        let stdin = try_boxfuture!(self.stdin.lock().take().ok_or(Error::msg(
            "AsyncProcess unexpectedly does not contain stdin."
        )));
        let stdin_arc = self.stdin.clone();
        let process = self.clone();
        write_all(stdin, line)
            .and_then(move |(stdin, _)| flush(stdin))
            .map(move |stdin| {
                // Need to put stdin back
                stdin_arc.lock().replace(stdin);
            })
            .map_err(move |e| {
                // If we failed for whichever reason, we can't reuse the
                // same process. The failure might've been related to the process
                // itself, rather than the bundle. Let's err on the safe side
                process.invalidate();
                format_err!("{}", e)
            })
            .boxify()
    }

    pub fn invalidate(&self) {
        self.can_be_used.store(false, Ordering::SeqCst);
    }

    pub fn is_valid(&self) -> bool {
        self.can_be_used.load(Ordering::SeqCst)
    }

    pub fn kill(&self, logger: Logger) {
        self.child.lock().as_mut().map(|child| {
            child
                .kill()
                .unwrap_or_else(|e| debug!(logger, "failed to kill the hg process: {}", e))
        });
    }

    /// Make sure child is still alive while provided future is being executed
    /// If `grace_period` is specified, future will be given additional time
    /// to resolve even if peer has already been terminated.
    pub fn ensure_alive<F: Future<Error = Error>>(
        &self,
        fut: F,
        grace_period: Option<Duration>,
    ) -> impl Future<Item = F::Item, Error = Error> {
        let child = match self.child.lock().take() {
            None => return future::err(Error::msg("hg peer is dead")).left_future(),
            Some(child) => child,
        };

        let with_grace_period = move |fut: F, error| match grace_period {
            None => future::err(error).left_future(),
            Some(grace_period) => fut
                .select(sleep(grace_period).then(|_| Err(error)))
                .then(|result| match result {
                    Ok((ok, _)) => Ok(ok),
                    Err((err, _)) => Err(err),
                })
                .right_future(),
        };

        child
            .select2(fut)
            .then({
                let this = self.clone();
                move |result| match result {
                    Ok(Either::A((exit_status, fut))) => {
                        this.invalidate();
                        with_grace_period(
                            fut,
                            format_err!("hg peer has died unexpectedly: {}", exit_status),
                        )
                        .right_future()
                    }
                    Err(Either::A((child_err, fut))) => {
                        this.invalidate();
                        with_grace_period(fut, child_err.into()).right_future()
                    }
                    Ok(Either::B((future_ok, child))) => {
                        this.child.lock().replace(child);
                        future::ok(future_ok).left_future()
                    }
                    Err(Either::B((future_err, child))) => {
                        this.child.lock().replace(child);
                        future::err(future_err).left_future()
                    }
                }
            })
            .right_future()
    }
}

#[derive(Clone)]
struct HgPeer {
    process: AsyncProcess,
    reports_file: Arc<NamedTempFile>,
    bundle_applied: Arc<AtomicUsize>,
    max_bundles_allowed: usize,
    baseline_bundle_timeout_ms: u64,
    extension_file: Arc<NamedTempFile>,
}

impl !Sync for HgPeer {}

impl HgPeer {
    pub fn new(
        repo_path: &str,
        max_bundles_allowed: usize,
        baseline_bundle_timeout_ms: u64,
    ) -> Result<Self> {
        let reports_file = NamedTempFile::new()?;
        let file_path = reports_file
            .path()
            .to_str()
            .ok_or(Error::msg("Temp file path contains non-unicode chars"))?;

        let extension_file = NamedTempFile::new()?;
        let extension_path = extension_file
            .path()
            .to_str()
            .ok_or(Error::msg("Temp file path contains non-unicode chars"))?;
        fs::write(extension_path, SEND_UNBUNDLE_REPLAY_EXTENSION)?;

        let args = &[
            "--config",
            &format!("extensions.sendunbundlereplay={}", extension_path),
            "sendunbundlereplaybatch",
            "--debug",
            "--path",
            repo_path,
            "--reports",
            file_path,
        ];
        let process = AsyncProcess::new(args)?;

        Ok(HgPeer {
            process,
            reports_file: Arc::new(reports_file),
            bundle_applied: Arc::new(AtomicUsize::new(0)),
            max_bundles_allowed,
            baseline_bundle_timeout_ms,
            extension_file: Arc::new(extension_file),
        })
    }

    pub fn arc_mutexed(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }

    pub fn still_good(&self, logger: Logger) -> bool {
        let can_be_used: bool = self.process.is_valid();
        let bundle_applied: usize = self.bundle_applied.load(Ordering::SeqCst);
        let can_write_more = bundle_applied < self.max_bundles_allowed;
        debug!(
            logger,
            "can be used: {}, bundle_applied: {}, max bundles allowed: {}",
            can_be_used,
            bundle_applied,
            self.max_bundles_allowed
        );
        can_be_used && can_write_more
    }

    pub fn kill(&self, logger: Logger) {
        self.process.kill(logger);
    }

    pub fn apply_bundle(
        &self,
        bundle_path: &str,
        timestamps_path: &str,
        onto_bookmark: BookmarkName,
        expected_bookmark_position: Option<HgChangesetId>,
        attempt: usize,
        logger: Logger,
    ) -> impl Future<Item = (), Error = Error> {
        let mut log_file = match NamedTempFile::new() {
            Ok(log_file) => log_file,
            Err(e) => {
                return err(format_err!("could not create log file: {:?}", e)).left_future();
            }
        };

        let log_path = match log_file.path().to_str() {
            Some(log_path) => log_path,
            None => {
                return err(Error::msg("log_file path was not a valid string")).left_future();
            }
        };

        let onto_bookmark = onto_bookmark.to_string();
        let onto_bookmark = base64::encode(&onto_bookmark);
        let expected_hash = expected_location_string_arg(expected_bookmark_position);
        let input_line = format!(
            "{} {} {} {} {}\n",
            bundle_path, timestamps_path, onto_bookmark, expected_hash, log_path,
        );
        let path = self.reports_file.path().to_path_buf();
        let bundle_timeout_ms = self.baseline_bundle_timeout_ms * 2_u64.pow(attempt as u32 - 1);
        {
            cloned!(path);
            async move { lines_after(&path, 0).await }.boxed().compat()
        }
        .map(|v| v.len())
        .and_then({
            cloned!(self.process);
            move |line_num_in_reports_file| {
                process
                    .write_line(input_line.into_bytes())
                    .map(move |_| line_num_in_reports_file)
            }
        })
        .and_then({
            cloned!(logger, bundle_timeout_ms, self.bundle_applied, self.process);
            move |line_num_in_reports_file| {
                bundle_applied.fetch_add(1, Ordering::SeqCst);
                let response = async move {
                    wait_till_more_lines(path, line_num_in_reports_file, bundle_timeout_ms).await
                }
                .boxed()
                .compat()
                .and_then({
                    cloned!(process, logger);
                    move |report_lines| {
                        let full_report = report_lines.join("\n");
                        let success = !full_report.contains("failed");
                        debug!(logger, "sync report: {}", full_report);
                        if success {
                            Ok(())
                        } else {
                            process.invalidate();
                            let log = match read_file_contents(&mut log_file) {
                                Ok(log) => format!("hg logs follow:\n{}", log),
                                Err(e) => format!("no hg logs available ({:?})", e),
                            };
                            Err(format_err!("sync failed: {}", log))
                        }
                    }
                })
                .map_err({
                    cloned!(process);
                    move |err| {
                        info!(logger, "sync failed. Invalidating process");
                        process.invalidate();
                        err
                    }
                });
                process.ensure_alive(
                    response,
                    // even if peer process has died, lets wait for additional grace
                    // period, and trie to collect the report if any.
                    Some(Duration::from_secs(1)),
                )
            }
        })
        .right_future()
    }
}

/// Struct that knows how to work with on-disk mercurial repository.
/// It shells out to `hg` cmd line tool.
#[derive(Clone)]
pub struct HgRepo {
    repo_path: Arc<String>,
    peer: Arc<Mutex<HgPeer>>,
    max_bundles_per_peer: usize,
    baseline_bundle_timeout_ms: u64,
    verify_server_bookmark_on_failure: bool,
}

impl HgRepo {
    pub fn new(
        repo_path: String,
        max_bundles_per_peer: usize,
        baseline_bundle_timeout_ms: u64,
        verify_server_bookmark_on_failure: bool,
    ) -> Result<Self> {
        let peer = HgPeer::new(&repo_path, max_bundles_per_peer, baseline_bundle_timeout_ms)?;
        Ok(Self {
            repo_path: Arc::new(repo_path),
            peer: peer.arc_mutexed(),
            max_bundles_per_peer,
            baseline_bundle_timeout_ms,
            verify_server_bookmark_on_failure,
        })
    }

    pub fn apply_bundle(
        &self,
        bundle_filename: String,
        timestamps_path: String,
        onto_bookmark: BookmarkName,
        expected_bookmark_position: Option<HgChangesetId>,
        attempt: usize,
        logger: Logger,
    ) -> impl Future<Item = (), Error = Error> {
        match self.renew_peer_if_needed(logger.clone()) {
            Ok(_) => self
                .peer
                .lock()
                .apply_bundle(
                    &bundle_filename,
                    &timestamps_path,
                    onto_bookmark.clone(),
                    expected_bookmark_position.clone(),
                    attempt,
                    logger.clone(),
                )
                .or_else({
                    let this = self.clone();
                    cloned!(onto_bookmark, expected_bookmark_position, logger);
                    move |sync_error| {
                        if !this.verify_server_bookmark_on_failure {
                            return err(sync_error).left_future();
                        }
                        info!(
                            logger,
                            "sync failed, let's check if the bookmark is where we want \
                             it to be anyway"
                        );
                        this.verify_server_bookmark_location(
                            &onto_bookmark,
                            expected_bookmark_position,
                        )
                        .map_err(|_verification_error| sync_error)
                        .right_future()
                    }
                })
                .boxify(),
            Err(e) => err(e).boxify(),
        }
    }

    fn renew_peer_if_needed(&self, logger: Logger) -> Result<()> {
        if !self.peer.lock().still_good(logger.clone()) {
            debug!(logger, "killing the old peer");
            self.peer.lock().kill(logger.clone());
            debug!(logger, "renewing hg peer");
            let new_peer = HgPeer::new(
                &self.repo_path.clone(),
                self.max_bundles_per_peer,
                self.baseline_bundle_timeout_ms,
            )?;
            *self.peer.lock() = new_peer;
            Ok(debug!(logger, "done renewing hg peer"))
        } else {
            Ok(debug!(logger, "existing hg peer is still good"))
        }
    }

    fn verify_server_bookmark_location(
        &self,
        name: &BookmarkName,
        expected_bookmark_position: Option<HgChangesetId>,
    ) -> impl Future<Item = (), Error = Error> {
        let name = name.to_string();
        let mut args: Vec<String> = [
            "checkserverbookmark",
            // NB: we can't enable extensions.checkserverbookmark universally until it
            //     is deployed as part of the package. For now, let it be enabled only when
            //     the appropriate command line flag is present (e.g. when this function is
            //     called)
            "--config",
            "extensions.checkserverbookmark=",
            "--path",
            &self.repo_path.clone(),
            "--name",
        ]
        .iter()
        .map(|item| item.to_string())
        .collect();
        args.push(name);
        match expected_bookmark_position {
            Some(hash) => {
                args.push("--hash".into());
                args.push(hash.to_string());
            }
            None => args.push("--deleted".into()),
        };
        let proc = match get_hg_command(args).stdin(Stdio::piped()).status_async() {
            Ok(proc) => proc,
            Err(_) => return err(Error::msg("failed to start a mercurial process")).left_future(),
        };
        proc.map_err(|e| format_err!("process error: {:?}", e))
            .timeout(Duration::from_millis(BOOKMARK_LOCATION_LOOKUP_TIMEOUT_MS))
            .map_err(remap_timeout_error)
            .and_then(|exit_status| {
                if exit_status.success() {
                    ok(())
                } else {
                    err(Error::msg(
                        "server does not have a bookmark in the expected location",
                    ))
                }
            })
            .right_future()
    }
}

fn remap_timeout_error(err: TimeoutError<Error>) -> Error {
    match err.into_inner() {
        Some(err) => err,
        None => Error::msg("timed out waiting for process"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use futures_old::Future;
    use tokio_compat::runtime::Runtime;
    use tokio_timer::sleep;

    #[test]
    fn ensure_alive_alive_process() -> Result<()> {
        let mut rt = Runtime::new()?;

        let command = {
            let mut command = Command::new("sleep");
            command.args(vec!["2"]);
            command
        };
        let proc = AsyncProcess::from_command(command)?;

        let fut = proc.ensure_alive(sleep(Duration::from_millis(100)).from_err(), None);
        let res = rt.block_on(fut);
        assert_matches!(res, Ok(()));

        assert!(proc.is_valid());
        Ok(())
    }

    #[test]
    fn ensure_alive_dead_process() -> Result<()> {
        let mut rt = Runtime::new()?;

        let proc = AsyncProcess::from_command(Command::new("false"))?;

        let fut = proc.ensure_alive(sleep(Duration::from_secs(5)).from_err(), None);
        let res = rt.block_on(fut);
        assert_matches!(res, Err(_));

        assert!(!proc.is_valid());
        Ok(())
    }

    #[test]
    fn ensure_alive_grace_period() -> Result<()> {
        let mut rt = Runtime::new()?;

        let proc = AsyncProcess::from_command(Command::new("false"))?;

        let fut = proc.ensure_alive(
            sleep(Duration::from_secs(1)).from_err(),
            Some(Duration::from_secs(10)),
        );
        let res = rt.block_on(fut);
        assert_matches!(res, Ok(()));

        assert!(!proc.is_valid());
        Ok(())
    }
}
