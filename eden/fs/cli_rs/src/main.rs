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
fn fallback() -> Result<()> {
    let binary = std::env::current_exe().context("unable to locate Python binary")?;
    let python_binary = binary
        .parent()
        .ok_or_else(|| anyhow!("unable to locate Python binary"))?
        .join("edenfsctl.real");
    let mut cmd = Command::new(python_binary);

    cmd.args(std::env::args().skip(1));

    #[cfg(windows)]
    {
        cmd.status()
            .with_context(|| format!("failed to execute: {:?}", cmd))?;
        Ok(())
    }

    #[cfg(unix)]
    {
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
