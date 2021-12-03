/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use structopt::{clap, StructOpt};
use tracing_subscriber::filter::EnvFilter;

fn python_fallback() -> Result<Command> {
    if let Ok(args) = std::env::var("EDENFSCTL_REAL") {
        // We might get a command starting with python.exe here instead of a simple path.
        let mut parts = args.split_ascii_whitespace();
        let binary = parts
            .next()
            .ok_or_else(|| anyhow!("invalid fallback environment variable: {:?}", args))?;
        let mut cmd = Command::new(binary);
        cmd.args(parts);
        Ok(cmd)
    } else {
        let binary = std::env::current_exe().context("unable to locate Python binary")?;
        let python_binary = binary
            .parent()
            .ok_or_else(|| anyhow!("unable to locate Python binary"))?
            .join(if cfg!(windows) {
                "edenfsctl.real.exe"
            } else {
                "edenfsctl.real"
            });
        Ok(Command::new(python_binary))
    }
}

fn fallback() -> Result<()> {
    let mut cmd = python_fallback()?;
    // skip arg0
    cmd.args(std::env::args().skip(1));

    // Users have PYTHONHOME and PYTHONPATH variables
    // that break the python version of edenfsctl since it will fail to
    // import modules. So, let's strip the PYTHONHOME and PYTHONPATH variables.
    cmd.env_remove("PYTHONHOME");
    cmd.env_remove("PYTHONPATH");

    #[cfg(windows)]
    {
        // Windows doesn't have exec, so we have to open a subprocess
        let status = cmd
            .status()
            .with_context(|| format!("failed to execute: {:?}", cmd))?;
        std::process::exit(
            status
                .code()
                .expect(format!("{:?} didn't set an error code", cmd).as_str()),
        )
    }

    #[cfg(unix)]
    {
        // `.exec()` should take over the process, if we ever get to return this Err, then it means
        // exec has failed, hence an error.
        Err(cmd.exec()).with_context(|| format!("failed to execute {:?}", cmd))
    }
}

/// Setup tracing logging. If we are in development mode, we use the fancier logger, otherwise a
/// simple logger for production use. Logs will be printined to stderr when `--debug` flag is
/// passed.
fn setup_logging() {
    let subscriber = tracing_subscriber::fmt();
    #[cfg(debug_assertions)]
    let subscriber = subscriber.pretty();
    let subscriber = subscriber.with_env_filter(EnvFilter::from_env("EDENFS_LOG"));

    if let Err(e) = subscriber.try_init() {
        eprintln!(
            "Unable to initialize logger. Logging will be disabled. Cause: {:?}",
            e
        );
    }
}

fn rust_main(cmd: edenfs_commands::MainCommand) -> Result<()> {
    if cmd.debug {
        setup_logging();
    }
    match cmd.run() {
        Ok(code) => std::process::exit(code),
        Err(e) => Err(e.into()),
    }
}

fn main() -> Result<()> {
    if std::env::var("EDENFSCTL_ONLY_RUST").is_ok() {
        let cmd = edenfs_commands::MainCommand::from_args();
        rust_main(cmd)
    } else if std::env::var("EDENFSCTL_SKIP_RUST").is_ok() {
        fallback()
    } else {
        match edenfs_commands::MainCommand::from_args_safe() {
            Ok(cmd) => rust_main(cmd),
            Err(e) if e.kind == clap::ErrorKind::HelpDisplayed => {
                // If we get a help message, we don't want to fallback to the Python version. The
                // help flag has been disabled for the main command and debug subcommand so they
                // will fallback correct while we still show help message for enabled commands
                // correctly.
                e.exit();
            }
            Err(_) => fallback(),
        }
    }
}
