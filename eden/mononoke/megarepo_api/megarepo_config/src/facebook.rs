/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use blobstore_factory::ReadOnlyStorage;
use cached_config::ConfigStore;
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

mod paths;
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
    pub async fn new(
        fb: FacebookInit,
        logger: &Logger,
        mysql_options: MysqlOptions,
        readonly_storage: ReadOnlyStorage,
        config_store: ConfigStore,
        test_write_path: Option<PathBuf>,
    ) -> Result<Self, MegarepoError> {
        info!(logger, "Creating a new CfgrMononokeMegarepoConfigs");

        let writer = if let Some(write_path) = test_write_path {
            CfgrMononokeMegarepoConfigsWriter::new_test(
                fb,
                mysql_options,
                readonly_storage,
                write_path,
            )?
        } else {
            CfgrMononokeMegarepoConfigsWriter::new(fb, mysql_options, readonly_storage)?
        };
        Ok(Self {
            reader: CfgrMononokeMegarepoConfigsReader::new(config_store)?,
            writer,
        })
    }
}

#[async_trait]
impl MononokeMegarepoConfigs for CfgrMononokeMegarepoConfigs {
    /// Get all the versions for a given Target
    fn get_target_config_versions(
        &self,
        ctx: CoreContext,
        target: Target,
    ) -> Result<Vec<SyncConfigVersion>, MegarepoError> {
        self.reader.get_target_config_versions(ctx, target)
    }

    /// Get a SyncTargetConfig by its version
    fn get_config_by_version(
        &self,
        ctx: CoreContext,
        target: Target,
        version: SyncConfigVersion,
    ) -> Result<SyncTargetConfig, MegarepoError> {
        self.reader.get_config_by_version(ctx, target, version)
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
