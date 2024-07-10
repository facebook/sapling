/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use async_trait::async_trait;
use blobstore_factory::ReadOnlyStorage;
use context::CoreContext;
use fbinit::FacebookInit;
use megarepo_configs::SyncConfigVersion;
use megarepo_configs::SyncTargetConfig;
use megarepo_configs::Target;
use megarepo_error::MegarepoError;
use metaconfig_types::RepoConfig;
use slog::info;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;

mod reader;
mod writer;

use crate::facebook::reader::CfgrMononokeMegarepoConfigsReader;
use crate::facebook::writer::CfgrMononokeMegarepoConfigsWriter;
use crate::MononokeMegarepoConfigs;

pub struct CfgrMononokeMegarepoConfigs {
    reader: CfgrMononokeMegarepoConfigsReader,
    writer: CfgrMononokeMegarepoConfigsWriter,
}

impl CfgrMononokeMegarepoConfigs {
    pub fn new(
        fb: FacebookInit,
        logger: &Logger,
        mysql_options: MysqlOptions,
        readonly_storage: ReadOnlyStorage,
    ) -> Result<Self, MegarepoError> {
        info!(logger, "Creating a new CfgrMononokeMegarepoConfigs");

        Ok(Self {
            reader: CfgrMononokeMegarepoConfigsReader::new(
                fb,
                mysql_options.clone(),
                readonly_storage,
            )?,
            writer: CfgrMononokeMegarepoConfigsWriter::new(fb, mysql_options, readonly_storage)?,
        })
    }
}

#[async_trait]
impl MononokeMegarepoConfigs for CfgrMononokeMegarepoConfigs {
    /// Get a SyncTargetConfig by its version
    async fn get_config_by_version(
        &self,
        ctx: CoreContext,
        repo_config: Arc<RepoConfig>,
        target: Target,
        version: SyncConfigVersion,
    ) -> Result<SyncTargetConfig, MegarepoError> {
        let get_config_by_version =
            self.reader
                .get_config_by_version(ctx, repo_config, target, version);
        get_config_by_version.await
    }

    /// Add a new unused SyncTargetConfig
    async fn add_config_version(
        &self,
        ctx: CoreContext,
        repo_config: Arc<RepoConfig>,
        config: SyncTargetConfig,
    ) -> Result<(), MegarepoError> {
        self.writer
            .add_config_version(ctx, repo_config, config)
            .await
    }
}
