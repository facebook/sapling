/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use context::CoreContext;
use failure_ext::Error;
use futures_ext::BoxFuture;
use mercurial_types::{blobs::HgBlobChangeset, FileBytes, HgChangesetId, HgFileNodeId, MPath};
use mononoke_types::FileType;

#[derive(Clone)]
pub enum ChangedFileType {
    Added,
    Deleted,
    Modified,
}

pub trait FileContentStore: Send + Sync {
    fn resolve_path(
        &self,
        ctx: CoreContext,
        changeset_id: HgChangesetId,
        path: MPath,
    ) -> BoxFuture<Option<HgFileNodeId>, Error>;

    fn get_file_text(
        &self,
        ctx: CoreContext,
        id: HgFileNodeId,
    ) -> BoxFuture<Option<FileBytes>, Error>;

    fn get_file_size(&self, ctx: CoreContext, id: HgFileNodeId) -> BoxFuture<u64, Error>;
}

pub trait ChangesetStore: Send + Sync {
    fn get_changeset_by_changesetid(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<HgBlobChangeset, Error>;

    fn get_changed_files(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>, Error>;
}
