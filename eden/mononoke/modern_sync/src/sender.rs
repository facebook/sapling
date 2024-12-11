/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use edenapi_types::UploadTreeEntry;
use mononoke_types::ContentId;
use mononoke_types::FileContents;
pub mod dummy;
pub mod edenapi;

#[async_trait]
pub trait ModernSyncSender {
    async fn upload_content(&self, content_id: ContentId, _blob: FileContents) -> Result<()>;

    #[allow(unused)]
    async fn upload_tree(&self, trees: Vec<UploadTreeEntry>) -> Result<()>;
}
