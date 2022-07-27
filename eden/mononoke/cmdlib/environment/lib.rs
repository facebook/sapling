/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use blobstore_factory::BlobstoreOptions;
use blobstore_factory::ReadOnlyStorage;
use cached_config::ConfigStore;
use derived_data_remote::RemoteDerivationOptions;
use fbinit::FacebookInit;
use megarepo_config::MononokeMegarepoConfigsOptions;
use observability::ObservabilityContext;
use permission_checker::AclProvider;
use rendezvous::RendezVousOptions;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;
use tokio::runtime::Runtime;

#[derive(Copy, Clone, PartialEq)]
pub enum Caching {
    /// Caching is enabled with the given number of shards.
    Enabled(usize),

    /// Caching is enabled only for the blobstore via cachelib, with the given
    /// number of shards.
    CachelibOnlyBlobstore(usize),

    /// Caching is not enabled.
    Disabled,
}

/// One instance of a Mononoke program. This is primarily useful to pass into a RepoFactory.
pub struct MononokeEnvironment {
    pub fb: FacebookInit,
    pub logger: Logger,
    pub scuba_sample_builder: MononokeScubaSampleBuilder,
    pub warm_bookmarks_cache_scuba_sample_builder: MononokeScubaSampleBuilder,
    pub config_store: ConfigStore,
    pub caching: Caching,
    pub observability_context: ObservabilityContext,
    pub runtime: Runtime,
    pub mysql_options: MysqlOptions,
    pub blobstore_options: BlobstoreOptions,
    pub readonly_storage: ReadOnlyStorage,
    pub rendezvous_options: RendezVousOptions,
    pub megarepo_configs_options: MononokeMegarepoConfigsOptions,
    pub remote_derivation_options: RemoteDerivationOptions,
    pub disabled_hooks: HashMap<String, HashSet<String>>,
    pub acl_provider: Box<dyn AclProvider>,
}
