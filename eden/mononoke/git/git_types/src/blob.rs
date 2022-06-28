/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;

use mononoke_types::hash::RichGitSha1;
use mononoke_types::ContentMetadata;
use mononoke_types::FileType;

use crate::mode;
use crate::thrift;
use crate::ObjectKind;

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct BlobHandle {
    oid: RichGitSha1,
    file_type: FileType,
}

impl BlobHandle {
    pub fn new(metadata: ContentMetadata, file_type: FileType) -> Self {
        Self {
            oid: metadata.git_sha1,
            file_type,
        }
    }

    pub fn filemode(&self) -> i32 {
        match self.file_type {
            FileType::Regular => mode::GIT_FILEMODE_BLOB,
            FileType::Executable => mode::GIT_FILEMODE_BLOB_EXECUTABLE,
            FileType::Symlink => mode::GIT_FILEMODE_LINK,
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

impl Into<thrift::BlobHandle> for BlobHandle {
    fn into(self) -> thrift::BlobHandle {
        let size = self.oid.size();

        thrift::BlobHandle {
            oid: self.oid.into_thrift(),
            size: size
                .try_into()
                .expect("Blob size must fit in a i64 for Thrift serialization"),
            file_type: self.file_type.into_thrift(),
        }
    }
}
