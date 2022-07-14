/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod app;
mod cache;
mod matches;

pub use self::cache::CachelibSettings;

use std::borrow::Borrow;
use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use cached_config::ConfigStore;
use clap_old::ArgMatches;
use fbinit::FacebookInit;
use scribe_ext::Scribe;
use slog::info;
use slog::warn;
use slog::Logger;

pub use metaconfig_parser::RepoConfigs;
pub use metaconfig_parser::StorageConfigs;
use metaconfig_types::BlobConfig;
use metaconfig_types::CommonConfig;
use metaconfig_types::Redaction;
use metaconfig_types::RepoConfig;
use mononoke_types::RepositoryId;
use repo_factory::RepoFactory;
use repo_factory::RepoFactoryBuilder;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;

use crate::helpers::setup_repo_dir;
use crate::helpers::CreateStorage;

use self::app::CONFIG_PATH;
use self::app::REPO_ID;
use self::app::REPO_NAME;
use self::app::SCRIBE_LOGGING_DIRECTORY;
use self::app::SOURCE_REPO_ID;
use self::app::SOURCE_REPO_NAME;
use self::app::TARGET_REPO_ID;
use self::app::TARGET_REPO_NAME;

pub use self::app::ArgType;
pub use self::app::MononokeAppBuilder;
pub use self::app::MononokeClapApp;
pub use self::app::RepoRequirement;
pub use self::matches::MononokeMatches;

fn get_repo_id_and_name_from_values<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
    option_repo_name: &str,
    option_repo_id: &str,
) -> Result<(RepositoryId, String)> {
    let resolved = resolve_repo(config_store, matches, option_repo_name, option_repo_id)?;
    Ok((resolved.id, resolved.name))
}

pub struct ResolvedRepo {
    pub id: RepositoryId,
    pub name: String,
    pub config: RepoConfig,
}

pub fn resolve_repo_by_name<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
    repo_name: &str,
) -> Result<ResolvedRepo> {
    let configs = load_repo_configs(config_store, matches)?;
    resolve_repo_given_name(repo_name, &configs)
}

pub fn resolve_repo_by_id<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
    repo_id: i32,
) -> Result<ResolvedRepo> {
    let configs = load_repo_configs(config_store, matches)?;
    resolve_repo_given_id(RepositoryId::new(repo_id), &configs)
}

pub fn resolve_repo<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
    option_repo_name: &str,
    option_repo_id: &str,
) -> Result<ResolvedRepo> {
    let repo_name = matches.value_of(option_repo_name);
    let repo_id = matches.value_of(option_repo_id);
    let configs = load_repo_configs(config_store, matches)?;
    match (repo_name, repo_id) {
        (Some(_), Some(_)) => bail!("both repo-name and repo-id parameters set"),
        (None, None) => bail!("neither repo-name nor repo-id parameter set"),
        (None, Some(repo_id)) => resolve_repo_given_id(RepositoryId::from_str(repo_id)?, &configs),
        (Some(repo_name), None) => resolve_repo_given_name(repo_name, &configs),
    }
}

pub fn resolve_repos<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<Vec<ResolvedRepo>> {
    resolve_repos_from_args(config_store, matches, REPO_NAME, REPO_ID)
}

fn resolve_repos_from_args<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
    option_repo_name: &str,
    option_repo_id: &str,
) -> Result<Vec<ResolvedRepo>> {
    if matches.app_data().repo_required == Some(RepoRequirement::ExactlyOne) {
        return resolve_repo(config_store, matches, option_repo_name, option_repo_id)
            .map(|r| vec![r]);
    }

    let repo_names = matches.values_of(option_repo_name);
    let repo_ids = matches.values_of(option_repo_id);
    let configs = load_repo_configs(config_store, matches)?;

    let mut repos = Vec::new();
    let mut names = HashSet::new();
    if let Some(repo_ids) = repo_ids {
        for i in repo_ids {
            let resolved = resolve_repo_given_id(RepositoryId::from_str(i)?, &configs)?;
            if names.insert(resolved.name.clone()) {
                repos.push(resolved);
            }
        }
    }
    if let Some(repo_names) = repo_names {
        for n in repo_names {
            let resolved = resolve_repo_given_name(n, &configs)?;
            if names.insert(n.to_string()) {
                repos.push(resolved)
            }
        }
    }
    if repos.is_empty() {
        bail!("neither repo-name nor repo-id parameters set");
    }
    Ok(repos)
}

fn resolve_repo_given_id(id: RepositoryId, configs: &RepoConfigs) -> Result<ResolvedRepo> {
    let config = configs
        .repos
        .iter()
        .filter(|(_, c)| c.repoid == id)
        .enumerate()
        .last();
    if let Some((count, (name, config))) = config {
        if count > 1 {
            Err(format_err!("multiple configs defined for repo-id {:?}", id))
        } else {
            Ok(ResolvedRepo {
                id,
                name: name.to_string(),
                config: config.clone(),
            })
        }
    } else {
        Err(format_err!("unknown config for repo-id {:?}", id))
    }
}

fn resolve_repo_given_name(name: &str, configs: &RepoConfigs) -> Result<ResolvedRepo> {
    let config = configs.repos.get(name);
    if let Some(config) = config {
        Ok(ResolvedRepo {
            id: config.repoid,
            name: name.to_string(),
            config: config.clone(),
        })
    } else {
        Err(format_err!("unknown repo-name {:?}", name))
    }
}

pub fn get_repo_id<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<RepositoryId> {
    let (repo_id, _) = get_repo_id_and_name_from_values(config_store, matches, REPO_NAME, REPO_ID)?;
    Ok(repo_id)
}

pub fn get_repo_name<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<String> {
    let (_, repo_name) =
        get_repo_id_and_name_from_values(config_store, matches, REPO_NAME, REPO_ID)?;
    Ok(repo_name)
}

pub fn get_source_repo_id<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<RepositoryId> {
    let (repo_id, _) =
        get_repo_id_and_name_from_values(config_store, matches, SOURCE_REPO_NAME, SOURCE_REPO_ID)?;
    Ok(repo_id)
}

pub fn get_source_repo_id_opt<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<Option<RepositoryId>> {
    if matches.is_present(SOURCE_REPO_NAME) || matches.is_present(SOURCE_REPO_ID) {
        let (repo_id, _) = get_repo_id_and_name_from_values(
            config_store,
            matches,
            SOURCE_REPO_NAME,
            SOURCE_REPO_ID,
        )?;
        Ok(Some(repo_id))
    } else {
        Ok(None)
    }
}

pub fn get_target_repo_id<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<RepositoryId> {
    let (repo_id, _) =
        get_repo_id_and_name_from_values(config_store, matches, TARGET_REPO_NAME, TARGET_REPO_ID)?;
    Ok(repo_id)
}

pub fn get_repo_id_from_value<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
    repo_id_arg: &str,
    repo_name_arg: &str,
) -> Result<RepositoryId> {
    let (repo_id, _) =
        get_repo_id_and_name_from_values(config_store, matches, repo_name_arg, repo_id_arg)?;
    Ok(repo_id)
}

pub fn open_sql<'a, T>(
    fb: FacebookInit,
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<T, Error>
where
    T: SqlConstructFromMetadataDatabaseConfig,
{
    let (_, config) = get_config(config_store, matches)?;
    T::with_metadata_database_config(
        fb,
        &config.storage_config.metadata,
        matches.mysql_options(),
        matches.readonly_storage().0,
    )
}

pub fn open_sql_with_config<'a, T>(
    fb: FacebookInit,
    matches: &'a MononokeMatches<'a>,
    repo_config: &RepoConfig,
) -> Result<T, Error>
where
    T: SqlConstructFromMetadataDatabaseConfig,
{
    T::with_metadata_database_config(
        fb,
        &repo_config.storage_config.metadata,
        matches.mysql_options(),
        matches.readonly_storage().0,
    )
}

pub fn open_source_sql<'a, T>(
    fb: FacebookInit,
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<T, Error>
where
    T: SqlConstructFromMetadataDatabaseConfig,
{
    let source_repo_id = get_source_repo_id(config_store, matches)?;
    let (_, config) = get_config_by_repoid(config_store, matches, source_repo_id)?;
    T::with_metadata_database_config(
        fb,
        &config.storage_config.metadata,
        matches.mysql_options(),
        matches.readonly_storage().0,
    )
}

/// Create a new repo object -- for local instances, expect its contents to be empty.
#[inline]
pub fn create_repo<'a, R: 'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a MononokeMatches<'a>,
) -> impl Future<Output = Result<R, Error>> + 'a
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    open_repo_internal(fb, logger, matches, true, None, None)
}

/// Create a new repo object -- for local instances, expect its contents to be empty.
/// Make sure that the opened repo has redaction disabled
#[inline]
pub fn create_repo_unredacted<'a, R: 'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a MononokeMatches<'a>,
) -> impl Future<Output = Result<R, Error>> + 'a
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    open_repo_internal(fb, logger, matches, true, Some(Redaction::Disabled), None)
}

/// Open an existing repo object -- for local instances, expect contents to already be there.
#[inline]
pub fn open_repo<'a, R: 'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a MononokeMatches<'a>,
) -> impl Future<Output = Result<R, Error>> + 'a
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    open_repo_internal(fb, logger, matches, false, None, None)
}

#[inline]
pub fn open_repo_by_name<'a, R: 'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a MononokeMatches<'a>,
    repo_name: String,
) -> impl Future<Output = Result<R, Error>> + 'a
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    open_repo_by_name_internal(fb, logger, matches, false, None, None, repo_name)
}

#[inline]
pub fn open_repo_with_factory<'a, R: 'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a MononokeMatches<'a>,
    repo_factory: RepoFactory,
) -> impl Future<Output = Result<R, Error>> + 'a
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    open_repo_internal(fb, logger, matches, false, None, Some(repo_factory))
}

/// Open an existing repo object -- for local instances, expect contents to already be there.
/// Make sure that the opened repo has redaction disabled
#[inline]
pub fn open_repo_unredacted<'a, R: 'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a MononokeMatches<'a>,
) -> impl Future<Output = Result<R, Error>> + 'a
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    open_repo_internal(fb, logger, matches, false, Some(Redaction::Disabled), None)
}

/// Open the repo corresponding to the provided repo-name.
/// Make sure that the opened repo has redaction disabled
#[inline]
pub fn open_repo_by_name_unredacted<'a, R: 'a>(
    fb: FacebookInit,
    logger: &'a Logger,
    matches: &'a MononokeMatches<'a>,
    repo_name: String,
) -> impl Future<Output = Result<R, Error>> + 'a
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    open_repo_by_name_internal(
        fb,
        logger,
        matches,
        false,
        Some(Redaction::Disabled),
        None,
        repo_name,
    )
}

pub fn get_repo_factory<'a>(matches: &'a MononokeMatches<'a>) -> Result<RepoFactory, Error> {
    let config_store = matches.config_store();
    let common_config = load_common_config(config_store, matches)?;
    Ok(RepoFactory::new(
        matches.environment().clone(),
        &common_config,
    ))
}

/// Open an existing repo object by ID -- for local instances, expect contents to already be there.
/// It useful when we need to open more than 1 mononoke repo based on command line arguments
#[inline]
pub async fn open_repo_by_id<'a, R: 'a>(
    _: FacebookInit,
    logger: &'a Logger,
    matches: &'a MononokeMatches<'a>,
    repo_id: RepositoryId,
) -> Result<R, Error>
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    let repo_factory = get_repo_factory(matches)?;
    open_repo_internal_with_repo_id(
        logger,
        RepoIdentifier::Id(repo_id),
        matches,
        false, // use CreateStorage::ExistingOnly when creating blobstore
        None,  // do not override redaction config
        repo_factory,
    )
    .await
}

pub async fn open_source_repo<'a, R: 'a>(
    _: FacebookInit,
    logger: &'a Logger,
    matches: &'a MononokeMatches<'a>,
) -> Result<R, Error>
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    let config_store = matches.config_store();
    let source_repo_id = get_source_repo_id(config_store, matches)?;
    let repo_factory = get_repo_factory(matches)?;

    open_repo_internal_with_repo_id(
        logger,
        RepoIdentifier::Id(source_repo_id),
        matches,
        false, // use CreateStorage::ExistingOnly when creating blobstore
        None,  // do not override redaction config
        repo_factory,
    )
    .await
}

pub async fn open_target_repo<'a, R: 'a>(
    _: FacebookInit,
    logger: &'a Logger,
    matches: &'a MononokeMatches<'a>,
) -> Result<R, Error>
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    let config_store = matches.config_store();
    let source_repo_id = get_target_repo_id(config_store, matches)?;
    let repo_factory = get_repo_factory(matches)?;

    open_repo_internal_with_repo_id(
        logger,
        RepoIdentifier::Id(source_repo_id),
        matches,
        false, // use CreateStorage::ExistingOnly when creating blobstore
        None,  // do not override redaction config
        repo_factory,
    )
    .await
}

pub fn get_shutdown_grace_period<'a>(matches: &MononokeMatches<'a>) -> Result<Duration> {
    let seconds = matches
        .value_of("shutdown-grace-period")
        .ok_or_else(|| Error::msg("shutdown-grace-period must be specified"))?
        .parse()
        .map_err(Error::from)?;
    Ok(Duration::from_secs(seconds))
}

pub fn get_shutdown_timeout<'a>(matches: &MononokeMatches<'a>) -> Result<Duration> {
    let seconds = matches
        .value_of("shutdown-timeout")
        .ok_or_else(|| Error::msg("shutdown-timeout must be specified"))?
        .parse()
        .map_err(Error::from)?;
    Ok(Duration::from_secs(seconds))
}

pub fn get_scribe<'a>(fb: FacebookInit, matches: &MononokeMatches<'a>) -> Result<Scribe> {
    match matches.value_of(SCRIBE_LOGGING_DIRECTORY) {
        Some(dir) => Ok(Scribe::new_to_file(PathBuf::from(dir))),
        None => Ok(Scribe::new(fb)),
    }
}

pub fn get_config_path<'a>(matches: &'a MononokeMatches<'a>) -> Result<&'a str> {
    matches
        .value_of(CONFIG_PATH)
        .ok_or_else(|| Error::msg(format!("{} must be specified", CONFIG_PATH)))
}

pub fn load_repo_configs<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<RepoConfigs> {
    metaconfig_parser::load_repo_configs(get_config_path(matches)?, config_store)
}

pub fn load_common_config<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<CommonConfig> {
    metaconfig_parser::load_common_config(get_config_path(matches)?, config_store)
}

pub fn load_storage_configs<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<StorageConfigs> {
    metaconfig_parser::load_storage_configs(get_config_path(matches)?, config_store)
}

pub fn get_config<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
) -> Result<(String, RepoConfig)> {
    let repo_id = get_repo_id(config_store, matches)?;
    get_config_by_repoid(config_store, matches, repo_id)
}

pub fn get_config_by_repoid<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
    repo_id: RepositoryId,
) -> Result<(String, RepoConfig)> {
    let configs = load_repo_configs(config_store, matches)?;
    configs
        .get_repo_config(repo_id)
        .ok_or_else(|| format_err!("unknown repoid {:?}", repo_id))
        .map(|(name, config)| (name.clone(), config.clone()))
}

pub fn get_config_by_name<'a>(
    config_store: &ConfigStore,
    matches: &'a MononokeMatches<'a>,
    repo_name: String,
) -> Result<RepoConfig> {
    let configs = load_repo_configs(config_store, matches)?;
    configs
        .repos
        .get(&repo_name)
        .cloned()
        .ok_or_else(|| format_err!("unknown reponame {:?}", repo_name))
}

enum RepoIdentifier {
    Id(RepositoryId),
    Name(String),
}

async fn open_repo_internal<R>(
    _: FacebookInit,
    logger: &Logger,
    matches: &MononokeMatches<'_>,
    create: bool,
    redaction_override: Option<Redaction>,
    maybe_repo_factory: Option<RepoFactory>,
) -> Result<R, Error>
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    let config_store = matches.config_store();
    let repo_id = get_repo_id(config_store, matches)?;

    let repo_factory = match maybe_repo_factory {
        Some(repo_factory) => repo_factory,
        None => get_repo_factory(matches)?,
    };

    open_repo_internal_with_repo_id(
        logger,
        RepoIdentifier::Id(repo_id),
        matches,
        create,
        redaction_override,
        repo_factory,
    )
    .await
}

async fn open_repo_by_name_internal<R>(
    _: FacebookInit,
    logger: &Logger,
    matches: &MononokeMatches<'_>,
    create: bool,
    redaction_override: Option<Redaction>,
    maybe_repo_factory: Option<RepoFactory>,
    repo_name: String,
) -> Result<R, Error>
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    let repo_factory = match maybe_repo_factory {
        Some(repo_factory) => repo_factory,
        None => get_repo_factory(matches)?,
    };

    open_repo_internal_with_repo_id(
        logger,
        RepoIdentifier::Name(repo_name),
        matches,
        create,
        redaction_override,
        repo_factory,
    )
    .await
}

async fn open_repo_internal_with_repo_id<R>(
    logger: &Logger,
    repo_id: RepoIdentifier,
    matches: &MononokeMatches<'_>,
    create: bool,
    redaction_override: Option<Redaction>,
    repo_factory: RepoFactory,
) -> Result<R, Error>
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    let config_store = matches.config_store();

    let (reponame, repo_id, mut config) = match repo_id {
        RepoIdentifier::Id(repo_id) => {
            let (reponame, config) = get_config_by_repoid(config_store, matches, repo_id)?;
            (reponame, repo_id, config)
        }
        RepoIdentifier::Name(name) => {
            let config = get_config_by_name(config_store, matches, name.clone())?;
            (name, config.repoid, config)
        }
    };

    info!(logger, "using repo \"{}\" repoid {:?}", reponame, repo_id);
    match &config.storage_config.blobstore {
        BlobConfig::Files { path } | BlobConfig::Sqlite { path } => {
            let create = if create {
                // Many path repos can share one blobstore, so allow store to exist or create it.
                CreateStorage::ExistingOrCreate
            } else {
                CreateStorage::ExistingOnly
            };
            setup_repo_dir(path, create)?;
        }
        _ => {}
    };

    if let Some(redaction_override) = redaction_override {
        config.redaction = redaction_override;
    }

    let repo = repo_factory.build(reponame, config).await?;

    Ok(repo)
}

pub async fn open_repo_with_repo_id<'a, R: 'a>(
    _: FacebookInit,
    logger: &Logger,
    repo_id: RepositoryId,
    matches: &'a MononokeMatches<'a>,
) -> Result<R, Error>
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    let repo_factory = get_repo_factory(matches)?;

    open_repo_internal_with_repo_id(
        logger,
        RepoIdentifier::Id(repo_id),
        matches,
        false,
        None,
        repo_factory,
    )
    .await
}

pub async fn open_repo_with_repo_name<'a, R: 'a>(
    _: FacebookInit,
    logger: &Logger,
    repo_name: String,
    matches: &'a MononokeMatches<'a>,
) -> Result<R, Error>
where
    R: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    let repo_factory = get_repo_factory(matches)?;

    open_repo_internal_with_repo_id(
        logger,
        RepoIdentifier::Name(repo_name),
        matches,
        false,
        None,
        repo_factory,
    )
    .await
}

pub fn get_usize_opt<'a>(matches: &impl Borrow<ArgMatches<'a>>, key: &str) -> Option<usize> {
    matches.borrow().value_of(key).map(|val| {
        val.parse::<usize>()
            .unwrap_or_else(|_| panic!("{} must be integer", key))
    })
}

#[inline]
pub fn get_usize<'a>(matches: &impl Borrow<ArgMatches<'a>>, key: &str, default: usize) -> usize {
    get_usize_opt(matches, key).unwrap_or(default)
}

#[inline]
pub fn get_u64<'a>(matches: &impl Borrow<ArgMatches<'a>>, key: &str, default: u64) -> u64 {
    get_u64_opt(matches, key).unwrap_or(default)
}

#[inline]
pub fn get_and_parse_opt<'a, T: ::std::str::FromStr, M: Borrow<ArgMatches<'a>>>(
    matches: &M,
    key: &str,
) -> Option<T>
where
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    matches.borrow().value_of(key).map(|val| {
        val.parse::<T>()
            .unwrap_or_else(|_| panic!("{} - invalid value", key))
    })
}

#[inline]
pub fn get_and_parse<'a, T: ::std::str::FromStr, M: Borrow<ArgMatches<'a>>>(
    matches: &M,
    key: &str,
    default: T,
) -> T
where
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    get_and_parse_opt(matches, key).unwrap_or(default)
}

#[inline]
pub fn get_u64_opt<'a>(matches: &impl Borrow<ArgMatches<'a>>, key: &str) -> Option<u64> {
    matches.borrow().value_of(key).map(|val| {
        val.parse::<u64>()
            .unwrap_or_else(|_| panic!("{} must be integer", key))
    })
}

#[inline]
pub fn get_i32_opt<'a>(matches: &impl Borrow<ArgMatches<'a>>, key: &str) -> Option<i32> {
    matches.borrow().value_of(key).map(|val| {
        val.parse::<i32>()
            .unwrap_or_else(|_| panic!("{} must be integer", key))
    })
}

#[inline]
pub fn get_i32<'a>(matches: &impl Borrow<ArgMatches<'a>>, key: &str, default: i32) -> i32 {
    get_i32_opt(matches, key).unwrap_or(default)
}

#[inline]
pub fn get_i64_opt<'a>(matches: &impl Borrow<ArgMatches<'a>>, key: &str) -> Option<i64> {
    matches.borrow().value_of(key).map(|val| {
        val.parse::<i64>()
            .unwrap_or_else(|_| panic!("{} must be integer", key))
    })
}

pub fn get_bool_opt<'a>(matches: &impl Borrow<ArgMatches<'a>>, key: &str) -> Option<bool> {
    matches.borrow().value_of(key).map(|val| {
        val.parse::<bool>()
            .unwrap_or_else(|_| panic!("{} must be bool", key))
    })
}

pub fn parse_disabled_hooks_no_repo_prefix<'a>(
    matches: &'a MononokeMatches<'a>,
    logger: &Logger,
) -> HashSet<String> {
    let disabled_hooks: HashSet<String> = matches
        .values_of("disabled-hooks")
        .map(Vec::from_iter)
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    if !disabled_hooks.is_empty() {
        warn!(
            logger,
            "The following Hooks were disabled: {:?}", disabled_hooks
        );
    }

    disabled_hooks
}

/// Fxed macro from clap2 so it refers to clap_old
#[macro_export]
macro_rules! value_t {
    ($m:ident, $v:expr, $t:ty) => {
        value_t!($m.value_of($v), $t)
    };
    ($m:ident.value_of($v:expr), $t:ty) => {
        if let Some(v) = $m.value_of($v) {
            match v.parse::<$t>() {
                Ok(val) => Ok(val),
                Err(_) => Err(::clap_old::Error::value_validation_auto(format!(
                    "The argument '{}' isn't a valid value",
                    v
                ))),
            }
        } else {
            Err(::clap_old::Error::argument_not_found_auto($v))
        }
    };
}
