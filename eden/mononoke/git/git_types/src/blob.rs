/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::Blobstore;
use context::CoreContext;
use filestore::FetchKey;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::BasicFileChange;
use mononoke_types::ContentId;
use mononoke_types::FileType;

use crate::errors::MononokeGitError;
use crate::mode;
use crate::thrift;
use crate::ObjectKind;

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct BlobHandle {
    oid: RichGitSha1,
    file_type: FileType,
}

impl BlobHandle {
    pub async fn new<B: Blobstore + Clone>(
        ctx: &CoreContext,
        blobstore: &B,
        file_change: &BasicFileChange,
    ) -> Result<Self, Error> {
        let file_type = file_change.file_type();

        let oid = if file_type == FileType::GitSubmodule {
            let bytes =
                filestore::fetch_concat_exact(blobstore, ctx, file_change.content_id(), 20).await?;
            RichGitSha1::from_bytes(&bytes, ObjectKind::Commit.as_str(), 0)?
        } else {
            let key = FetchKey::Canonical(file_change.content_id());
            let metadata = filestore::get_metadata(blobstore, ctx, &key)
                .await?
                .ok_or(MononokeGitError::ContentMissing(key))?;
            metadata.git_sha1
        };

        Ok(Self { oid, file_type })
    }

    pub fn from_oid_and_file_type(oid: RichGitSha1, file_type: FileType) -> Self {
        Self { oid, file_type }
    }

    pub async fn from_content_id_and_file_type<B: Blobstore>(
        ctx: &CoreContext,
        blobstore: &B,
        content_id: ContentId,
        file_type: FileType,
    ) -> Result<Self, Error> {
        let key = FetchKey::Canonical(content_id);
        let metadata = filestore::get_metadata(blobstore, ctx, &key)
            .await?
            .ok_or(MononokeGitError::ContentMissing(key))?;
        Ok(Self {
            oid: metadata.git_sha1,
            file_type,
        })
    }

    pub fn filemode(&self) -> i32 {
        match self.file_type {
            FileType::Regular => mode::GIT_FILEMODE_BLOB,
            FileType::Executable => mode::GIT_FILEMODE_BLOB_EXECUTABLE,
            FileType::Symlink => mode::GIT_FILEMODE_LINK,
            FileType::GitSubmodule => mode::GIT_FILEMODE_COMMIT,
        }
    }

    pub fn oid(&self) -> &RichGitSha1 {
        &self.oid
    }
}

impl TryFrom<thrift::BlobHandle> for BlobHandle {
    type Error = Error;

    fn try_from(t: thrift::BlobHandle) -> Result<Self, Error> {
        let size = t.size.try_into()?;
        let oid = RichGitSha1::from_bytes(&t.oid.0, ObjectKind::Blob.as_str(), size)?;

        Ok(Self {
            oid,
            file_type: FileType::from_thrift(t.file_type)?,
        })
    }
}

impl From<BlobHandle> for thrift::BlobHandle {
    fn from(bh: BlobHandle) -> thrift::BlobHandle {
        let size = bh.oid.size();

        thrift::BlobHandle {
            oid: bh.oid.into_thrift(),
            size: size
                .try_into()
                .expect("Blob size must fit in a i64 for Thrift serialization"),
            file_type: bh.file_type.into_thrift(),
        }
    }
}
