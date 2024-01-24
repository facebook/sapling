/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use clidispatch::ReqCtx;

use super::define_flags;
use super::Repo;
use super::Result;

define_flags! {
    pub struct Opts {
        /// detect visible commits and bookmarks change
        commits: bool,

        /// detect working parents (current commit, '.') change
        wdir_parents: bool,

        /// detect working copy file changes (status and diff)
        wdir_content: bool,

        /// maximum changes before exiting (0 or negative: unlimited)
        #[short('n')]
        limit: i64 = 0,
    }
}

pub fn run(ctx: ReqCtx<Opts>, repo: &mut Repo) -> Result<u8> {
    let limit = ctx.opts.limit;
    let io = ctx.io();
    let (tx, rx) = mpsc::channel::<&'static str>();
    let mut count = 0;
    let wait_thread_count = AtomicUsize::new(0);

    let spawn_wait_thread =
        |name: &'static str, mut wait: Box<dyn FnMut() -> Result<()> + Send>| -> Result<()> {
            wait_thread_count.fetch_add(1, Ordering::Release);
            let mut err = io.error();
            let tx = tx.clone();
            let mut error_start = None;
            std::thread::Builder::new()
                .name(format!("wait:{}", name))
                .spawn(move || {
                    loop {
                        if let Err(e) = wait() {
                            if should_retry(&e, &mut error_start) {
                                tracing::warn!("retry error in wait:{}: {}\n{:?}", name, &e, &e);
                                std::thread::sleep(Duration::from_secs(1));
                                continue;
                            } else {
                                let _ = write!(err, "error({}): {}\n{:?}\n", name, &e, &e);
                                let _ = tx.send("error");
                                break;
                            }
                        }
                        error_start = None;
                        if tx.send(name).is_err() {
                            break;
                        }
                    }
                })?;
            Ok(())
        };

    if ctx.opts.commits {
        let metalog = repo.metalog()?;
        let metalog = metalog.read();
        let mut metalog = metalog.checkout(metalog.root_id())?;
        spawn_wait_thread(
            "commits",
            Box::new(move || -> anyhow::Result<()> {
                metalog = metalog.wait_for_change(&["bookmarks", "remotenames", "visibleheads"])?;
                Ok(())
            }),
        )?;
    }
    if ctx.opts.wdir_parents {
        let dot_dir = repo.dot_hg_path();
        let mut wait = treestate::Wait::from_dot_dir(dot_dir);
        spawn_wait_thread(
            "wdir-parents",
            Box::new(move || -> anyhow::Result<()> {
                wait.wait_for_parent_change()?;
                Ok(())
            }),
        )?;
    }
    if ctx.opts.wdir_content {
        let mut working_copy = repo.working_copy()?;
        let repo_path = repo.path().to_owned();
        let config = repo.config().clone();
        let mut wait = workingcopy::wait::Wait::new(&working_copy, repo.dot_hg_path(), &config)?;
        spawn_wait_thread(
            "wdir-content",
            Box::new(move || -> anyhow::Result<()> {
                loop {
                    let v = wait.wait_for_change(&working_copy, &config)?;
                    if v.should_reload_working_copy() {
                        let mut repo = Repo::load(&repo_path, &[], &[])?;
                        working_copy = repo.working_copy()?;
                        continue;
                    }
                    break;
                }
                Ok(())
            }),
        )?;
    }

    let mut ret = 0;
    drop(tx);

    if wait_thread_count.load(Ordering::Acquire) == 0 {
        ret = 1;
        if !ctx.global_opts().quiet {
            io.write_err("nothing to wait (see '--help')\n")?;
        }
    } else {
        while let Ok(changed) = rx.recv() {
            if changed == "error" {
                ret = 254;
                break;
            }
            io.write(format!("{}\n", changed))?;
            io.flush()?;
            count += 1;
            if limit > 0 && count >= limit {
                break;
            }
        }
    }
    Ok(ret)
}

pub fn aliases() -> &'static str {
    "debugwait"
}

pub fn doc() -> &'static str {
    r#"print a line on change

Wait for changes and print what changed.

With --commits, wait for bookmarks and visible heads change,
then print ``commits`` to stdout.

With --wdir-parents, wait for "working directory parents" changes,
then print a line ``wdir-parents`` to stdout.

With a positive --limit, exit after detecting the given number of changes.
If --limit is 0, detect changes in an endless loop (use Ctrl+C to stop).

If an internal watcher encountered a fatal error and can no longer
properly watch for changes, the error will be printed to stderr, and the command
will exit with code 254.
"#
}

pub fn synopsis() -> Option<&'static str> {
    None
}

/// Decide whether to retry when a wait thread encountered an error.
///
/// Errors like edenfs commit out-of-date is considered transient and will be
/// retired with limited patience.
fn should_retry(error: &anyhow::Error, error_start: &mut Option<Instant>) -> bool {
    if error_start.is_none() {
        // Track the error start time.
        *error_start = Some(Instant::now());
    }

    #[allow(unused_mut)]
    let mut patience: Option<Duration> = None;

    tracing::trace!("wait error: {:?}", error);
    #[cfg(feature = "eden")]
    {
        if let Some(error) = error.downcast_ref::<edenfs_client::EdenError>() {
            // Assuming that a checkout is in process. Expect those to recover later.
            tracing::trace!("eden error code: {}", error.error_type);
            if error.error_type == "OUT_OF_DATE_PARENT"
                || error.error_type == "CHECKOUT_IN_PROGRESS"
            {
                patience = Some(Duration::from_secs(60));
            }
        }
    }

    if let (Some(start), Some(patience)) = (error_start.as_ref(), patience) {
        if start.elapsed() < patience {
            return true;
        }
    }

    false
}
