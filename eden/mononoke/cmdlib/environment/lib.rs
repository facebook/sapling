/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use blobstore_factory::BlobstoreOptions;
use blobstore_factory::ReadOnlyStorage;
use cached_config::ConfigStore;
use clap::ValueEnum;
use derived_data_remote::RemoteDerivationOptions;
use fbinit::FacebookInit;
use megarepo_config::MononokeMegarepoConfigsOptions;
use observability::ObservabilityContext;
use permission_checker::AclProvider;
use rendezvous::RendezVousOptions;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;
use strum::EnumString;
use tokio::runtime::Handle;

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct LocalCacheConfig {
    /// Number of shards in the local blobstore cache
    pub blobstore_cache_shards: usize,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Caching {
    /// Caching is fully enabled, with both local and shared caches.
    Enabled(LocalCacheConfig),

    /// Caching is only enabled locally - the shared cache is disabled.
    LocalOnly(LocalCacheConfig),

    /// Caching is not enabled.
    Disabled,
}

#[derive(
    Copy,
    Clone,
    Debug,
    ValueEnum,
    EnumString,
    strum::Display,
    PartialEq,
    Eq
)]

/// Which derived data types should the cache wait for before
/// exposing the bookmark move to the users.
pub enum BookmarkCacheDerivedData {
    /// Only wait for hg derived data - the option used mainly by Mononoke EdenAPI Server.
    HgOnly,
    /// Wait for all derived data types - mainly used by Mononoke SCS Server.
    AllKinds,
    /// Don't wait for any derived data - advance bookmarks as they move.
    NoDerivation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookmarkCacheAddress {
    SmcTier(String),
    HostPort(String),
}

impl Default for BookmarkCacheAddress {
    fn default() -> Self {
        Self::SmcTier("mononoke-bookmark-cache".to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookmarkCacheKind {
    Disabled,
    Local,
    Remote(BookmarkCacheAddress),
}

#[derive(Clone, Debug)]
pub struct BookmarkCacheOptions {
    pub cache_kind: BookmarkCacheKind,
    pub derived_data: BookmarkCacheDerivedData,
}

impl Default for BookmarkCacheOptions {
    fn default() -> Self {
        Self {
            cache_kind: BookmarkCacheKind::Disabled,
            derived_data: BookmarkCacheDerivedData::NoDerivation,
        }
    }
}

/// Struct representing the configuration associated with a MononokeApp instance which
/// is immutable post the point of app construction.
pub struct MononokeEnvironment {
    pub fb: FacebookInit,
    pub logger: Logger,
    pub scuba_sample_builder: MononokeScubaSampleBuilder,
    pub warm_bookmarks_cache_scuba_sample_builder: MononokeScubaSampleBuilder,
    pub config_store: ConfigStore,
    pub caching: Caching,
    pub observability_context: ObservabilityContext,
    pub runtime: Handle,
    pub mysql_options: MysqlOptions,
    pub blobstore_options: BlobstoreOptions,
    pub readonly_storage: ReadOnlyStorage,
    pub rendezvous_options: RendezVousOptions,
    pub megarepo_configs_options: MononokeMegarepoConfigsOptions,
    pub remote_derivation_options: RemoteDerivationOptions,
    pub disabled_hooks: HashMap<String, HashSet<String>>,
    pub acl_provider: Arc<dyn AclProvider>,
    pub bookmark_cache_options: BookmarkCacheOptions,
    /// Function determining whether given repo (identified by name) should be loaded
    pub filter_repos: Option<Arc<dyn Fn(&str) -> bool + Send + Sync + 'static>>,
}
