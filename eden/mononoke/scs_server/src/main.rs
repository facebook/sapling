/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "2097152"]

use std::fs::File;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use clap::Parser;
use cloned::cloned;
use cmdlib_logging::ScribeLoggingArgs;
use connection_security_checker::ConnectionSecurityChecker;
use environment::WarmBookmarksCacheDerivedData;
use executor_lib::args::ShardedExecutorArgs;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use fb303_core::server::make_BaseService_server;
use fbinit::FacebookInit;
use megarepo_api::MegarepoApi;
use mononoke_api::repo::Repo;
use mononoke_api::CoreContext;
use mononoke_app::args::HooksAppExtension;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::args::ShutdownTimeoutArgs;
use mononoke_app::MononokeAppBuilder;
use mononoke_app::MononokeReposManager;
use panichandler::Fate;
use permission_checker::DefaultAclProvider;
use slog::info;
use source_control::server::make_SourceControlService_server;
use srserver::service_framework::BuildModule;
use srserver::service_framework::ContextPropModule;
use srserver::service_framework::Fb303Module;
use srserver::service_framework::ProfileModule;
use srserver::service_framework::ServiceFramework;
use srserver::service_framework::ThriftStatsModule;
use srserver::ThriftServer;
use srserver::ThriftServerBuilder;
use tokio::task;

mod commit_id;
mod errors;
mod facebook;
mod from_request;
mod history;
mod into_response;
mod metadata;
mod methods;
mod monitoring;
mod scuba_common;
mod scuba_params;
mod scuba_response;
mod source_control_impl;
mod specifiers;

const SERVICE_NAME: &str = "mononoke_scs_server";
const SM_CLEANUP_TIMEOUT_SECS: u64 = 60;

/// Mononoke Source Control Service Server
#[derive(Parser)]
struct ScsServerArgs {
    #[clap(flatten)]
    shutdown_timeout_args: ShutdownTimeoutArgs,
    #[clap(flatten)]
    scribe_logging_args: ScribeLoggingArgs,
    /// Thrift host
    #[clap(long, short = 'H', default_value = "::")]
    host: String,
    /// Thrift port
    #[clap(long, short = 'p', default_value_t = 8367)]
    port: u16,
    /// Path for file in which to write the bound tcp address in rust std::net::SocketAddr format
    #[clap(long)]
    bound_address_file: Option<String>,
    #[clap(flatten)]
    sharded_executor_args: ShardedExecutorArgs,
}

/// Struct representing the Source Control Service process when sharding by
/// repo.
pub struct ScsServerProcess {
    repos_mgr: Arc<MononokeReposManager<Repo>>,
}

impl ScsServerProcess {
    fn new(repos_mgr: MononokeReposManager<Repo>) -> Self {
        let repos_mgr = Arc::new(repos_mgr);
        Self { repos_mgr }
    }
}

#[async_trait]
impl RepoShardedProcess for ScsServerProcess {
    async fn setup(&self, repo_name: &str) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let logger = self.repos_mgr.repo_logger(repo_name);
        info!(&logger, "Setting up repo {} in SCS service", repo_name);
        // Check if the input repo is already initialized. This can happen if the repo is a
        // shallow-sharded repo, in which case it would already be initialized during service startup.
        if self.repos_mgr.repos().get_by_name(repo_name).is_none() {
            // The input repo is a deep-sharded repo, so it needs to be added now.
            self.repos_mgr.add_repo(repo_name).await.with_context(|| {
                format!("Failure in setting up repo {} in SCS service", repo_name)
            })?;
            info!(&logger, "Completed repo {} setup in SCS service", repo_name);
        } else {
            info!(
                &logger,
                "Repo {} is already setup in SCS service", repo_name
            );
        }
        Ok(Arc::new(ScsServerProcessExecutor {
            repo_name: repo_name.to_string(),
            repos_mgr: self.repos_mgr.clone(),
        }))
    }
}

/// Struct representing the execution of the source control service for a
/// particular repo when sharding by repo.
pub struct ScsServerProcessExecutor {
    repo_name: String,
    repos_mgr: Arc<MononokeReposManager<Repo>>,
}

#[async_trait]
impl RepoShardedProcessExecutor for ScsServerProcessExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        info!(
            self.repos_mgr.logger(),
            "Serving repo {} in SCS service", &self.repo_name,
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        let config = self
            .repos_mgr
            .repo_config(&self.repo_name)
            .with_context(|| {
                format!(
                    "Failure in stopping repo {}. The config for repo doesn't exist",
                    &self.repo_name
                )
            })?;
        // Check if the current repo is a deep-sharded or shallow-sharded repo. If the
        // repo is deep-sharded, then remove it since SM wants some other host to serve it.
        // If repo is shallow-sharded, then keep it since regardless of SM sharding, shallow
        // sharded repos need to be present on each host.
        if config.deep_sharded {
            self.repos_mgr.remove_repo(&self.repo_name);
            info!(
                self.repos_mgr.logger(),
                "No longer serving repo {} in SCS service.", &self.repo_name,
            );
        } else {
            info!(
                self.repos_mgr.logger(),
                "Continuing serving repo {} in SCS service because it's shallow-sharded.",
                &self.repo_name,
            );
        }
        Ok(())
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    panichandler::set_panichandler(Fate::Abort);

    let app = MononokeAppBuilder::new(fb)
        .with_warm_bookmarks_cache(WarmBookmarksCacheDerivedData::AllKinds)
        .with_app_extension(HooksAppExtension {})
        .with_app_extension(RepoFilterAppExtension {})
        .build::<ScsServerArgs>()?;

    let args: ScsServerArgs = app.args()?;

    let logger = app.logger().clone();
    let runtime = app.runtime();

    let exec = runtime.clone();
    let env = app.environment();

    let scuba_builder = env.scuba_sample_builder.clone();

    let repos_mgr = runtime.block_on(app.open_managed_repos())?;
    let mononoke = Arc::new(repos_mgr.make_mononoke_api()?);
    let megarepo_api = Arc::new(runtime.block_on(MegarepoApi::new(&app, mononoke.clone()))?);

    let will_exit = Arc::new(AtomicBool::new(false));

    // Initialize the FB303 Thrift stack.

    let fb303_base = {
        cloned!(will_exit);
        move |proto| {
            make_BaseService_server(proto, facebook::BaseServiceImpl::new(will_exit.clone()))
        }
    };
    let acl_provider = DefaultAclProvider::new(fb);
    let security_checker = runtime.block_on(ConnectionSecurityChecker::new(
        acl_provider.as_ref(),
        &app.repo_configs().common,
    ))?;
    let source_control_server = source_control_impl::SourceControlServiceImpl::new(
        fb,
        mononoke.clone(),
        megarepo_api,
        logger.clone(),
        scuba_builder,
        args.scribe_logging_args.get_scribe(fb)?,
        security_checker,
        &app.repo_configs().common,
    );
    let service = {
        move |proto| {
            make_SourceControlService_server(
                proto,
                source_control_server.thrift_server(),
                fb303_base.clone(),
            )
        }
    };
    let monitoring_forever = {
        let monitoring_ctx = CoreContext::new_with_logger(fb, logger.clone());
        monitoring::monitoring_stats_submitter(monitoring_ctx, mononoke)
    };
    runtime.spawn(monitoring_forever);

    let thrift: ThriftServer = ThriftServerBuilder::new(fb)
        .with_name(SERVICE_NAME)
        .expect("failed to set name")
        .with_address(&args.host, args.port, false)?
        .with_tls()
        .expect("failed to enable TLS")
        .with_cancel_if_client_disconnected()
        .with_metadata(metadata::create_metadata())
        .with_factory(exec, move || service)
        .build();

    let mut service_framework = ServiceFramework::from_server(SERVICE_NAME, thrift)
        .context("Failed to create service framework server")?;

    service_framework.add_module(BuildModule)?;
    service_framework.add_module(ThriftStatsModule)?;
    service_framework.add_module(Fb303Module)?;
    service_framework.add_module(ProfileModule)?;
    service_framework.add_module(ContextPropModule)?;

    service_framework
        .serve_background()
        .expect("failed to start thrift service");

    let bound_addr = format!(
        "{}:{}",
        &args.host,
        service_framework.get_address()?.get_port()?
    );
    info!(logger, "Listening on {}", bound_addr);

    // Write out the bound address if requested, this is helpful in tests when using automatic binding with :0
    if let Some(bound_addr_path) = args.bound_address_file {
        let mut writer = File::create(bound_addr_path)?;
        writer.write_all(bound_addr.as_bytes())?;
        writer.write_all(b"\n")?;
    }

    if let Some(mut executor) = args.sharded_executor_args.build_executor(
        fb,
        runtime.clone(),
        app.logger(),
        || Arc::new(ScsServerProcess::new(repos_mgr)),
        false, // disable shard (repo) level healing
        SM_CLEANUP_TIMEOUT_SECS,
    )? {
        // The Sharded Process Executor needs to branch off and execute
        // on its own dedicated task spawned off the common tokio runtime.
        runtime.spawn({
            let logger = logger.clone();
            {
                cloned!(will_exit);
                async move { executor.block_and_execute(&logger, will_exit).await }
            }
        });
    }

    // Monitoring is provided by the `Fb303Module`, but we must still start
    // stats aggregation.
    app.start_stats_aggregation()?;

    app.wait_until_terminated(
        move || will_exit.store(true, Ordering::Relaxed),
        args.shutdown_timeout_args.shutdown_grace_period,
        async {
            // Note that async blocks are lazy, so this isn't called until first poll
            let _ = task::spawn_blocking(move || {
                // Calling `stop` blocks until the service has completed all requests.
                service_framework.stop();
            })
            .await;
        },
        args.shutdown_timeout_args.shutdown_timeout,
    )?;

    info!(logger, "Exiting...");
    Ok(())
}
