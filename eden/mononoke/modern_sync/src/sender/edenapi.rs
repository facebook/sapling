/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use edenapi_types::AnyFileContentId;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::blobs::HgBlobChangeset;
use minibytes::Bytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

mod config;
mod default;
mod filter;
mod noop;
mod retry;
mod util;

pub use config::EdenapiConfig;
pub(crate) use default::DefaultEdenapiSenderBuilder;
pub(crate) use filter::FilterEdenapiSender;
pub(crate) use filter::MethodFilter;
pub(crate) use noop::NoopEdenapiSender;
pub(crate) use retry::RetryEdenapiSender;

#[async_trait]
pub(crate) trait EdenapiSender {
    async fn upload_contents(&self, contents: Vec<(AnyFileContentId, Bytes)>) -> Result<()>;

    async fn upload_trees(&self, trees: Vec<HgManifestId>) -> Result<()>;

    async fn upload_filenodes(&self, fn_ids: Vec<HgFileNodeId>) -> Result<()>;

    async fn set_bookmark(
        &self,
        bookmark: String,
        from: Option<HgChangesetId>,
        to: Option<HgChangesetId>,
    ) -> Result<()>;

    async fn upload_identical_changeset(
        &self,
        css: Vec<(HgBlobChangeset, BonsaiChangeset)>,
    ) -> Result<()>;

    async fn filter_existing_commits(
        &self,
        ids: Vec<(HgChangesetId, ChangesetId)>,
    ) -> Result<Vec<ChangesetId>>;

    async fn read_bookmark(&self, bookmark: String) -> Result<Option<HgChangesetId>>;
}
