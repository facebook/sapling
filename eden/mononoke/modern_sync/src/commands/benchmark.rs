/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use context::SessionContainer;
use metadata::Metadata;
use mononoke_app::MononokeApp;
use mutable_counters::MutableCounters;
use slog::info;

use crate::commands::sync_loop::CHUNK_SIZE_DEFAULT;
use crate::sync::get_unsharded_repo_args;
use crate::sync::ExecutionType;
use crate::ModernSyncArgs;

/// Replays bookmark's moves
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(long, help = "Chunk size for the sync [default: 1000]")]
    chunk_size: Option<u64>,
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
            .insert(name.to_string(), value)
            .unwrap();
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

    let mc = MemoryMutableCounters::new();

    let now = std::time::Instant::now();
    crate::sync::sync(
        app,
        Some(0),
        source_repo_args.clone(),
        dest_repo_name.clone(),
        ExecutionType::SyncOnce,
        false,
        args.chunk_size.clone().unwrap_or(CHUNK_SIZE_DEFAULT),
        PathBuf::from(""),
        true,
        Some(Arc::new(mc.clone())),
    )
    .await?;
    let elapsed = now.elapsed();

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
