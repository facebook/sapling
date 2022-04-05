/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, format_err, Context, Error};
use blobstore::Blobstore;
use blobstore_factory::ScrubHandler;
use cloned::cloned;
use cmdlib::args::ResolvedRepo;
use fbinit::FacebookInit;
use metaconfig_types::{MetadataDatabaseConfig, Redaction, RepoConfig};
use mononoke_app::MononokeApp;
use mononoke_types::repo::RepositoryId;
use newfilenodes::NewFilenodesBuilder;
use repo_factory::RepoFactory;
use samplingblob::{ComponentSamplingHandler, SamplingBlobstore, SamplingHandler};
use scuba_ext::MononokeScubaSampleBuilder;
use slog::{info, warn, Logger};
use sql_ext::facebook::MysqlOptions;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use walker_commands_impl::{
    blobstore::{replace_blobconfig, StatsScrubHandler},
    graph::SqlShardInfo,
    progress::sort_by_string,
    setup::{setup_repo, JobParams, JobWalkParams},
    tail::TailParams,
    validate::WALK_TYPE,
    walk::OutgoingEdge,
};

use crate::args::{TailArgs, WalkerCommonArgs, WalkerGraphParams};

#[allow(dead_code)]
pub async fn setup_common<'a>(
    walk_stats_key: &'static str,
    app: &MononokeApp,
    common_args: &WalkerCommonArgs,
    blobstore_sampler: Option<Arc<dyn SamplingHandler>>,
    blobstore_component_sampler: Option<Arc<dyn ComponentSamplingHandler>>,
) -> Result<JobParams, Error> {
    let logger = app.logger();

    let mut scuba_builder = app.environment().scuba_sample_builder.clone();
    scuba_builder.add(WALK_TYPE, walk_stats_key);

    let WalkerGraphParams {
        include_node_types,
        include_edge_types,
        error_as_data_node_types,
        error_as_data_edge_types,
    } = common_args.graph_params.parse_args()?;

    if !error_as_data_node_types.is_empty() || !error_as_data_edge_types.is_empty() {
        if !app.readonly_storage().0 {
            return Err(format_err!(
                "Error as data could mean internal state is invalid, run with --with-readonly-storage=true to ensure no risk of persisting it"
            ));
        }
        warn!(
            logger,
            "Error as data enabled, walk results may not be complete. Errors as data enabled for node types {:?} edge types {:?}",
            sort_by_string(&error_as_data_node_types),
            sort_by_string(&error_as_data_edge_types)
        );
    }

    // There is no need to check if repos is empty: at least one repo arg
    // is required when running the command.
    let repos = app.multi_repo_configs(common_args.repos.ids_or_names()?)?;
    let repo_count = repos.len();
    if repo_count > 1 {
        info!(
            logger,
            "Walking repos {:?}",
            repos.iter().map(|repo| &repo.0).collect::<Vec<_>>()
        );
    }

    let repos = override_repo_configs(walk_stats_key, app, common_args, repos)?;

    // configure repo factory
    let repo_id_to_name: HashMap<_, _> = repos
        .iter()
        .map(|(name, conf)| (conf.repoid, name.clone()))
        .collect();
    let repo_factory = setup_repo_factory(
        walk_stats_key,
        app,
        repo_id_to_name,
        blobstore_sampler,
        blobstore_component_sampler,
        scuba_builder.clone(),
        logger,
        common_args.quiet,
    );

    let progress_options = common_args.progress.parse_args();
    let hash_validation_node_types = common_args.hash_validation.parse_args()?;

    let mysql_options = app.mysql_options();

    let walk_roots = common_args.walk_roots.parse_args()?;
    let parsed_tail_params = parse_tail_params(
        app.fb,
        &common_args.tailing,
        mysql_options,
        &repos,
        &walk_roots,
    )?;

    let mut per_repo = Vec::new();
    for (repo, repo_conf) in repos {
        let metadatadb_config = &repo_conf.storage_config.metadata;
        let tail_params = parsed_tail_params
            .get(metadatadb_config)
            .ok_or_else(|| format_err!("No tail params for {}", repo))?;

        // repo factory reuses sql factory if one was already initiated for the config
        let sql_factory = repo_factory.sql_factory(metadatadb_config).await?;
        let sql_shard_info = SqlShardInfo {
            filenodes: sql_factory.tier_info_shardable::<NewFilenodesBuilder>()?,
            active_keys_per_shard: mysql_options.per_key_limit(),
        };

        let resolved_repo = ResolvedRepo {
            id: repo_conf.repoid,
            name: repo,
            config: repo_conf,
        };
        // For some reason the repos are initialized sequentially in the original
        // walker_commands_impl::setup::setup_common, he behaviour is preserved in here too.
        //
        // TODO(aida): Init repos in parallel.
        let one_repo = setup_repo(
            walk_stats_key,
            app.fb,
            logger,
            &repo_factory,
            scuba_builder.clone(),
            sql_shard_info,
            common_args.scheduled_max,
            repo_count,
            &resolved_repo,
            walk_roots.clone(),
            tail_params.clone(),
            include_edge_types.clone(),
            include_node_types.clone(),
            hash_validation_node_types.clone(),
            progress_options,
        )
        .await?;
        per_repo.push(one_repo);
    }

    Ok(JobParams {
        walk_params: JobWalkParams {
            enable_derive: common_args.enable_derive,
            quiet: common_args.quiet,
            error_as_data_node_types,
            error_as_data_edge_types,
            repo_count,
        },
        per_repo,
    })
}

// Override the blobstore config so we can do things like run on one side of a multiplex
fn override_repo_configs(
    walk_stats_key: &'static str,
    app: &MononokeApp,
    common_args: &WalkerCommonArgs,
    mut repos: Vec<(String, RepoConfig)>,
) -> Result<Vec<(String, RepoConfig)>, Error> {
    let storage_override = if let Some(storage_id) = &common_args.storage_id {
        let storage_configs = &app.storage_configs().storage;
        let config_opt = storage_configs.get(storage_id).cloned();
        let storage_config = config_opt.ok_or_else(|| {
            format_err!(
                "Storage id `{}` not found in {:?}",
                storage_id,
                storage_configs.keys()
            )
        })?;
        Some(storage_config)
    } else {
        None
    };

    for (name, config) in &mut repos {
        if let Some(storage_config) = storage_override.clone() {
            config.storage_config = storage_config;
        }
        replace_blobconfig(
            &mut config.storage_config.blobstore,
            common_args.inner_blobstore_id,
            name,
            walk_stats_key,
            app.blobstore_options().scrub_options.is_some(),
        )?;

        let sampling_multiplier =
            NonZeroU64::new(common_args.blobstore_sampling_multiplier).context("Cannot be zero")?;
        config
            .storage_config
            .blobstore
            .apply_sampling_multiplier(sampling_multiplier);
    }

    // Disable redaction unless we are running with it enabled.
    if !common_args.enable_redaction {
        for (_repo, config) in &mut repos {
            config.redaction = Redaction::Disabled;
        }
    };

    Ok(repos)
}

fn setup_repo_factory<'a>(
    walk_stats_key: &'static str,
    app: &MononokeApp,
    repo_id_to_name: HashMap<RepositoryId, String>,
    blobstore_sampler: Option<Arc<dyn SamplingHandler>>,
    blobstore_component_sampler: Option<Arc<dyn ComponentSamplingHandler>>,
    scuba_builder: MononokeScubaSampleBuilder,
    logger: &'a Logger,
    quiet: bool,
) -> RepoFactory {
    let mut repo_factory = app.repo_factory();
    if let Some(blobstore_sampler) = blobstore_sampler.clone() {
        repo_factory.with_blobstore_override({
            cloned!(logger);
            move |blobstore| -> Arc<dyn Blobstore> {
                if !quiet {
                    info!(logger, "Sampling from blobstore: {}", blobstore);
                }
                Arc::new(SamplingBlobstore::new(blobstore, blobstore_sampler.clone()))
            }
        });
    }

    if let Some(sampler) = blobstore_component_sampler {
        repo_factory.with_blobstore_component_sampler(sampler);
    }

    repo_factory.with_scrub_handler(Arc::new(StatsScrubHandler::new(
        false,
        scuba_builder,
        walk_stats_key,
        repo_id_to_name,
    )) as Arc<dyn ScrubHandler>);

    repo_factory
}

fn parse_tail_params(
    fb: FacebookInit,
    tail_args: &TailArgs,
    mysql_options: &MysqlOptions,
    repos: &[(String, RepoConfig)],
    walk_roots: &[OutgoingEdge],
) -> Result<HashMap<MetadataDatabaseConfig, TailParams>, Error> {
    let mut parsed_tail_params: HashMap<MetadataDatabaseConfig, TailParams> = HashMap::new();
    for (_repo, repo_conf) in repos {
        let metadatadb_config = &repo_conf.storage_config.metadata;
        let tail_params = match parsed_tail_params.get(metadatadb_config) {
            Some(tail_params) => tail_params.clone(),
            None => {
                let tail_params = tail_args.parse_args(fb, metadatadb_config, mysql_options)?;
                parsed_tail_params.insert(metadatadb_config.clone(), tail_params.clone());
                tail_params
            }
        };

        if tail_params.chunking.is_none() && walk_roots.is_empty() {
            bail!(
                "No walk roots provided, pass with  --bookmark, --walk-root or --chunk-by-public",
            );
        }
    }

    Ok(parsed_tail_params)
}
