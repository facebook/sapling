/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobstore_factory::{BlobstoreOptions, ReadOnlyStorage};
use cached_config::ConfigStore;
use fbinit::FacebookInit;
use observability::ObservabilityContext;
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
    pub config_store: ConfigStore,
    pub caching: Caching,
    pub observability_context: ObservabilityContext,
    pub runtime: Runtime,
    pub mysql_options: MysqlOptions,
    pub blobstore_options: BlobstoreOptions,
    pub readonly_storage: ReadOnlyStorage,
}
