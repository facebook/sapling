/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl redirect

use async_trait::async_trait;
use clap::Parser;
use std::path::PathBuf;
use util::path::expand_path;

use edenfs_client::EdenFsInstance;
use edenfs_error::Result;

use crate::ExitCode;
use crate::Subcommand;

#[derive(Parser, Debug)]
#[clap(name = "redirect")]
#[clap(about = "List and manipulate redirected paths")]
pub enum RedirectCmd {
    List {
        #[clap(long, parse(from_str = expand_path), help = "The EdenFS mount point path.")]
        mount: Option<PathBuf>,
        #[clap(long, help = "output in json rather than human readable text")]
        json: bool,
    },
}

impl RedirectCmd {
    async fn list(
        &self,
        instance: EdenFsInstance,
        mount: &Option<PathBuf>,
        json: bool,
    ) -> Result<ExitCode> {
        eprintln!("Rust `eden redirect list` is unimplemented...");
        Ok(-1)
    }
}

#[async_trait]
impl Subcommand for RedirectCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        match self {
            Self::List { mount, json } => self.list(instance, mount, *json).await,
        }
    }
}
