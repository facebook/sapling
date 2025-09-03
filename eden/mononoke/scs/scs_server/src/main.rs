/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "2097152"]

use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use clap::Parser;
use clap::ValueEnum;
use clientinfo::ClientEntryPoint;
use cloned::cloned;
use cmdlib_logging::ScribeLoggingArgs;
use connection_security_checker::ConnectionSecurityChecker;
use environment::BookmarkCacheDerivedData;
use environment::BookmarkCacheKind;
use environment::BookmarkCacheOptions;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use executor_lib::args::ShardedExecutorArgs;
use factory_group::FactoryGroup;
use fb303_core_services::make_BaseService_server;
use fbinit::FacebookInit;
use git_source_of_truth::GitSourceOfTruthConfig;
use git_source_of_truth::SqlGitSourceOfTruthConfigBuilder;
use megarepo_api::MegarepoApi;
use metaconfig_types::ShardedService;
use mononoke_api::CoreContext;
use mononoke_api::repo::Repo;
use mononoke_app::MononokeAppBuilder;
use mononoke_app::MononokeReposManager;
use mononoke_app::args::HooksAppExtension;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::args::ShutdownTimeoutArgs;
use mononoke_app::args::WarmBookmarksCacheExtension;
use mysql_client::ConnectionOptionsBuilder;
use mysql_client::ConnectionPoolOptionsBuilder;
use panichandler::Fate;
use scs_methods::source_control_impl::SourceControlServiceImpl;
use sharding_ext::RepoShard;
use slog::info;
use source_control_services::make_SourceControlService_server;
use sql_construct::SqlConstruct;
use sql_storage::Destination;
use sql_storage::XdbFactory;
use srserver::ThriftExecutor;
use srserver::ThriftServer;
use srserver::ThriftServerBuilder;
use srserver::ThriftStreamExecutor;
use srserver::service_framework::BuildModule;
use srserver::service_framework::ContextPropModule;
use srserver::service_framework::Fb303Module;
use srserver::service_framework::ProfileModule;
use srserver::service_framework::ServiceFramework;
use srserver::service_framework::ThriftStatsModule;
use thrift_factory::ThriftFactoryBuilder;
use tokio::task;

mod facebook;
mod metadata;
mod monitoring;

const SERVICE_NAME: &str = "mononoke_scs_server";
const MONONOKE_PRODUCTION_SHARD_NAME: &str = "xdb.mononoke_production";
const SM_CLEANUP_TIMEOUT_SECS: u64 = 60;
const NUM_PRIORITY_QUEUES: usize = 2;

/// Mononoke Source Control Service Server
#[derive(Parser)]
struct ScsServerArgs {
    #[clap(flatten)]
    shutdown_timeout_args: ShutdownTimeoutArgs,
    #[clap(flatten)]
    scribe_logging_args: ScribeLoggingArgs,
    /// Enable trace logging of dependencies
    #[clap(long, default_value = "false")]
    trace: bool,
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
    /// Max memory to use for the thrift server
    #[clap(long)]
    max_memory: Option<usize>,
    /// Thrift server mode;
    #[clap(long, value_enum, default_value_t = ThriftServerMode::Default)]
    thift_server_mode: ThriftServerMode,
    /// Thrift queue size
    #[clap(long, default_value = "0")]
    thrift_queue_size: usize,
    /// Thrift queue timeout in milliseconds
    #[clap(long, default_value = "500")]
    thrift_queue_timeout: u64,
    /// Number of Thrift workers
    #[clap(long, default_value = "1000")]
    thrift_workers_num: usize,
    /// Number of Thrift workers for fast methods
    #[clap(long, default_value = "1000")]
    thrift_workers_num_fast: usize,
    /// Number of Thrift workers for slow methods
    #[clap(long, default_value = "5")]
    thrift_workers_num_slow: usize,
    /// Some long-running requests are processed asynchronously by default. This flag disables that behavior; requests will fail.
    #[clap(long, default_value = "false")]
    disable_async_requests: bool,
    /// Sets the threshold for watchdog logging of top-level SCS methods. As a rule of thumb this should the same or lower than thrift_queue_timeout.
    #[clap(long, default_value = "500")]
    watchdog_method_max_poll: u64,
    /// Load all repos in the tier at startup, even if they are configured to be deep sharded.
    /// WARNING: Using this in any kind of production service will lead to the loading of potentially tens of thousands of repos,
    /// considerably slowing down the startup. This flag primarily exists for the purpose of local testing with --filter-repos argument
    #[clap(long, requires = "filter_repos")]
    load_all_repos_in_tier: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum ThriftServerMode {
    Default,
    ThriftFactory,
    FactoryGroup,
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
    async fn setup(&self, repo: &RepoShard) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let repo_name = repo.repo_name.as_str();
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
        let is_deep_sharded = config
            .deep_sharding_config
            .and_then(|c| c.status.get(&ShardedService::SourceControlService).copied())
            .unwrap_or(false);
        if is_deep_sharded {
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

async fn create_git_source_of_truth_config(
    fb: FacebookInit,
) -> Result<Arc<dyn GitSourceOfTruthConfig>, Error> {
    let pool_options = ConnectionPoolOptionsBuilder::default()
        .build()
        .map_err(Error::msg)?;

    let conn_options = ConnectionOptionsBuilder::default()
        .build()
        .map_err(Error::msg)?;
    let destination = Destination::Prod;
    let xdb_factory = XdbFactory::new(fb, destination, pool_options, conn_options)?;
    let mononoke_production_xdb = xdb_factory
        .create_or_get_shard(MONONOKE_PRODUCTION_SHARD_NAME)
        .await?;
    let connections = mononoke_production_xdb.read_conns().await?;
    Ok(Arc::new(
        SqlGitSourceOfTruthConfigBuilder::from_sql_connections(connections).build(),
    ))
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    panichandler::set_panichandler(Fate::Abort);

    let app = MononokeAppBuilder::new(fb)
        .with_entry_point(ClientEntryPoint::ScsServer)
        .with_bookmarks_cache(BookmarkCacheOptions {
            cache_kind: BookmarkCacheKind::Local,
            derived_data: BookmarkCacheDerivedData::AllKinds,
        })
        .with_app_extension(WarmBookmarksCacheExtension {})
        .with_app_extension(HooksAppExtension {})
        .with_app_extension(RepoFilterAppExtension {})
        .build::<ScsServerArgs>()?;

    let args: ScsServerArgs = app.args()?;
    let logger = app.logger().clone();
    let runtime = app.runtime();
    let env = app.environment();

    let scuba_builder = env.scuba_sample_builder.clone();
    // Service name is used for shallow or deep sharding.
    let service_name = if args.load_all_repos_in_tier {
        None
    } else {
        Some(ShardedService::SourceControlService)
    };
    let repos_mgr = runtime.block_on(app.open_managed_repos(service_name))?;
    let mononoke = Arc::new(repos_mgr.make_mononoke_api()?);
    let megarepo_api = Arc::new(MegarepoApi::new(&app, mononoke.clone())?);

    let will_exit = Arc::new(AtomicBool::new(false));
    let (sm_shutdown_sender, sm_shutdown_receiver) = tokio::sync::oneshot::channel::<bool>();

    if let Some(max_memory) = args.max_memory {
        memory::set_max_memory(max_memory);
    }

    let security_checker = runtime.block_on(ConnectionSecurityChecker::new(
        app.environment().acl_provider.as_ref(),
        &app.repo_configs().common,
    ))?;

    let async_requests_queue_client = if args.disable_async_requests {
        None
    } else {
        let queue_client = runtime.block_on(async_requests_client::build(fb, &app, None))?;
        Some(Arc::new(queue_client))
    };

    let git_source_of_truth_config = runtime.block_on(create_git_source_of_truth_config(fb))?;

    let source_control_server = {
        let maybe_factory_group = if let ThriftServerMode::FactoryGroup = args.thift_server_mode {
            let worker_counts: [usize; NUM_PRIORITY_QUEUES] =
                vec![args.thrift_workers_num_fast, args.thrift_workers_num_slow]
                    .try_into()
                    .unwrap();
            Some(Arc::new(runtime.block_on(FactoryGroup::<
                { NUM_PRIORITY_QUEUES },
            >::new(
                fb,
                "requests-pri-queues",
                worker_counts,
                None,
            ))?))
        } else {
            None
        };
        runtime.block_on(SourceControlServiceImpl::new(
            fb,
            &app,
            mononoke.clone(),
            megarepo_api,
            logger.clone(),
            scuba_builder,
            args.scribe_logging_args.get_scribe(fb)?,
            security_checker,
            app.configs(),
            &app.repo_configs().common,
            maybe_factory_group,
            async_requests_queue_client,
            git_source_of_truth_config,
            args.watchdog_method_max_poll,
        ))?
    };

    let monitoring_forever = {
        let monitoring_ctx = CoreContext::new_with_logger(fb, logger.clone());
        monitoring::monitoring_stats_submitter(monitoring_ctx, mononoke)
    };
    runtime.spawn(monitoring_forever);

    let thrift = match args.thift_server_mode {
        ThriftServerMode::Default => setup_thrift_server(
            fb,
            &args,
            &will_exit,
            source_control_server,
            runtime.clone(),
        ),
        _ => {
            let (factory, _processing_handle) = runtime.block_on(async move {
                ThriftFactoryBuilder::new(fb, "main-thrift-incoming", args.thrift_workers_num)
                    .with_queueing_limit(args.thrift_queue_size)
                    .with_queueing_timeout(Some(Duration::from_millis(args.thrift_queue_timeout)))
                    .build()
                    .await
                    .expect("Failed to build thrift factory")
            });
            setup_thrift_server(fb, &args, &will_exit, source_control_server, factory)
        }
    }
    .context("Failed to set up Thrift server")?;

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

    if let Some(executor) = args.sharded_executor_args.build_executor(
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
                async move {
                    executor
                        .block_and_execute(&logger, sm_shutdown_receiver)
                        .await
                }
            }
        });
    }

    // Monitoring is provided by the `Fb303Module`, but we must still start
    // stats aggregation.
    app.start_stats_aggregation()?;

    app.wait_until_terminated(
        move || {
            let _ = sm_shutdown_sender.send(true);
            will_exit.store(true, Ordering::Relaxed)
        },
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
        None,
    )?;

    info!(logger, "Exiting...");
    Ok(())
}

fn setup_thrift_server(
    fb: FacebookInit,
    args: &ScsServerArgs,
    will_exit: &Arc<AtomicBool>,
    source_control_server: SourceControlServiceImpl,
    exec: impl 'static + Clone + ThriftExecutor + ThriftStreamExecutor,
) -> anyhow::Result<ThriftServer> {
    let fb303_base = {
        cloned!(will_exit);
        move |proto| {
            make_BaseService_server(proto, facebook::BaseServiceImpl::new(will_exit.clone()))
        }
    };

    let service = {
        move |proto| {
            make_SourceControlService_server(
                proto,
                source_control_server.thrift_server(),
                fb303_base.clone(),
            )
        }
    };

    Ok(ThriftServerBuilder::new(fb)
        .with_name(SERVICE_NAME)
        .expect("failed to set name")
        .with_address(&args.host, args.port, false)?
        .with_tls()
        .expect("failed to enable TLS")
        .with_cancel_if_client_disconnected()
        .add_factory(exec, move || service, Some(metadata::create_metadata()))
        .build())
}
