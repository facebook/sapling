/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::NonRootMPath;

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
}

#[derive(Debug, Clone)]
pub struct HeaderlessUnifiedDiff {
    pub raw_diff: Vec<u8>,
    pub is_binary: bool,
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
