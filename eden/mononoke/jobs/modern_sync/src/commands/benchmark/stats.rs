/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::Write;
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;
use fb303_core_thriftclients::BaseService;
use fb303_core_thriftclients::make_BaseService_thriftclient;
use fb303_core_thriftclients::thriftclient::TransportType;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;

const CONN_TIMEOUT_MS: u32 = 1_000;
const RECV_TIMEOUT_MS: u32 = 1_000;

pub(crate) struct StatsBuilder {
    fb: FacebookInit,
    name: Option<String>,
    repo: String,
    fb303_port: u16,
    interval: Duration,
}

impl StatsBuilder {
    pub(crate) fn new(
        fb: FacebookInit,
        name: Option<String>,
        repo: String,
        fb303_port: u16,
        interval: Duration,
    ) -> Self {
        Self {
            fb,
            name,
            repo,
            fb303_port,
            interval,
        }
    }

    pub(crate) async fn build(&self) -> Result<Stats> {
        let fb303 = get_fb303_client(self.fb, self.fb303_port)?;

        let filename = {
            let pid = format!("{}", std::process::id());
            let benchmark = if let Some(name) = &self.name {
                format!("benchmark-{}", name)
            } else {
                "benchmark".to_string()
            };
            format!(
                "/tmp/{}",
                ["modern_sync", &benchmark, &pid, "csv"].join(".")
            )
        };

        let writer = {
            let stats_writer = Arc::new(File::create(filename)?);
            write_csv_header(&stats_writer).await?;
            stats_writer
        };

        Ok(Stats {
            repo: self.repo.clone(),
            interval: self.interval,

            start_time: SystemTime::now(),
            fb303,
            writer,
        })
    }
}

pub(crate) struct Stats {
    repo: String,
    interval: Duration,

    start_time: SystemTime,
    fb303: Arc<dyn BaseService + Send + Sync>,
    writer: Arc<File>,
}

impl Stats {
    pub(crate) fn run(&self) {
        mononoke::spawn_task({
            let source_repo_name = self.repo.clone();
            let interval = self.interval;
            let start_time = self.start_time;
            let fb303 = self.fb303.clone();
            let writer = self.writer.clone();

            async move {
                let mut interval = tokio::time::interval(interval);
                loop {
                    interval.tick().await;
                    _ = log_perf_stats(
                        fb303.clone(),
                        &source_repo_name,
                        start_time,
                        writer.clone(),
                    )
                    .await
                    .inspect_err(|e| tracing::warn!("Failed to get counters: {e:?}"));
                }
            }
        });
    }

    pub(crate) async fn finish(&self) {
        _ = log_perf_stats(
            self.fb303.clone(),
            &self.repo,
            self.start_time,
            self.writer.clone(),
        )
        .await
        .inspect_err(|e| tracing::warn!("Failed to get counters: {e:?}"));
    }
}

fn get_fb303_client(fb: FacebookInit, port: u16) -> Result<Arc<dyn BaseService + Sync + Send>> {
    let addr: SocketAddr = format!("[::]:{}", port).parse()?;
    make_BaseService_thriftclient!(
        fb,
        from_sock_addr = addr,
        with_transport_type = TransportType::Rocket,
        with_conn_timeout = CONN_TIMEOUT_MS,
        with_recv_timeout = RECV_TIMEOUT_MS,
        with_secure = true,
        with_persistent_header = ("caller_language", "rust"),
    )
    .with_context(|| format!("failed to create base thrift client from socket address {addr}"))
}

async fn log_perf_stats(
    fb303: Arc<dyn BaseService + Sync + Send>,
    repo_name: &str,
    start_time: SystemTime,
    stats_writer: Arc<File>,
) -> Result<()> {
    let regex = format!(
        "^mononoke\\.modern_sync\\.manager\\.changeset\\.{}\\.commits_synced\\.sum.*",
        repo_name
    );
    let sum_key = format!(
        "mononoke.modern_sync.manager.changeset.{}.commits_synced.sum",
        repo_name
    );
    let last60_key = format!(
        "mononoke.modern_sync.manager.changeset.{}.commits_synced.sum.60",
        repo_name
    );
    let last600_key = format!(
        "mononoke.modern_sync.manager.changeset.{}.commits_synced.sum.600",
        repo_name
    );
    let last3600_key = format!(
        "mononoke.modern_sync.manager.changeset.{}.commits_synced.sum.3600",
        repo_name
    );

    let counters = fb303
        .getRegexCounters(&regex)
        .await
        .with_context(|| "Failed to get counters")?;

    let sum = counters.get(&sum_key);
    let last60 = counters.get(&last60_key);
    let last600 = counters.get(&last600_key);
    let last3600 = counters.get(&last3600_key);

    tracing::info!(
        "Synced total={} speed={}/{}/{} (changesets/min last 60/600/3600) ({} {} {})",
        sum.map_or("?".to_string(), |v| v.to_string()),
        last60.map_or("?".to_string(), |v| v.to_string()),
        last600.map_or("?".to_string(), |v| (v / 10).to_string()),
        last3600.map_or("?".to_string(), |v| (v / 60).to_string()),
        last60.map_or("n/a".to_string(), |v| v.to_string()),
        last600.map_or("n/a".to_string(), |v| v.to_string()),
        last3600.map_or("n/a".to_string(), |v| v.to_string()),
    );

    if sum.is_some() {
        let now = std::time::SystemTime::now();
        writeln!(
            stats_writer.deref(),
            "{},{},{},{},{},{},{},{},{}",
            now.duration_since(UNIX_EPOCH)?.as_secs(),
            now.duration_since(start_time)?.as_secs(),
            sum.map_or("?".to_string(), |v| v.to_string()),
            last60.map_or("?".to_string(), |v| v.to_string()),
            last600.map_or("?".to_string(), |v| (v / 10).to_string()),
            last3600.map_or("?".to_string(), |v| (v / 60).to_string()),
            last60.map_or("n/a".to_string(), |v| v.to_string()),
            last600.map_or("n/a".to_string(), |v| v.to_string()),
            last3600.map_or("n/a".to_string(), |v| v.to_string()),
        )?;
    }

    Ok(())
}

async fn write_csv_header(stats_writer: &Arc<File>) -> Result<(), std::io::Error> {
    writeln!(
        stats_writer.deref(),
        "time,elapsed,sum,last60,last600,last3600,speed60,speed600,speed3600"
    )
}
