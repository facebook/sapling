/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use edenapi_types::HgFilenodeData;
use edenapi_types::UploadTreeEntry;
use mononoke_types::ContentId;
use mononoke_types::FileContents;
use slog::info;
use slog::Logger;

use crate::sender::ModernSyncSender;

#[derive(Clone)]
pub struct DummySender {
    logger: Logger,
}

impl DummySender {
    pub fn new(logger: Logger) -> Self {
        Self { logger }
    }
}

#[async_trait]
impl ModernSyncSender for DummySender {
    async fn upload_content(&self, content_id: ContentId, _blob: FileContents) -> Result<()> {
        info!(&self.logger, "Uploading content with id: {:?}", content_id);
        Ok(())
    }

    async fn upload_tree(&self, trees: Vec<UploadTreeEntry>) -> Result<()> {
        for tree in trees {
            info!(&self.logger, "Uploading tree with id {:?}", tree.node_id);
        }
        Ok(())
    }

    async fn upload_filenodes(&self, filenodes: Vec<HgFilenodeData>) -> Result<()> {
        for filenode in filenodes {
            info!(
                &self.logger,
                "Uploading filenode with id {:?}", filenode.node_id
            );
        }
        Ok(())
    }
}
