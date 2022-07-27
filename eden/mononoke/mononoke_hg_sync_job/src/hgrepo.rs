/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::CommitsInBundle;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context as _;
use anyhow::Error;
use anyhow::Result;
use bookmarks::BookmarkName;
use futures::future;
use futures::future::FutureExt;
use futures::future::TryFuture;
use futures::future::TryFutureExt;
use futures_ext::future::FbFutureExt;
use futures_ext::future::FbTryFutureExt;
use futures_watchdog::WatchdogExt;
use itertools::Itertools;
use mercurial_types::HgChangesetId;
use mononoke_hg_sync_job_helper_lib::lines_after;
use mononoke_hg_sync_job_helper_lib::read_file_contents;
use mononoke_hg_sync_job_helper_lib::wait_till_more_lines;
use mononoke_hg_sync_job_helper_lib::write_to_named_temp_file;
use slog::debug;
use slog::info;
use slog::Logger;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::Command;
use tokio::sync::Mutex;

const BOOKMARK_LOCATION_LOOKUP_TIMEOUT_MS: u64 = 10_000;
const LIST_SERVER_BOOKMARKS_EXTENSION: &str = include_str!("listserverbookmarks.py");
const SEND_UNBUNDLE_REPLAY_EXTENSION: &str = include_str!("sendunbundlereplay.py");

pub async fn list_hg_server_bookmarks(
    hg_repo_path: String,
) -> Result<HashMap<BookmarkName, HgChangesetId>, Error> {
    let extension_file = NamedTempFile::new()?;
    let file_path = extension_file
        .path()
        .to_str()
        .ok_or_else(|| Error::msg("Temp file path contains non-unicode chars"))?;
    fs::write(file_path, LIST_SERVER_BOOKMARKS_EXTENSION)?;
    let ext = format!("extensions.listserverbookmarks={}", file_path);

    let full_args = vec![
        "--config",
        &ext,
        "listserverbookmarks",
        "--path",
        &hg_repo_path,
    ];

    let output = get_hg_command(&full_args)
        .output()
        .await
        .context("Error listing server bookmarks")?;

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
        "--traceback",
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

struct AsyncProcess {
    child: Child,
    stdin: ChildStdin,
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
            .spawn()
            .context("Failed to spawn hg command")?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::msg("ChildStdin unexpectedly not captured"))?;
        Ok(Self { child, stdin })
    }

    pub async fn write_line(&mut self, line: Vec<u8>) -> Result<(), Error> {
        self.stdin
            .write_all(&line)
            .await
            .context("Failed to write")?;
        Ok(())
    }

    pub fn is_valid(&mut self) -> bool {
        !self.child_is_finished()
    }

    pub async fn kill(&mut self, logger: &Logger) {
        if let Err(e) = self.child.kill().await {
            debug!(logger, "failed to kill the hg process: {}", e);
        }
    }

    fn child_is_finished(&mut self) -> bool {
        self.child.try_wait().transpose().is_some()
    }

    /// Make sure child is still alive while provided future is being executed
    /// If `grace_period` is specified, future will be given additional time
    /// to resolve even if peer has already been terminated.
    pub async fn ensure_alive<F>(
        &mut self,
        fut: F,
        grace_period: Option<Duration>,
    ) -> Result<<F as TryFuture>::Ok, <F as TryFuture>::Error>
    where
        F: TryFuture<Error = Error> + Unpin + Send,
    {
        if self.child_is_finished() {
            return Err(Error::msg("hg peer is dead"));
        }

        let child = self.child.wait();

        let watchdog = async {
            let res = child.await;
            if let Some(grace_period) = grace_period {
                tokio::time::sleep(grace_period).await;
            }
            Err(format_err!("hg peer has died unexpectedly: {:?}", res))
        }
        .boxed();

        // NOTE: Right can't actually return an Ok variant here (which is why this compiles at
        // all), but it still needs to be in the match clause.
        match future::try_select(fut, watchdog).await {
            Ok(future::Either::Left((res, _))) | Ok(future::Either::Right((res, _))) => Ok(res),
            Err(future::Either::Left((e, _))) | Err(future::Either::Right((e, _))) => Err(e),
        }
    }
}

struct HgPeer {
    process: AsyncProcess,
    reports_file: Arc<NamedTempFile>,
    bundle_applied: usize,
    max_bundles_allowed: usize,
    baseline_bundle_timeout_ms: u64,
    invalidated: bool,
    // The extension_file needs to be kept around while we have running instances of the process.
    #[allow(unused)]
    extension_file: Arc<File>,
}

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
            .ok_or_else(|| Error::msg("Temp file path contains non-unicode chars"))?;

        let extension_file = NamedTempFile::new().context("Error in creating extension file")?;
        // Persisting the file so it does not get deleted before its contents are read.
        let (extension_file, extension_path) = extension_file.keep()?;
        let path_string = extension_path.clone();
        fs::write(extension_path, SEND_UNBUNDLE_REPLAY_EXTENSION)
            .with_context(|| format!("Error in writing data to file {}", &path_string.display()))?;
        let args = &[
            "--config",
            &format!("extensions.sendunbundlereplay={}", &path_string.display()),
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
            bundle_applied: 0,
            max_bundles_allowed,
            baseline_bundle_timeout_ms,
            invalidated: false,
            extension_file: Arc::new(extension_file),
        })
    }

    pub fn arc_mutexed(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }

    pub fn still_good(&mut self, logger: &Logger) -> bool {
        let can_be_used: bool = !self.invalidated & self.process.is_valid();
        let can_write_more = self.bundle_applied < self.max_bundles_allowed;
        debug!(
            logger,
            "can be used: {}, bundle_applied: {}, max bundles allowed: {}",
            can_be_used,
            self.bundle_applied,
            self.max_bundles_allowed
        );
        can_be_used && can_write_more
    }

    pub async fn kill(&mut self, logger: &Logger) {
        self.process.kill(logger).await;
    }

    pub async fn apply_bundle<'a>(
        &'a mut self,
        bundle_path: &'a str,
        timestamps_path: &'a str,
        onto_bookmark: BookmarkName,
        expected_bookmark_position: Option<HgChangesetId>,
        attempt: usize,
        logger: &Logger,
        commits_in_bundle: &CommitsInBundle,
    ) -> Result<(), Error> {
        let mut log_file = match NamedTempFile::new() {
            Ok(log_file) => log_file,
            Err(e) => {
                return Err(format_err!("could not create log file: {:?}", e));
            }
        };

        let log_path = match log_file.path().to_str() {
            Some(log_path) => log_path,
            None => {
                return Err(Error::msg("log_file path was not a valid string"));
            }
        };

        let onto_bookmark = onto_bookmark.to_string();
        let onto_bookmark = base64::encode(&onto_bookmark);
        let expected_hash = expected_location_string_arg(expected_bookmark_position);

        let hgbonsaimapping = match commits_in_bundle {
            CommitsInBundle::Commits(hgbonsaimapping) => {
                let encoded_hg_bonsai_mapping = hgbonsaimapping
                    .iter()
                    .map(|(hg_cs_id, bcs_id)| format!("{}={}", hg_cs_id, bcs_id))
                    .join("\n");

                encoded_hg_bonsai_mapping
            }
            CommitsInBundle::Unknown => "".to_string(),
        };

        let hgbonsaimappingfile = write_to_named_temp_file(hgbonsaimapping)
            .watched(logger)
            .await?;
        let hgbonsaimappingpath = match hgbonsaimappingfile.path().to_str() {
            Some(path) => path,
            None => {
                return Err(Error::msg("hg bonsai mapping path was not a valid string"));
            }
        };

        let input_line = format!(
            "{} {} {} {} {} {}\n",
            bundle_path,
            timestamps_path,
            hgbonsaimappingpath,
            onto_bookmark,
            expected_hash,
            log_path,
        );
        let path = self.reports_file.path().to_path_buf();
        let bundle_timeout_ms = self.baseline_bundle_timeout_ms * 2_u64.pow(attempt as u32 - 1);

        let line_num_in_reports_file = lines_after(&path, 0).watched(logger).await?.len();

        let res = async {
            self.process
                .write_line(input_line.into_bytes())
                .watched(logger)
                .await?;
            self.bundle_applied += 1;

            let report_lines = self
                .process
                .ensure_alive(
                    wait_till_more_lines(path, line_num_in_reports_file, bundle_timeout_ms).boxed(),
                    // even if peer process has died, lets wait for additional grace
                    // period, and try to collect the report if any.
                    Some(Duration::from_secs(1)),
                )
                .watched(logger)
                .await?;

            let full_report = report_lines.join("\n");
            let success = !full_report.contains("failed");
            debug!(logger, "sync report: {}", full_report);

            if success {
                return Ok(());
            }

            let log = match read_file_contents(&mut log_file) {
                Ok(log) => format!("hg logs follow:\n{}", log),
                Err(e) => format!("no hg logs available ({:?})", e),
            };

            return Err(format_err!("sync failed: {}", log));
        }
        .watched(logger)
        .await;

        if res.is_err() {
            info!(logger, "sync failed. Invalidating process");
            self.invalidated = true;
        }

        res
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

    pub async fn apply_bundle(
        &self,
        bundle_filename: String,
        timestamps_path: String,
        onto_bookmark: BookmarkName,
        expected_bookmark_position: Option<HgChangesetId>,
        attempt: usize,
        logger: &Logger,
        commits_in_bundle: &CommitsInBundle,
    ) -> Result<(), Error> {
        self.renew_peer_if_needed(logger).await?;

        let mut peer = self.peer.lock().await;

        let res = peer
            .apply_bundle(
                &bundle_filename,
                &timestamps_path,
                onto_bookmark.clone(),
                expected_bookmark_position.clone(),
                attempt,
                logger,
                commits_in_bundle,
            )
            .watched(logger.clone())
            .await;

        let err = match res {
            Ok(()) => return Ok(()),
            Err(err) => err,
        };

        if !self.verify_server_bookmark_on_failure {
            return Err(err);
        }

        info!(
            logger,
            "sync failed, let's check if the bookmark is where we want it to be anyway",
        );

        if self
            .verify_server_bookmark_location(&onto_bookmark, expected_bookmark_position)
            .watched(logger.clone())
            .await
            .is_ok()
        {
            return Ok(());
        }

        Err(err)
    }

    async fn renew_peer_if_needed(&self, logger: &Logger) -> Result<()> {
        let mut peer = self.peer.lock().watched(logger).await;

        if peer.still_good(logger) {
            return Ok(debug!(logger, "existing hg peer is still good"));
        }

        debug!(logger, "killing the old peer");
        peer.kill(logger).await;

        debug!(logger, "renewing hg peer");
        let new_peer = HgPeer::new(
            &self.repo_path.clone(),
            self.max_bundles_per_peer,
            self.baseline_bundle_timeout_ms,
        )?;
        *peer = new_peer;
        Ok(debug!(logger, "done renewing hg peer"))
    }

    async fn verify_server_bookmark_location(
        &self,
        name: &BookmarkName,
        expected_bookmark_position: Option<HgChangesetId>,
    ) -> Result<(), Error> {
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

        let mut cmd = get_hg_command(args)
            .stdin(Stdio::piped())
            .spawn()
            .context("failed to start a mercurial process")?;

        let exit_status = cmd
            .wait()
            .map_err(Error::from)
            .timeout(Duration::from_millis(BOOKMARK_LOCATION_LOOKUP_TIMEOUT_MS))
            .flatten_err()
            .await?;

        if exit_status.success() {
            Ok(())
        } else {
            Err(Error::msg(
                "server does not have a bookmark in the expected location",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;

    #[tokio::test]
    async fn ensure_alive_alive_process() -> Result<()> {
        let command = {
            let mut command = Command::new("sleep");
            command.args(vec!["2"]);
            command
        };
        let mut proc = AsyncProcess::from_command(command)?;

        let res = proc
            .ensure_alive(
                async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok(())
                }
                .boxed(),
                None,
            )
            .await;
        assert_matches!(res, Ok(()));

        assert!(proc.is_valid());
        Ok(())
    }

    #[tokio::test]
    async fn ensure_alive_dead_process() -> Result<()> {
        let mut proc = AsyncProcess::from_command(Command::new("false"))?;

        // Give the command a little time to finish
        tokio::time::sleep(Duration::from_millis(100)).await;
        let res = proc
            .ensure_alive(
                async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok(())
                }
                .boxed(),
                None,
            )
            .await;
        assert_matches!(res, Err(_));
        assert!(res.unwrap_err().to_string().starts_with("hg peer is dead"));

        assert!(!proc.is_valid());
        Ok(())
    }

    #[tokio::test]
    async fn ensure_alive_no_grace_period() -> Result<()> {
        let command = {
            let mut command = Command::new("sleep");
            command.args(vec!["0.1"]);
            command
        };
        let mut proc = AsyncProcess::from_command(command)?;

        let res = proc
            .ensure_alive(
                async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok(())
                }
                .boxed(),
                None,
            )
            .await;
        assert_matches!(res, Err(_));
        assert!(res.unwrap_err().to_string().starts_with("hg peer has died"));

        assert!(!proc.is_valid());
        Ok(())
    }

    #[tokio::test]
    async fn ensure_alive_grace_period() -> Result<()> {
        let command = {
            let mut command = Command::new("sleep");
            command.args(vec!["0.1"]);
            command
        };
        let mut proc = AsyncProcess::from_command(command)?;

        let res = proc
            .ensure_alive(
                async {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    Ok(())
                }
                .boxed(),
                Some(Duration::from_secs(10)),
            )
            .await;
        assert_matches!(res, Ok(()));

        assert!(!proc.is_valid());
        Ok(())
    }
}
