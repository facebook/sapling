/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use edenapi_types::AnyFileContentId;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
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
    async fn upload_contents(&self, contents: Vec<(AnyFileContentId, FileContents)>) -> Result<()> {
        for (content_id, _blob) in contents {
            info!(&self.logger, "Uploading content with id: {:?}", content_id);
        }

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

    async fn set_bookmark(
        &self,
        bookmark: String,
        from: Option<HgChangesetId>,
        to: Option<HgChangesetId>,
    ) -> Result<()> {
        info!(
            &self.logger,
            "Uploading moving bookmark {} from {:?} to {:?}", bookmark, from, to
        );
        Ok(())
    }

    async fn upload_identical_changeset(
        &self,
        css: Vec<(HgBlobChangeset, BonsaiChangeset)>,
    ) -> Result<()> {
        for (hg_cs, bs_cs) in css {
            info!(
                &self.logger,
                "Uploading hg changeset with hgid {} and bsid{}",
                hg_cs.get_changeset_id(),
                bs_cs.get_changeset_id()
            );
        }
        Ok(())
    }

    async fn filter_existing_commits(
        &self,
        ids: Vec<(HgChangesetId, ChangesetId)>,
    ) -> Result<Vec<ChangesetId>> {
        info!(&self.logger, "Looking up hg changeset with ids {:?}", ids);
        Ok(vec![])
    }
}
