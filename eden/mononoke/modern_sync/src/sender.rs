/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_types::ContentId;
use mononoke_types::FileContents;
pub mod dummy;
pub mod edenapi;

pub enum Entry {
    #[allow(unused)]
    Content(ContentId, FileContents),
    #[allow(unused)]
    Tree(HgManifestId),
    #[allow(unused)]
    FileNode(HgFileNodeId),
    #[allow(unused)]
    HgChangeset(HgBlobChangeset),
}

#[async_trait]
pub trait ModernSyncSender {
    #[allow(unused)]
    async fn enqueue_entry(&self, entry: Entry) -> Result<()>;

    async fn upload_content(&self, content_id: ContentId, _blob: FileContents) -> Result<()>;

    async fn upload_trees(&self, trees: Vec<HgManifestId>) -> Result<()>;

    async fn upload_filenodes(&self, filenodes: Vec<HgFileNodeId>) -> Result<()>;

    async fn upload_hg_changeset(&self, hg_css: Vec<HgBlobChangeset>) -> Result<()>;

    async fn set_bookmark(
        &self,
        bookmark: String,
        from: Option<HgChangesetId>,
        to: Option<HgChangesetId>,
    ) -> Result<()>;
}
