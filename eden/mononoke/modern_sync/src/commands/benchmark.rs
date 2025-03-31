/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
#[cfg(fbcode_build)]
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(fbcode_build)]
use std::time::Duration;

#[cfg(fbcode_build)]
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use clap::ValueEnum;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
#[cfg(fbcode_build)]
use cloned::cloned;
use context::CoreContext;
use context::SessionContainer;
#[cfg(fbcode_build)]
use fb303_core_thriftclients::make_BaseService_thriftclient;
#[cfg(fbcode_build)]
use fb303_core_thriftclients::thriftclient::TransportType;
#[cfg(fbcode_build)]
use fb303_core_thriftclients::BaseService;
#[cfg(fbcode_build)]
use fbinit::FacebookInit;
use metadata::Metadata;
#[cfg(fbcode_build)]
use mononoke_app::args::MonitoringArgs;
use mononoke_app::MononokeApp;
#[cfg(fbcode_build)]
use mononoke_macros::mononoke;
use mutable_counters::MutableCounters;
use slog::info;
#[cfg(fbcode_build)]
use slog::warn;
#[cfg(fbcode_build)]
use slog::Logger;

use crate::commands::sync_loop::CHUNK_SIZE_DEFAULT;
use crate::sender::edenapi::EdenapiSender;
use crate::sender::edenapi::FilterEdenapiSender;
use crate::sender::edenapi::MethodFilter;
use crate::sender::edenapi::NoopEdenapiSender;
use crate::sync::get_unsharded_repo_args;
use crate::sync::ExecutionType;
use crate::ModernSyncArgs;

#[cfg(fbcode_build)]
const CONN_TIMEOUT_MS: u32 = 1_000;
#[cfg(fbcode_build)]
const RECV_TIMEOUT_MS: u32 = 1_000;

#[derive(ValueEnum, Default, Clone)]
enum BenchmarkMode {
    #[default]
    Noop,
    UploadContents,
}

/// Replays bookmark's moves
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(long, default_value_t, value_enum)]
    mode: BenchmarkMode,

    #[clap(long, help = "Chunk size for the sync [default: 1000]")]
    chunk_size: Option<u64>,

    #[clap(
        long,
        default_value = "60",
        help = "How often to report stats, in seconds"
    )]
    stat_interval: u64,
}

#[derive(Clone, Default)]
struct MemoryMutableCounters {
    counters: Arc<std::sync::RwLock<std::collections::HashMap<String, i64>>>,
}

impl MemoryMutableCounters {
    pub fn new() -> Self {
        Self {
            counters: Arc::new(std::sync::RwLock::new(Default::default())),
        }
    }
}

#[async_trait]
impl MutableCounters for MemoryMutableCounters {
    async fn get_counter(&self, _ctx: &CoreContext, name: &str) -> Result<Option<i64>> {
        Ok(self.counters.read().unwrap().get(name).cloned())
    }

    async fn get_maybe_stale_counter(&self, ctx: &CoreContext, name: &str) -> Result<Option<i64>> {
        self.get_counter(ctx, name).await
    }

    async fn set_counter(
        &self,
        _ctx: &CoreContext,
        name: &str,
        value: i64,
        _prev_value: Option<i64>,
    ) -> Result<bool> {
        self.counters
            .write()
            .unwrap()
            .insert(name.to_string(), value);
        Ok(true)
    }

    async fn get_all_counters(&self, _ctx: &CoreContext) -> Result<Vec<(String, i64)>> {
        Ok(self
            .counters
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect())
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let app = Arc::new(app);
    let app_args = &app.args::<ModernSyncArgs>()?;
    let (source_repo_args, source_repo_name, dest_repo_name) =
        get_unsharded_repo_args(app.clone(), app_args).await?;
    let ctx = new_context(&app);
    let logger = app.logger().clone();

    let benchmark_mode = args.mode;
    let mc = MemoryMutableCounters::new();

    #[cfg(fbcode_build)]
    let fb303 = {
        let stat_interval = Duration::from_secs(args.stat_interval);
        let port = app.args::<MonitoringArgs>()?.fb303_thrift_port.unwrap();
        let fb303 = get_fb303_client(app.fb, port).unwrap();

        mononoke::spawn_task({
            cloned!(fb303, source_repo_name, logger);
            async move {
                let mut interval = tokio::time::interval(stat_interval);
                loop {
                    interval.tick().await;
                    _ = log_perf_stats(fb303.clone(), &source_repo_name, &logger)
                        .await
                        .inspect_err(|e| warn!(logger, "Failed to get counters: {e:?}"));
                }
            }
        });

        fb303
    };

    let now = std::time::Instant::now();
    crate::sync::sync(
        app,
        Some(0),
        source_repo_args,
        dest_repo_name.clone(),
        ExecutionType::SyncOnce,
        false,
        args.chunk_size.clone().unwrap_or(CHUNK_SIZE_DEFAULT),
        PathBuf::from(""),
        Some(Box::new(move |sender| {
            let sender: Arc<dyn EdenapiSender + Sync + Send> = match benchmark_mode {
                BenchmarkMode::Noop => Arc::new(NoopEdenapiSender::default()),
                BenchmarkMode::UploadContents => {
                    let allowed = HashMap::from([(MethodFilter::UploadContents, true)]);
                    Arc::new(FilterEdenapiSender::new(sender, allowed))
                }
            };
            sender
        })),
        Some(Arc::new(mc.clone())),
    )
    .await?;
    let elapsed = now.elapsed();

    #[cfg(fbcode_build)]
    {
        _ = log_perf_stats(fb303, &source_repo_name, &logger)
            .await
            .inspect_err(|e| warn!(logger, "Failed to get counters: {e:?}"));
    }

    info!(
        logger,
        "Benchmark: Sync {} to {:?} took {}ms",
        elapsed.as_millis(),
        &source_repo_name,
        dest_repo_name,
    );

    info!(logger, "Counters:");
    let mut counters = mc.get_all_counters(&ctx).await?;
    counters.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in counters {
        info!(logger, "{}={}", k, v);
    }

    Ok(())
}

fn new_context(app: &MononokeApp) -> CoreContext {
    let mut metadata = Metadata::default();
    metadata.add_client_info(ClientInfo::default_with_entry_point(
        ClientEntryPoint::ModernSync,
    ));

    let scuba = app.environment().scuba_sample_builder.clone();
    let session_container = SessionContainer::builder(app.fb)
        .metadata(Arc::new(metadata))
        .build();

    session_container.new_context(app.logger().clone(), scuba)
}

#[cfg(fbcode_build)]
fn get_fb303_client(fb: FacebookInit, port: i32) -> Result<Arc<dyn BaseService + Sync + Send>> {
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

#[cfg(fbcode_build)]
async fn log_perf_stats(
    fb303: Arc<dyn BaseService + Sync + Send>,
    repo_name: &str,
    logger: &Logger,
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

    let sum = counters
        .get(&sum_key)
        .map_or("n/a".to_string(), |v| v.to_string());
    let last60 = counters.get(&last60_key);
    let last600 = counters.get(&last600_key);
    let last3600 = counters.get(&last3600_key);

    info!(
        logger,
        "Synced total={} speed={}/{}/{} (changesets/min last 60/600/3600) ({} {} {})",
        sum,
        last60.map_or("?".to_string(), |v| v.to_string()),
        last600.map_or("?".to_string(), |v| (v / 10).to_string()),
        last3600.map_or("?".to_string(), |v| (v / 60).to_string()),
        last60.map_or("n/a".to_string(), |v| v.to_string()),
        last600.map_or("n/a".to_string(), |v| v.to_string()),
        last3600.map_or("n/a".to_string(), |v| v.to_string()),
    );

    Ok(())
}
