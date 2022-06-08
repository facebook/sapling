/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl prefetch-profile

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use clap::Parser;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::str;
use util::path::expand_path;

use edenfs_client::EdenFsInstance;
use edenfs_error::{Result, ResultExt};

use crate::{ExitCode, Subcommand};

#[derive(Parser, Debug)]
#[clap(name = "prefetch-profile")]
#[clap(about = "Create, manage, and use Prefetch Profiles. This command is \
    primarily for use in automation.")]
pub enum PrefetchCmd {
    #[clap(about = "Stop recording fetched file paths and save previously \
        collected fetched file paths in the output prefetch profile")]
    Finish {
        #[clap(long, parse(from_str = expand_path), help = "The output path to store the prefetch profile")]
        output_path: Option<PathBuf>,
    },
    #[clap(about = "Start recording fetched file paths.")]
    Record,
}

impl PrefetchCmd {
    async fn finish(
        &self,
        instance: EdenFsInstance,
        output_path: &Option<PathBuf>,
    ) -> Result<ExitCode> {
        let client = instance.connect(None).await?;
        let files = client.stopRecordingBackingStoreFetch().await.from_err()?;
        let out_path = match output_path {
            Some(p) => p.clone(),
            None => PathBuf::from(r"prefetch_profile.txt"),
        };
        let fetched_files = files
            .fetchedFilePaths
            .get("HgQueuedBackingStore")
            .ok_or_else(|| anyhow!("no Path vector found"))?;
        let mut out_file = File::create(out_path).context("unable to create output file")?;
        for path_bytes in fetched_files {
            out_file
                .write_all(path_bytes)
                .context("failed to write to output file")?;
            out_file
                .write_all(b"\n")
                .context("failed to write to output file")?;
        }
        Ok(0)
    }

    async fn record(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let client = instance.connect(None).await?;
        client.startRecordingBackingStoreFetch().await.from_err()?;
        Ok(0)
    }
}

#[async_trait]
impl Subcommand for PrefetchCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        match self {
            Self::Finish { output_path } => self.finish(instance, output_path).await,
            Self::Record {} => self.record(instance).await,
        }
    }
}
