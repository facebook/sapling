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
use edenfs_client::counters::CrawlingScore;
use edenfs_client::counters::TelemetryCounters;
use serde_json::Value;

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
    #[clap(about = "Get counters from EdenFS daemon start (no delta calculation)")]
    FromStart(FromStartCmd),
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

    #[clap(long, help = "Print the entire result")]
    full: bool,
}

#[derive(Parser, Debug)]
pub struct FromStartCmd {
    #[clap(long, help = "Print only the crawling score")]
    score: bool,

    #[clap(long, help = "Print the entire result")]
    full: bool,
}

#[async_trait]
impl Subcommand for CountersCmd {
    async fn run(&self) -> Result<ExitCode> {
        match self {
            CountersCmd::StartRecording(cmd) => cmd.run().await,
            CountersCmd::Finalize(cmd) => cmd.run().await,
            CountersCmd::FromStart(cmd) => cmd.run().await,
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

/// Recursively filter out zero values and empty objects/arrays from a JSON value
fn filter_zero_values(value: Value) -> Option<Value> {
    match value {
        Value::Object(obj) => {
            let filtered: serde_json::Map<String, Value> = obj
                .into_iter()
                .filter_map(|(k, v)| filter_zero_values(v).map(|filtered_v| (k, filtered_v)))
                .collect();

            if filtered.is_empty() {
                None
            } else {
                Some(Value::Object(filtered))
            }
        }
        Value::Array(arr) => {
            let filtered: Vec<Value> = arr.into_iter().filter_map(filter_zero_values).collect();

            if filtered.is_empty() {
                None
            } else {
                Some(Value::Array(filtered))
            }
        }
        Value::Number(n) => {
            if n.is_i64() && n.as_i64() == Some(0) {
                None
            } else {
                Some(Value::Number(n))
            }
        }
        Value::String(s) => {
            if s.is_empty() {
                None
            } else {
                Some(Value::String(s))
            }
        }
        Value::Bool(b) => Some(Value::Bool(b)),
        Value::Null => None,
    }
}

/// Convert TelemetryCounters to a pretty-printed JSON string with zero values filtered out
fn to_filtered_json_pretty(counters: &TelemetryCounters) -> Result<String> {
    // First convert to a JSON value
    let json_value: Value = serde_json::to_value(counters)?;

    // Filter out zero values
    let filtered_value =
        filter_zero_values(json_value).unwrap_or(Value::Object(serde_json::Map::new()));

    // Convert back to a pretty-printed string
    let pretty = serde_json::to_string_pretty(&filtered_value)?;

    Ok(pretty)
}

/// Convert CrawlingScore to a pretty-printed JSON string with zero values filtered out
fn crawling_score_to_filtered_json_pretty(score: &CrawlingScore) -> Result<String> {
    // First convert to a JSON value
    let json_value: Value = serde_json::to_value(score)?;

    // Filter out zero values
    let filtered_value =
        filter_zero_values(json_value).unwrap_or(Value::Object(serde_json::Map::new()));

    // Convert back to a pretty-printed string
    let pretty = serde_json::to_string_pretty(&filtered_value)?;

    Ok(pretty)
}

#[async_trait]
impl Subcommand for FinalizeCmd {
    async fn run(&self) -> Result<ExitCode> {
        if self.score {
            let score = self
                .get_telemetry_counters_delta_from_snapshot(&self.snapshot_path)
                .await?
                .get_crawling_score();
            if self.full {
                println!("{}", score.to_json_pretty()?);
            } else {
                println!("{}", crawling_score_to_filtered_json_pretty(&score)?);
            };
        } else {
            let counters = self
                .get_telemetry_counters_delta_from_snapshot(&self.snapshot_path)
                .await?;
            if self.full {
                println!("{}", counters.to_json_pretty()?);
            } else {
                println!("{}", to_filtered_json_pretty(&counters)?);
            }
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

#[async_trait]
impl Subcommand for FromStartCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();
        if self.score {
            let counters = client.get_telemetry_counters().await?;
            let score = counters.get_crawling_score();
            if self.full {
                println!("{}", score.to_json_pretty()?);
            } else {
                println!("{}", crawling_score_to_filtered_json_pretty(&score)?);
            }
        } else {
            let counters = client.get_telemetry_counters().await?;
            if self.full {
                println!("{}", counters.to_json_pretty()?);
            } else {
                println!("{}", to_filtered_json_pretty(&counters)?);
            }
        }
        Ok(0)
    }
}
