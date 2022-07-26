/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use std::process::Command;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;
use tracing_subscriber::filter::EnvFilter;

#[cfg(fbcode_build)]
use edenfs_telemetry::cli_usage::CliUsageSample;
#[cfg(fbcode_build)]
use edenfs_telemetry::send;
#[cfg(windows)]
use edenfs_utils::strip_unc_prefix;

#[cfg(windows)]
const PYTHON_CANDIDATES: &[&str] = &[
    r"c:\tools\fb-python\fb-python39",
    r"c:\tools\fb-python\fb-python38",
    r"c:\Python39",
    r"c:\Python38",
];

#[cfg(windows)]
fn find_python() -> Option<PathBuf> {
    for candidate in PYTHON_CANDIDATES.iter() {
        let candidate = Path::new(candidate);
        let python = candidate.join("python.exe");

        if candidate.exists() && python.exists() {
            tracing::debug!("Found Python runtime at {}", python.display());
            return Some(python);
        }
    }
    None
}

#[cfg(windows)]
fn execute_par(par: PathBuf) -> Result<Command> {
    let python = find_python().ok_or_else(|| {
        anyhow!(
            "Unable to find Python runtime. Paths tried:\n\n - {}",
            PYTHON_CANDIDATES.join("\n - ")
        )
    })?;
    let mut python = Command::new(python);
    python.arg(par);
    Ok(python)
}

fn python_fallback() -> Result<Command> {
    if let Ok(args) = std::env::var("EDENFSCTL_REAL") {
        // We might get a command starting with python.exe here instead of a simple path.
        let mut parts = args.split_ascii_whitespace();
        let binary = parts
            .next()
            .ok_or_else(|| anyhow!("invalid fallback environment variable: {:?}", args))?;
        let mut cmd = Command::new(binary);
        cmd.args(parts);
        return Ok(cmd);
    }

    let binary = std::env::current_exe().context("unable to retrieve path to the executable")?;
    let binary =
        std::fs::canonicalize(binary).context("unable to canonicalize path to the executable")?;
    #[cfg(windows)]
    let binary = strip_unc_prefix(binary);
    let libexec = binary.parent().ok_or_else(|| {
        anyhow!(
            "unable to retrieve parent directory to '{}'",
            binary.display()
        )
    })?;

    let executable = libexec.join(if cfg!(windows) {
        "edenfsctl.real.exe"
    } else {
        "edenfsctl.real"
    });
    tracing::debug!("trying {:?}", executable);
    if executable.exists() {
        return Ok(Command::new(executable));
    }

    // On Windows we are shipping the Python edenfsctl as PAR file that is not executable by itself
    #[cfg(windows)]
    {
        let par = libexec.join("edenfsctl.real.par");
        tracing::debug!("trying {:?}", par);

        if par.exists() {
            return execute_par(par);
        }
    }

    Err(anyhow!("unable to locate fallback binary"))
}

fn fallback() -> Result<i32> {
    if std::env::var("EDENFS_LOG").is_ok() {
        setup_logging();
    }

    let mut cmd = python_fallback()?;
    // skip arg0
    cmd.args(std::env::args().skip(1));

    // Users have PYTHONHOME and PYTHONPATH variables
    // that break the python version of edenfsctl since it will fail to
    // import modules. So, let's strip the PYTHONHOME and PYTHONPATH variables.
    cmd.env_remove("PYTHONHOME");
    cmd.env_remove("PYTHONPATH");

    tracing::debug!("Falling back to {:?}", cmd);

    // Create a subprocess to run Python edenfsctl
    let status = cmd
        .status()
        .with_context(|| format!("failed to execute: {:?}", cmd))?;
    Ok(status.code().unwrap_or(1))
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

fn rust_main(cmd: edenfs_commands::MainCommand) -> Result<i32> {
    if cmd.debug {
        setup_logging();
    }
    Ok(cmd.run()?)
}

/// This function takes care of the fallback logic, hijack supported subcommand
/// to Rust implementation and forward the rest to Python.
fn wrapper_main() -> Result<i32> {
    if std::env::var("EDENFSCTL_ONLY_RUST").is_ok() {
        let cmd = edenfs_commands::MainCommand::parse();
        rust_main(cmd)
    } else if std::env::var("EDENFSCTL_SKIP_RUST").is_ok() {
        fallback()
    } else {
        match edenfs_commands::MainCommand::try_parse() {
            Ok(cmd) => rust_main(cmd),
            // If we get a help message, we don't want to fallback to the Python version. The
            // help flag has been disabled for the main command and debug subcommand so they
            // will fallback correct while we still show help message for enabled commands
            // correctly.
            Err(e) if e.kind() == clap::ErrorKind::DisplayHelp => e.exit(),
            Err(_) => fallback(),
        }
    }
}

#[fbinit::main]
fn main(_fb: FacebookInit) -> Result<()> {
    #[cfg(fbcode_build)]
    let mut sample = CliUsageSample::build(_fb);

    let code = match wrapper_main() {
        Ok(code) => Ok(code),
        Err(e) => {
            #[cfg(fbcode_build)]
            sample.set_exception(&e);
            Err(e)
        }
    };

    #[cfg(fbcode_build)]
    {
        sample.set_exit_code(*code.as_ref().unwrap_or(&1));
        send(sample.builder);
    }

    match code {
        Ok(code) => std::process::exit(code),
        Err(e) => Err(e),
    }
}
