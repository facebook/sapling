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
use structopt::StructOpt;

mod opt;

use crate::opt::Opt;

fn process_opt(_opt: Opt) -> Result<()> {
    Ok(())
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
        Ok(cmd)
    } else {
        let binary = std::env::current_exe().context("unable to locate Python binary")?;
        let python_binary = binary
            .parent()
            .ok_or_else(|| anyhow!("unable to locate Python binary"))?
            .join("edenfsctl.real");
        Ok(Command::new(python_binary))
    }
}

fn fallback() -> Result<()> {
    let mut cmd = python_fallback()?;
    // skip arg0
    cmd.args(std::env::args().skip(1));

    #[cfg(windows)]
    {
        // Windows doesn't have exec, so we have to open a subprocess
        cmd.status()
            .with_context(|| format!("failed to execute: {:?}", cmd))?;
        Ok(())
    }

    #[cfg(unix)]
    {
        // `.exec()` should take over the process, if we ever get to return this Err, then it means
        // exec has failed, hence an error.
        Err(cmd.exec()).with_context(|| format!("failed to execute {:?}", cmd))
    }
}

fn main() -> Result<()> {
    if std::env::var("EDENFSCTL_SKIP_RUST").is_ok() {
        fallback()
    } else if let Ok(opt) = Opt::from_args_safe() {
        process_opt(opt)
    } else {
        fallback()
    }
}
