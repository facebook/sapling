/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use async_trait::async_trait;
use cached_config::ConfigStore;
use context::CoreContext;
use fbinit::FacebookInit;
use megarepo_configs::SyncConfigVersion;
use megarepo_configs::SyncTargetConfig;
use megarepo_configs::Target;
use megarepo_error::MegarepoError;
use slog::info;
use slog::Logger;

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
    pub fn new(
        fb: FacebookInit,
        logger: &Logger,
        config_store: ConfigStore,
        test_write_path: Option<PathBuf>,
    ) -> Result<Self, MegarepoError> {
        info!(logger, "Creating a new CfgrMononokeMegarepoConfigs");
        let writer = if let Some(write_path) = test_write_path {
            CfgrMononokeMegarepoConfigsWriter::new_test(write_path)?
        } else {
            CfgrMononokeMegarepoConfigsWriter::new(fb)?
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
        config: SyncTargetConfig,
    ) -> Result<(), MegarepoError> {
        self.writer.add_config_version(ctx, config).await
    }
}
