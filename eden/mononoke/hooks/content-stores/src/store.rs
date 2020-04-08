/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ErrorKind;

use anyhow::Error;

use async_trait::async_trait;
use bytes::Bytes;
use context::CoreContext;
use mercurial_types::{blobs::HgBlobChangeset, FileBytes, HgChangesetId, HgFileNodeId, MPath};
use mononoke_types::ContentId;
use mononoke_types::FileType;

#[derive(Clone, PartialEq, Eq)]
pub enum ChangedFileType {
    Added,
    Deleted,
    Modified,
}

#[async_trait]
pub trait FileContentFetcher: Send + Sync {
    async fn get_file_size<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind>;

    async fn get_file_text<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind>;
}

#[async_trait]
pub trait FileContentStore: Send + Sync {
    async fn resolve_path<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        changeset_id: HgChangesetId,
        path: MPath,
    ) -> Result<Option<HgFileNodeId>, Error>;

    async fn get_file_text<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: HgFileNodeId,
    ) -> Result<Option<FileBytes>, Error>;

    async fn get_file_size<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: HgFileNodeId,
    ) -> Result<u64, Error>;
}

#[async_trait]
pub trait ChangesetStore: Send + Sync {
    async fn get_changeset_by_changesetid<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        changesetid: HgChangesetId,
    ) -> Result<HgBlobChangeset, Error>;

    async fn get_changed_files<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        changesetid: HgChangesetId,
    ) -> Result<Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>, Error>;
}
