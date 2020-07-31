/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{format_err, Context, Error};
use backsyncer::{open_backsyncer_dbs, TargetRepoDbs};
use blobrepo::BlobRepo;
use blobrepo_factory::{BlobstoreOptions, Caching, ReadOnlyStorage};
use blobstore_factory::make_blobstore;
use cache_warmup::cache_warmup;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::{FutureExt as _, TryFutureExt},
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use futures_old::{future, Future};
use hooks::{hook_loader::load_hooks, HookManager};
use hooks_content_stores::blobrepo_text_only_fetcher;
use maplit::btreeset;
use mercurial_derived_data::MappedHgChangesetId;
use metaconfig_types::{CommitSyncConfig, RepoConfig, WireprotoLoggingConfig};
use mononoke_types::RepositoryId;
use repo_client::{MononokeRepo, MononokeRepoBuilder, PushRedirectorArgs, WireprotoLogging};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::{debug, info, o, Logger};
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::MysqlOptions;

use synced_commit_mapping::SqlSyncedCommitMapping;
use warm_bookmarks_cache::WarmBookmarksCache;

use crate::errors::ErrorKind;

/// An auxillary struct to pass between closures before we
/// are capable of creating a full `RepoHandler`
/// To create `RepoHandler`, we need to look at various
/// fields of such struct for other repos, so we first
/// have to construct all `IncompleteRepoHandler`s and
/// only then can we populate the `PushRedirector`
#[derive(Clone)]
struct IncompleteRepoHandler {
    logger: Logger,
    scuba: ScubaSampleBuilder,
    wireproto_logging: Arc<WireprotoLogging>,
    repo: MononokeRepo,
    hash_validation_percentage: usize,
    preserve_raw_bundle2: bool,
    maybe_incomplete_push_redirector_args: Option<IncompletePushRedirectorArgs>,
    maybe_warm_bookmarks_cache: Option<Arc<WarmBookmarksCache>>,
}

#[derive(Clone)]
struct IncompletePushRedirectorArgs {
    commit_sync_config: CommitSyncConfig,
    synced_commit_mapping: SqlSyncedCommitMapping,
    target_repo_dbs: TargetRepoDbs,
    source_blobrepo: BlobRepo,
}

impl IncompletePushRedirectorArgs {
    fn try_into_push_redirector_args(
        self,
        repo_lookup_table: &HashMap<RepositoryId, IncompleteRepoHandler>,
    ) -> Result<PushRedirectorArgs, Error> {
        let Self {
            commit_sync_config,
            synced_commit_mapping,
            target_repo_dbs,
            source_blobrepo,
        } = self;

        let large_repo_id = commit_sync_config.large_repo_id;
        let target_repo: MononokeRepo = repo_lookup_table
            .get(&large_repo_id)
            .ok_or(ErrorKind::LargeRepoNotFound(large_repo_id))?
            .repo
            .clone();

        Ok(PushRedirectorArgs::new(
            commit_sync_config,
            target_repo,
            source_blobrepo,
            synced_commit_mapping,
            target_repo_dbs,
        ))
    }
}

impl IncompleteRepoHandler {
    fn try_into_repo_handler(
        self,
        repo_lookup_table: &HashMap<RepositoryId, IncompleteRepoHandler>,
    ) -> Result<RepoHandler, Error> {
        let IncompleteRepoHandler {
            logger,
            scuba,
            wireproto_logging,
            repo,
            hash_validation_percentage,
            preserve_raw_bundle2,
            maybe_incomplete_push_redirector_args,
            maybe_warm_bookmarks_cache,
        } = self;

        let maybe_push_redirector_args = match maybe_incomplete_push_redirector_args {
            None => None,
            Some(incomplete_push_redirector_args) => Some(
                incomplete_push_redirector_args.try_into_push_redirector_args(repo_lookup_table)?,
            ),
        };

        Ok(RepoHandler {
            logger,
            scuba,
            wireproto_logging,
            repo,
            hash_validation_percentage,
            preserve_raw_bundle2,
            maybe_push_redirector_args,
            maybe_warm_bookmarks_cache,
        })
    }
}

#[derive(Clone)]
pub struct RepoHandler {
    pub logger: Logger,
    pub scuba: ScubaSampleBuilder,
    pub wireproto_logging: Arc<WireprotoLogging>,
    pub repo: MononokeRepo,
    pub hash_validation_percentage: usize,
    pub preserve_raw_bundle2: bool,
    pub maybe_push_redirector_args: Option<PushRedirectorArgs>,
    pub maybe_warm_bookmarks_cache: Option<Arc<WarmBookmarksCache>>,
}

pub fn repo_handlers(
    fb: FacebookInit,
    repos: impl IntoIterator<Item = (String, RepoConfig)>,
    mysql_options: MysqlOptions,
    caching: Caching,
    mut disabled_hooks: HashMap<String, HashSet<String>>,
    scuba_censored_table: Option<String>,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: BlobstoreOptions,
    root_log: &Logger,
) -> BoxFuture<HashMap<String, RepoHandler>, Error> {
    // compute eagerly to avoid lifetime issues
    let repo_futs: Vec<BoxFuture<(String, IncompleteRepoHandler), Error>> = repos
        .into_iter()
        .filter(|(reponame, config)| {
            if !config.enabled {
                info!(root_log, "Repo {} not enabled", reponame)
            };
            config.enabled
        })
        .map(|(reponame, config)| {
            let root_log = root_log.clone();
            let logger = root_log.new(o!("repo" => reponame.clone()));
            let ctx = CoreContext::new_with_logger(fb, logger.clone());

            let disabled_hooks = disabled_hooks.remove(&reponame).unwrap_or_default();

            // Clone the few things we're going to need later in our bootstrap.
            let cache_warmup_params = config.cache_warmup.clone();
            let scuba_table = config.scuba_table.clone();
            let scuba_local_path = config.scuba_local_path.clone();
            let hooks_scuba_table = config.scuba_table_hooks.clone();
            let hooks_scuba_local_path = config.scuba_local_path_hooks.clone();
            let hook_max_file_size = config.hook_max_file_size.clone();
            let db_config = config.storage_config.metadata.clone();
            let hash_validation_percentage = config.hash_validation_percentage.clone();
            let preserve_raw_bundle2 = config.bundle2_replay_params.preserve_raw_bundle2.clone();
            let wireproto_logging = config.wireproto_logging.clone();
            let commit_sync_config = config.commit_sync_config.clone();
            let hook_manager_params = config.hook_manager_params.clone();
            let record_infinitepush_writes: bool =
                config.infinitepush.populate_reverse_filler_queue
                    && config.infinitepush.allow_writes;
            let repo_client_use_warm_bookmarks_cache = config.repo_client_use_warm_bookmarks_cache;

            // TODO: Don't require full config in load_hooks so we can avoid cloning the entire
            // config here.
            let hook_config = config.clone();

            // And clone a few things of which we only have one but which we're going to need one
            // per repo.
            let blobstore_options = blobstore_options.clone();
            let scuba_censored_table = scuba_censored_table.clone();

            let fut = async move {
                info!(logger, "Opening blobrepo");
                let builder = MononokeRepoBuilder::prepare(
                    ctx.clone(),
                    reponame.clone(),
                    config,
                    mysql_options,
                    caching,
                    scuba_censored_table.clone(),
                    readonly_storage,
                    blobstore_options,
                    record_infinitepush_writes,
                )
                .await?;

                let blobrepo = builder.blobrepo().clone();

                info!(logger, "Warming up cache");
                let initial_warmup = tokio::task::spawn({
                    cloned!(ctx, blobrepo, reponame);
                    async move {
                        cache_warmup(&ctx, &blobrepo, cache_warmup_params)
                            .await
                            .with_context(|| {
                                format!("while warming up cache for repo: {}", reponame)
                            })
                    }
                });

                let mut scuba_logger = ScubaSampleBuilder::with_opt_table(fb, scuba_table);
                scuba_logger.add_common_server_data();
                if let Some(scuba_local_path) = scuba_local_path {
                    scuba_logger = scuba_logger.with_log_file(scuba_local_path)?;
                }
                scuba_logger = scuba_logger.with_seq("seq");

                let mut hooks_scuba = ScubaSampleBuilder::with_opt_table(fb, hooks_scuba_table);
                hooks_scuba.add("repo", reponame.clone());
                if let Some(hooks_scuba_local_path) = hooks_scuba_local_path {
                    hooks_scuba = hooks_scuba.with_log_file(hooks_scuba_local_path)?;
                }

                info!(logger, "Creating HookManager");
                let mut hook_manager = HookManager::new(
                    ctx.fb,
                    blobrepo_text_only_fetcher(blobrepo.clone(), hook_max_file_size),
                    hook_manager_params.unwrap_or_default(),
                    hooks_scuba,
                )
                .await?;

                info!(logger, "Loading hooks");
                load_hooks(fb, &mut hook_manager, hook_config, &disabled_hooks)?;

                let repo = builder.finalize(Arc::new(hook_manager));

                let sql_commit_sync_mapping = SqlSyncedCommitMapping::with_metadata_database_config(
                    fb,
                    &db_config,
                    mysql_options,
                    readonly_storage.0,
                );

                let wireproto_logging = create_wireproto_logging(
                    fb,
                    reponame.clone(),
                    mysql_options,
                    readonly_storage,
                    wireproto_logging,
                    logger.clone(),
                )
                .compat();

                let backsyncer_dbs = open_backsyncer_dbs(
                    ctx.clone(),
                    blobrepo.clone(),
                    db_config.clone(),
                    mysql_options,
                    readonly_storage,
                );
                let maybe_warm_bookmarks_cache = async {
                    if repo_client_use_warm_bookmarks_cache {
                        info!(
                            ctx.logger(),
                            "Starting Warm bookmarks cache for {}",
                            blobrepo.name()
                        );
                        Ok(Some(Arc::new(
                            WarmBookmarksCache::new_with_types(
                                &ctx,
                                &blobrepo,
                                &btreeset! { MappedHgChangesetId::NAME.to_string() },
                            )
                            .await?,
                        )))
                    } else {
                        Ok(None)
                    }
                };

                info!(
                    logger,
                    "Creating MononokeRepo, CommitSyncMapping, WireprotoLogging, TargetRepoDbs, \
                    WarmBookmarksCache"
                );
                let (
                    repo,
                    sql_commit_sync_mapping,
                    wireproto_logging,
                    backsyncer_dbs,
                    maybe_warm_bookmarks_cache,
                ) = futures::future::try_join5(
                    repo,
                    sql_commit_sync_mapping,
                    wireproto_logging,
                    backsyncer_dbs,
                    maybe_warm_bookmarks_cache,
                )
                .await?;

                let maybe_incomplete_push_redirector_args = commit_sync_config.and_then({
                    cloned!(logger);
                    move |commit_sync_config| {
                        if commit_sync_config.large_repo_id == blobrepo.get_repoid() {
                            debug!(
                                logger,
                                "Not constructing push redirection args: {:?}",
                                blobrepo.get_repoid()
                            );
                            None
                        } else {
                            debug!(
                                logger,
                                "Constructing incomplete push redirection args: {:?}",
                                blobrepo.get_repoid()
                            );
                            Some(IncompletePushRedirectorArgs {
                                commit_sync_config,
                                synced_commit_mapping: sql_commit_sync_mapping,
                                target_repo_dbs: backsyncer_dbs,
                                source_blobrepo: blobrepo,
                            })
                        }
                    }
                });

                initial_warmup.await??;

                info!(logger, "Repository is ready");
                Ok((
                    reponame,
                    IncompleteRepoHandler {
                        logger,
                        scuba: scuba_logger,
                        wireproto_logging: Arc::new(wireproto_logging),
                        repo,
                        hash_validation_percentage,
                        preserve_raw_bundle2,
                        maybe_incomplete_push_redirector_args,
                        maybe_warm_bookmarks_cache,
                    },
                ))
            };

            fut.boxed().compat().boxify()
        })
        .collect();

    future::join_all(repo_futs)
        .and_then(build_repo_handlers)
        .boxify()
}

fn build_repo_handlers(
    tuples: Vec<(String, IncompleteRepoHandler)>,
) -> impl Future<Item = HashMap<String, RepoHandler>, Error = Error> {
    let lookup_table: HashMap<RepositoryId, IncompleteRepoHandler> = tuples
        .iter()
        .map(|(_, incomplete_repo_handler)| {
            (
                incomplete_repo_handler.repo.repoid(),
                incomplete_repo_handler.clone(),
            )
        })
        .collect();

    future::join_all({
        cloned!(lookup_table);
        tuples
            .into_iter()
            .map(move |(reponame, incomplete_repo_handler)| {
                let repo_handler =
                    try_boxfuture!(incomplete_repo_handler.try_into_repo_handler(&lookup_table));
                future::ok((reponame, repo_handler)).boxify()
            })
    })
    .map(|v| v.into_iter().collect())
}

fn create_wireproto_logging(
    fb: FacebookInit,
    reponame: String,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    wireproto_logging_config: WireprotoLoggingConfig,
    logger: Logger,
) -> impl Future<Item = WireprotoLogging, Error = Error> {
    let WireprotoLoggingConfig {
        storage_config_and_threshold,
        scribe_category,
        local_path,
    } = wireproto_logging_config;
    let blobstore_fut = match storage_config_and_threshold {
        Some((storage_config, threshold)) => {
            if readonly_storage.0 {
                return future::err(format_err!(
                    "failed to create blobstore for wireproto logging because storage is readonly",
                ))
                .right_future();
            }

            async move {
                let blobstore = make_blobstore(
                    fb,
                    storage_config.blobstore,
                    mysql_options,
                    readonly_storage,
                    &Default::default(),
                    &logger,
                )
                .await?;

                Ok(Some((blobstore, threshold)))
            }
            .boxed()
            .compat()
            .left_future()
        }
        None => future::ok(None).right_future(),
    };

    blobstore_fut
        .and_then(move |blobstore_and_threshold| {
            WireprotoLogging::new(
                fb,
                reponame,
                scribe_category,
                blobstore_and_threshold,
                local_path.as_ref().map(|p| p.as_ref()),
            )
        })
        .left_future()
}
