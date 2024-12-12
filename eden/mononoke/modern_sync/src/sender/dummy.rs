/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
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

    async fn upload_trees(&self, trees: Vec<HgManifestId>) -> Result<()> {
        for tree in trees {
            info!(&self.logger, "Uploading tree with id {:?}", tree);
        }
        Ok(())
    }

    async fn upload_filenodes(&self, filenodes: Vec<HgFileNodeId>) -> Result<()> {
        for filenode in filenodes {
            info!(&self.logger, "Uploading filenode with id {}", filenode);
        }
        Ok(())
    }
}
