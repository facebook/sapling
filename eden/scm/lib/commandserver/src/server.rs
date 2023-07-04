/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use nodeipc::derive::Serve;

use crate::ipc::Server;

/// Serve one client.
///
/// Internally, creates and listens to a uds.
///
/// Exits the process when the uds file is removed and no clients
/// are connected.
///
/// Returns if completes serving a client.
pub fn serve_one_client<'a>(
    run_func: &'a (dyn (Fn(&'_ Server<'a>, Vec<String>) -> i32) + Send + Sync),
) -> anyhow::Result<()> {
    let dir = crate::util::runtime_dir()?;
    let prefix = crate::util::prefix();
    tracing::debug!("serving at {}/{}", dir.display(), prefix);
    let incoming = udsipc::pool::serve(&dir, prefix)?;

    let is_uds_alive = incoming.get_is_alive_func();
    let is_waiting = AtomicBool::new(true);
    let start_time = Instant::now();

    thread::scope(|s| {
        // `for ipc in incoming` might block forever waiting for
        // a client. Detect that and exit early.
        s.spawn(|| {
            let idle_timeout = Duration::from_secs(1800);
            let interval = Duration::from_secs(5);
            while is_waiting.load(Ordering::Acquire)
                && start_time.elapsed() < idle_timeout
                && is_uds_alive()
            {
                thread::sleep(interval);
            }
            if is_waiting.load(Ordering::Acquire) {
                tracing::debug!("exiting server due to inactivity");
                std::process::exit(0);
            }
        });

        tracing::debug!("waiting for client connection");
        #[allow(clippy::never_loop)]
        for ipc in incoming {
            tracing::debug!("got client connection");
            is_waiting.store(false, Ordering::Release);
            if let Err(e) = ipc.recv_stdio() {
                tracing::warn!("failed to get client stdio:\n{:?}", &e);
            } else {
                tracing::debug!("server got client stdio");
                let server = Server {
                    ipc: ipc.into(),
                    run_func,
                };
                let _ = server.serve();
            }
            break;
        }
    });

    Ok(())
}
