/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use async_trait::async_trait;
use futures_util::try_join;
use mononoke_api::{
    ChangesetPathContext, ChangesetPathDiffContext, FileMetadata, FileType, TreeEntry, UnifiedDiff,
};
use source_control as thrift;

use crate::errors;

pub(crate) trait IntoResponse<T> {
    fn into_response(self) -> T;
}

impl IntoResponse<thrift::EntryType> for FileType {
    fn into_response(self) -> thrift::EntryType {
        match self {
            FileType::Regular => thrift::EntryType::FILE,
            FileType::Executable => thrift::EntryType::EXEC,
            FileType::Symlink => thrift::EntryType::LINK,
        }
    }
}

impl IntoResponse<thrift::TreeEntry> for (String, TreeEntry) {
    fn into_response(self) -> thrift::TreeEntry {
        let (name, entry) = self;
        let (type_, info) = match entry {
            TreeEntry::Directory(dir) => {
                let summary = dir.summary();
                let info = thrift::TreeInfo {
                    id: dir.id().as_ref().to_vec(),
                    simple_format_sha1: summary.simple_format_sha1.as_ref().to_vec(),
                    simple_format_sha256: summary.simple_format_sha256.as_ref().to_vec(),
                    child_files_count: summary.child_files_count as i64,
                    child_files_total_size: summary.child_files_total_size as i64,
                    child_dirs_count: summary.child_dirs_count as i64,
                    descendant_files_count: summary.descendant_files_count as i64,
                    descendant_files_total_size: summary.descendant_files_total_size as i64,
                };
                (thrift::EntryType::TREE, thrift::EntryInfo::tree(info))
            }
            TreeEntry::File(file) => {
                let info = thrift::FileInfo {
                    id: file.content_id().as_ref().to_vec(),
                    file_size: file.size() as i64,
                    content_sha1: file.content_sha1().as_ref().to_vec(),
                    content_sha256: file.content_sha256().as_ref().to_vec(),
                };
                (
                    file.file_type().into_response(),
                    thrift::EntryInfo::file(info),
                )
            }
        };
        thrift::TreeEntry { name, type_, info }
    }
}

impl IntoResponse<thrift::FileInfo> for FileMetadata {
    fn into_response(self) -> thrift::FileInfo {
        thrift::FileInfo {
            id: self.content_id.as_ref().to_vec(),
            file_size: self.total_size as i64,
            content_sha1: self.sha1.as_ref().to_vec(),
            content_sha256: self.sha256.as_ref().to_vec(),
        }
    }
}

impl IntoResponse<thrift::Diff> for UnifiedDiff {
    fn into_response(self) -> thrift::Diff {
        thrift::Diff::raw_diff(thrift::RawDiff {
            raw_diff: Some(self.raw_diff),
            is_binary: self.is_binary,
        })
    }
}

#[async_trait]
pub(crate) trait AsyncIntoResponse<T> {
    async fn into_response(self) -> Result<T, errors::ServiceError>;
}

#[async_trait]
impl AsyncIntoResponse<Option<thrift::FilePathInfo>> for ChangesetPathContext {
    async fn into_response(self) -> Result<Option<thrift::FilePathInfo>, errors::ServiceError> {
        let (meta, type_) = try_join!(
            async {
                let file = self.file().await?;
                match file {
                    Some(file) => Ok(Some(file.metadata().await?)),
                    None => Ok(None),
                }
            },
            self.file_type()
        )?;
        if let (Some(meta), Some(type_)) = (meta, type_) {
            Ok(Some(thrift::FilePathInfo {
                path: self.path().to_string(),
                type_: type_.into_response(),
                info: meta.into_response(),
            }))
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl AsyncIntoResponse<thrift::CommitCompareFile> for ChangesetPathDiffContext {
    async fn into_response(self) -> Result<thrift::CommitCompareFile, errors::ServiceError> {
        let (other_file, base_file, copy_info) = match self {
            ChangesetPathDiffContext::Added(base_context) => {
                let entry = base_context.into_response().await?;
                (None, entry, thrift::CopyInfo::NONE)
            }
            ChangesetPathDiffContext::Removed(other_context) => {
                let entry = other_context.into_response().await?;
                (entry, None, thrift::CopyInfo::NONE)
            }
            ChangesetPathDiffContext::Changed(base_context, other_context) => {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                (other_entry, base_entry, thrift::CopyInfo::NONE)
            }
            ChangesetPathDiffContext::Copied(base_context, other_context) => {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                (other_entry, base_entry, thrift::CopyInfo::COPY)
            }
            ChangesetPathDiffContext::Moved(base_context, other_context) => {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                (other_entry, base_entry, thrift::CopyInfo::MOVE)
            }
        };
        Ok(thrift::CommitCompareFile {
            base_file,
            other_file,
            copy_info,
        })
    }
}
