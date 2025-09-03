/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::IsTerminal;

use configmodel::Config;
use configmodel::ConfigExt;
use udsipc::pool;

use crate::ipc::Client;
use crate::ipc::CommandEnv;
use crate::ipc::ProcessProps;
use crate::ipc::ServerIpc;
use crate::spawn;
use crate::util;

/// Connect to a server to run a command. Returns exit code.
///
/// Error when no compatible server can be connected.
/// Spawn new servers on demand.
pub fn run_via_commandserver(args: Vec<String>, config: &dyn Config) -> anyhow::Result<i32> {
    let (should, reason) = should_run_remotely(&args);
    if !should {
        tracing::debug!("skipped using commandserver: {}", reason);
        anyhow::bail!("skipped using commandserver: {}", reason);
    }

    // For now, the server does not fork and can only be used with "exclusive".
    let exclusive = true;
    let dir = util::runtime_dir()?;
    let prefix = util::prefix();
    let ipc = match pool::connect(&dir, prefix, exclusive) {
        Err(e) => {
            tracing::debug!("no server to connect:\n{:?}", &e);
            if pool::list_uds_paths(&dir, prefix).next().is_none() {
                // No servers are running. Spawn a pool of servers.
                let pool_size = config.get_or::<usize>("commandserver", "pool-size", || 2)?;
                let _ = spawn::spawn_pool(pool_size);
            }
            return Err(e);
        }
        Ok(ipc) => {
            // Going to consume one server, so spawn another one.
            let _ = spawn::spawn_one();
            ipc
        }
    };

    tracing::debug!("sending stdio to server");
    ipc.send_stdio()?;

    // Check if the server is compatible.
    let client = Client { ipc };
    let props: ProcessProps = ServerIpc::process_props(&client)?;
    if let Some(ref server_groups) = props.groups {
        if let Some(ref client_groups) = util::groups() {
            if server_groups != client_groups {
                tracing::debug!("server groups mismatch");
                anyhow::bail!("Server groups do not match");
            }
        }
    }
    if let Some(server_nofile) = props.rlimit_nofile {
        if let Some(client_nofile) = util::rlimit_nofile() {
            if server_nofile < client_nofile {
                tracing::debug!("server RLIMIT_NOFILE incompatible");
                anyhow::bail!("Server RLIMIT_NOFILE is incompatible");
            }
        }
    }

    // Replace the server's env vars and chdir.
    // Disable demandimport as modules are expected to be pre-imported.
    let mut env = CommandEnv::current()?;
    env.env
        .push(("HGDEMANDIMPORT".to_owned(), "disable".to_owned()));
    let mask = util::get_umask();
    let applied = ServerIpc::apply_env(&client, env, mask)?;
    if !applied {
        tracing::debug!("server apply_env failed");
        anyhow::bail!("Server cannot apply env");
    }

    // We're likely going to use this command server.
    // On POSIX, forward signals so terminal resize, etc can work.
    // We don't use the "atexit" handler here, since it does not forward
    // signals like terminal resize, etc. This replaces the `atexit` handler.
    #[cfg(unix)]
    forward_signals(&props);

    // On Windows, terminate the server on Ctrl+C event. The server will kill
    // the pager process. We use an "AtExit" handler to handle Ctrl+C.
    #[cfg(windows)]
    let server_killer = atexit::AtExit::new({
        Box::new(move || {
            let _ = procutil::terminate_pid(props.pid, Some(Default::default()));
        })
    })
    .named("terminating server".into())
    .queued();

    // Send the run_command request.
    // Note the server might ask the client for "ui.system" requests.
    tracing::debug!("sending command request");
    let ret = ServerIpc::run_command(&client, args.clone())?;
    tracing::debug!("command {:?} returned: {}", &args, ret);

    // No need to kill the server if no Ctrl+C was pressed.
    #[cfg(windows)]
    server_killer.cancel();

    Ok(ret)
}

/// Check if a command should run remotely, with reasons.
/// See also `hgmain::chg`.
fn should_run_remotely(args: &[String]) -> (bool, &'static str) {
    // Bash might translate `<(...)` to `/dev/fd/x` instead of using a real fifo. That
    // path resolves to different fd by the chg server. Therefore chg cannot be used.
    if cfg!(unix)
        && args
            .iter()
            .any(|a| a.starts_with("/dev/fd/") || a.starts_with("/proc/self/"))
    {
        return (false, "arg starts with /dev/fd or /proc/self/");
    }

    // stdin is not a tty but stdout is a tty. Interactive pager is used
    // but lack of ctty makes it impossible to control the interactive
    // pager via keys.
    if cfg!(unix) && !std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return (false, "!stdin.is_tty() && stdout.is_tty()");
    }

    if let Some(val) = std::env::var_os("CHGDISABLE") {
        if val == "1" {
            return (false, "CHGDISABLE=1");
        }
    }

    (true, "")
}

#[cfg(unix)]
fn forward_signals(props: &ProcessProps) {
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;

    static PID: AtomicU32 = AtomicU32::new(0);
    static PGID: AtomicU32 = AtomicU32::new(0);

    unsafe extern "C" fn forward_signal_process(sig: libc::c_int) {
        let pid = PID.load(Ordering::Acquire);
        if pid > 0 {
            unsafe { libc::kill(pid as i32, sig) };
        }
    }

    unsafe extern "C" fn forward_signal_group(sig: libc::c_int) {
        let pgid = PGID.load(Ordering::Acquire);
        if pgid > 1 {
            unsafe { libc::kill(-(pgid as i32), sig) };
        } else {
            unsafe { forward_signal_process(sig) };
        }
    }

    PID.store(props.pid, Ordering::Release);
    PGID.store(props.pgid, Ordering::Release);

    for sig in [
        libc::SIGTERM,
        libc::SIGHUP,
        libc::SIGINT,
        libc::SIGCONT,
        libc::SIGTSTP,
    ] {
        unsafe { libc::signal(sig, forward_signal_group as _) };
    }

    // The main process is expected to setup SIGUSR* handler.
    // But child processes in the group is not ready, so we
    // only send SIGUSR* to the process, not the group.
    for sig in [libc::SIGUSR1, libc::SIGUSR2] {
        unsafe { libc::signal(sig, forward_signal_process as _) };
    }
}
