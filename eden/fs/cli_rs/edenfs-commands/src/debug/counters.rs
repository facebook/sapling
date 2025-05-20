/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl debug counters

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::counters::TelemetryCounters;

use crate::ExitCode;
use crate::Subcommand;
use crate::get_edenfs_instance;

const DEFAULT_SNAPSHOT_PATH: &str = "/tmp/eden_counters_snapshot.json";

#[derive(Parser, Debug)]
#[clap(about = "Commands for working with EdenFS telemetry counters")]
pub enum CountersCmd {
    #[clap(about = "Start recording counters by creating an initial snapshot")]
    StartRecording(StartRecordingCmd),
    #[clap(about = "Finalize recording and print the delta as JSON")]
    Finalize(FinalizeCmd),
}

#[derive(Parser, Debug)]
pub struct StartRecordingCmd {
    #[clap(
        long,
        help = "Path to save the snapshot file",
        default_value = DEFAULT_SNAPSHOT_PATH
    )]
    snapshot_path: PathBuf,
}

#[derive(Parser, Debug)]
pub struct FinalizeCmd {
    #[clap(
        long,
        help = "Path to the snapshot file",
        default_value = DEFAULT_SNAPSHOT_PATH
    )]
    snapshot_path: PathBuf,

    #[clap(long, help = "Print only the crawling score")]
    score: bool,
}

#[async_trait]
impl Subcommand for CountersCmd {
    async fn run(&self) -> Result<ExitCode> {
        match self {
            CountersCmd::StartRecording(cmd) => cmd.run().await,
            CountersCmd::Finalize(cmd) => cmd.run().await,
        }
    }
}

impl StartRecordingCmd {
    pub async fn init_counters_on_disk<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();
        let counters = client.get_telemetry_counters().await?;
        let json = counters.to_json_pretty()?;
        fs::write(path, json)?;
        Ok(())
    }
}

#[async_trait]
impl Subcommand for StartRecordingCmd {
    async fn run(&self) -> Result<ExitCode> {
        self.init_counters_on_disk(&self.snapshot_path).await?;
        println!(
            "Counters snapshot saved to {}",
            self.snapshot_path.display()
        );
        Ok(0)
    }
}

#[async_trait]
impl Subcommand for FinalizeCmd {
    async fn run(&self) -> Result<ExitCode> {
        if self.score {
            let score = self
                .get_telemetry_counters_delta_from_snapshot(&self.snapshot_path)
                .await?
                .get_crawling_score();
            let json = score.to_json_pretty()?;
            println!("{}", json);
        } else {
            let counters = self
                .get_telemetry_counters_delta_from_snapshot(&self.snapshot_path)
                .await?;
            let json = counters.to_json_pretty()?;
            println!("{}", json);
        }
        Ok(0)
    }
}

impl FinalizeCmd {
    pub async fn get_telemetry_counters_delta_from_snapshot<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<TelemetryCounters> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();
        // Fetch the current counters
        let current_counters = client.get_telemetry_counters().await?;
        // Read the snapshot file
        let snapshot_json = fs::read_to_string(&path)?;
        // Parse the snapshot
        let snapshot_counters = TelemetryCounters::from_json(&snapshot_json)?;
        // Remove the snapshot file
        fs::remove_file(&path)?;
        // Calculate the delta
        Ok(current_counters - snapshot_counters)
    }
}
