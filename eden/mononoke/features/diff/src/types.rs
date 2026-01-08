/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use metaconfig_types::RepoConfigRef;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;

pub trait Repo = RepoBlobstoreArc + RepoConfigRef + RepoDerivedDataRef + Send + Sync;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFileType {
    Regular,
    Executable,
    Symlink,
    GitSubmodule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffCopyInfo {
    None,
    Move,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffContentType {
    Text,
    NonUtf8,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffGeneratedStatus {
    NonGenerated,
    Partially,
    Fully,
}

#[derive(Debug, Clone)]
pub enum DiffFileContent {
    Inline(Bytes),
    Omitted {
        content_hash: String,
        git_lfs_pointer: Option<String>,
    },
    Submodule {
        commit_hash: String,
    },
}

#[derive(Debug, Clone)]
pub struct HeaderlessDiffOpts {
    pub context: usize,
    pub ignore_whitespace: bool,
}

#[derive(Debug, Clone)]
pub struct HeaderlessUnifiedDiff {
    pub raw_diff: Vec<u8>,
    pub is_binary: bool,
}

#[derive(Debug, Clone)]
pub struct HunkRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct HunkData {
    pub add_range: HunkRange,
    pub delete_range: HunkRange,
}

#[derive(Debug, Clone)]
pub struct MetadataFileInfo {
    pub file_type: Option<DiffFileType>,
    pub content_type: Option<DiffContentType>,
    pub generated_status: Option<DiffGeneratedStatus>,
}

#[derive(Debug, Clone)]
pub struct MetadataLinesCount {
    pub added_lines: i64,
    pub deleted_lines: i64,
    pub significant_added_lines: i64,
    pub significant_deleted_lines: i64,
    pub first_added_line_number: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct MetadataDiff {
    pub base_file_info: MetadataFileInfo,
    pub other_file_info: MetadataFileInfo,
    pub lines_count: Option<MetadataLinesCount>,
}

impl From<xdiff::Hunk> for HunkData {
    fn from(hunk: xdiff::Hunk) -> Self {
        let add_range = HunkRange {
            start: hunk.add.start,
            end: hunk.add.end,
        };

        let delete_range = HunkRange {
            start: hunk.remove.start,
            end: hunk.remove.end,
        };

        HunkData {
            add_range,
            delete_range,
        }
    }
}

impl From<DiffFileType> for xdiff::FileType {
    fn from(file_type: DiffFileType) -> Self {
        match file_type {
            DiffFileType::Regular => xdiff::FileType::Regular,
            DiffFileType::Executable => xdiff::FileType::Executable,
            DiffFileType::Symlink => xdiff::FileType::Symlink,
            DiffFileType::GitSubmodule => xdiff::FileType::GitSubmodule,
        }
    }
}

impl From<xdiff::FileType> for DiffFileType {
    fn from(file_type: xdiff::FileType) -> Self {
        match file_type {
            xdiff::FileType::Regular => DiffFileType::Regular,
            xdiff::FileType::Executable => DiffFileType::Executable,
            xdiff::FileType::Symlink => DiffFileType::Symlink,
            xdiff::FileType::GitSubmodule => DiffFileType::GitSubmodule,
        }
    }
}

impl From<DiffCopyInfo> for xdiff::CopyInfo {
    fn from(copy_info: DiffCopyInfo) -> Self {
        match copy_info {
            DiffCopyInfo::None => xdiff::CopyInfo::None,
            DiffCopyInfo::Move => xdiff::CopyInfo::Move,
            DiffCopyInfo::Copy => xdiff::CopyInfo::Copy,
        }
    }
}

impl From<xdiff::CopyInfo> for DiffCopyInfo {
    fn from(copy_info: xdiff::CopyInfo) -> Self {
        match copy_info {
            xdiff::CopyInfo::None => DiffCopyInfo::None,
            xdiff::CopyInfo::Move => DiffCopyInfo::Move,
            xdiff::CopyInfo::Copy => DiffCopyInfo::Copy,
        }
    }
}

impl From<DiffFileContent> for xdiff::FileContent<Bytes> {
    fn from(content: DiffFileContent) -> Self {
        match content {
            DiffFileContent::Inline(bytes) => xdiff::FileContent::Inline(bytes),
            DiffFileContent::Omitted {
                content_hash,
                git_lfs_pointer,
            } => xdiff::FileContent::Omitted {
                content_hash,
                git_lfs_pointer,
            },
            DiffFileContent::Submodule { commit_hash } => {
                xdiff::FileContent::Submodule { commit_hash }
            }
        }
    }
}

impl From<xdiff::FileContent<Bytes>> for DiffFileContent {
    fn from(content: xdiff::FileContent<Bytes>) -> Self {
        match content {
            xdiff::FileContent::Inline(bytes) => DiffFileContent::Inline(bytes),
            xdiff::FileContent::Omitted {
                content_hash,
                git_lfs_pointer,
            } => DiffFileContent::Omitted {
                content_hash,
                git_lfs_pointer,
            },
            xdiff::FileContent::Submodule { commit_hash } => {
                DiffFileContent::Submodule { commit_hash }
            }
        }
    }
}

impl From<HeaderlessDiffOpts> for xdiff::HeaderlessDiffOpts {
    fn from(opts: HeaderlessDiffOpts) -> Self {
        xdiff::HeaderlessDiffOpts {
            context: opts.context,
            // Note: xdiff doesn't support ignore_whitespace, so we handle it at the content loading level
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsPointer {
    pub sha256: String,
    pub size: i64,
}

#[derive(Debug, Clone)]
pub enum DiffSingleInput {
    ChangesetPath(DiffInputChangesetPath),
    Content(DiffInputContent),
    String(DiffInputString),
}

#[derive(Debug, Clone)]
pub struct DiffInputString {
    pub content: String,
}
#[derive(Debug, Clone)]
pub struct DiffInputChangesetPath {
    pub changeset_id: ChangesetId,
    pub path: NonRootMPath,
    pub replacement_path: Option<NonRootMPath>,
}

#[derive(Debug, Clone)]
pub struct DiffInputContent {
    pub content_id: ContentId,
    pub path: Option<NonRootMPath>,
    pub lfs_pointer: Option<LfsPointer>,
}

#[derive(Debug, Clone)]
pub struct UnifiedDiffOpts {
    pub context: usize,
    pub copy_info: DiffCopyInfo,
    pub file_type: DiffFileType,
    pub inspect_lfs_pointers: bool,
    pub omit_content: bool,
    pub ignore_whitespace: bool,
}

#[derive(Debug, Clone)]
pub struct UnifiedDiff {
    pub raw_diff: Vec<u8>,
    pub is_binary: bool,
}

impl From<UnifiedDiffOpts> for xdiff::DiffOpts {
    fn from(opts: UnifiedDiffOpts) -> Self {
        xdiff::DiffOpts {
            context: opts.context,
            copy_info: opts.copy_info.into(),
        }
    }
}
