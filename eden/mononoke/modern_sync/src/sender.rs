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
use mononoke_types::FileContents;
pub mod dummy;
pub mod edenapi;

pub enum Entry {
    #[allow(unused)]
    Content(AnyFileContentId, FileContents),
    #[allow(unused)]
    Tree(HgManifestId),
    #[allow(unused)]
    FileNode(HgFileNodeId),
    #[allow(unused)]
    HgChangeset(HgBlobChangeset, BonsaiChangeset),
}

#[async_trait]
pub trait ModernSyncSender {
    #[allow(unused)]
    async fn enqueue_entry(&self, entry: Entry) -> Result<()>;

    async fn upload_contents(&self, contents: Vec<(AnyFileContentId, FileContents)>) -> Result<()>;

    async fn upload_trees(&self, trees: Vec<HgManifestId>) -> Result<()>;

    async fn upload_filenodes(&self, filenodes: Vec<HgFileNodeId>) -> Result<()>;

    async fn upload_identical_changeset(
        &self,
        css: Vec<(HgBlobChangeset, BonsaiChangeset)>,
    ) -> Result<()>;

    async fn set_bookmark(
        &self,
        bookmark: String,
        from: Option<HgChangesetId>,
        to: Option<HgChangesetId>,
    ) -> Result<()>;
}
