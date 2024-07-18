/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Git Pushrebase
//!
//! This is a binary that will replace `git push` calls for Git repos that are
//! synced to a Mononoke large repo.
//!
//! When the source of truth is still in the Git repo, this binary will
//! act as a wrapper for `git push`, supporting only a subset of arguments
//! that will also be supported after the source of truth is changed.

use std::process::Command;

use anyhow::Context;
use anyhow::Ok;
use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;

#[derive(Debug, Parser)]
#[clap(about = "git push replacement for Git repos synced to a Mononoke large repo")]
pub struct GitPushrebaseArgs {
    #[clap(help = "Repository to push to")]
    pub repository: Option<String>,

    #[clap(help = "References (e.g. branch) to be pushed")]
    pub refspec: Option<String>,
}

#[fbinit::main]
fn main(_fb: FacebookInit) -> Result<()> {
    let args = GitPushrebaseArgs::parse();

    // Run `git push` with provided args
    let mut command = Command::new("git");
    command.arg("push");
    if let Some(repository) = args.repository {
        command.arg(repository);
    }
    if let Some(refspec) = args.refspec {
        command.arg(refspec);
    }

    let output = command
        .output()
        .context("Failed to execute git push command")?;

    // Pipe stderr
    let error_message = String::from_utf8_lossy(&output.stderr);
    eprintln!("{}", error_message);

    // Pipe stdout
    let result = String::from_utf8_lossy(&output.stdout);
    println!("{}", result);

    Ok(())
}
