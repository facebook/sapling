/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::io;
use std::process::Child;
use std::process::Command;

use fs2::FileExt;
use spawn_ext::CommandExt;

use crate::util;

/// Attempt to spawn servers (from a client) so there will be `pool_size`
/// servers running in background.
pub fn spawn_pool(pool_size: usize) -> anyhow::Result<()> {
    let dir = util::runtime_dir()?;
    let prefix = util::prefix();
    let spawn_lock = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(dir.join("spawn.lock"))?;
    spawn_lock.lock_exclusive()?;

    let existing = udsipc::pool::list_uds_paths(&dir, prefix)
        .take(pool_size)
        .count();
    let needed = pool_size.saturating_sub(existing);

    tracing::debug!("spawning {} command servers", needed);
    for _ in 0..needed {
        spawn_one()?;
    }
    Ok(())
}

/// Attempt to spawn one server (from a client).
/// Assume `$0 --spawn-commandserver` is the way to run a command server.
pub fn spawn_one() -> io::Result<Child> {
    let arg0 = std::env::current_exe()?;
    let mut cmd = Command::new(arg0);
    cmd.arg("start-commandserver")
        .current_dir("/")
        // The server will get node channel fd via recv_stdio.
        // They should not have NODE_CHANNEL_FD via env vars.
        .env_remove("NODE_CHANNEL_FD")
        .new_session();

    tracing::debug!("spawning a command server");
    if tracing::enabled!(tracing::Level::DEBUG) {
        // Do not silent stderr for easier debugging.
        cmd.spawn()
    } else {
        // Silent stderr.
        cmd.spawn_detached()
    }
}
