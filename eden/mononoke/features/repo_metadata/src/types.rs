/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use changeset_info::ChangesetInfo;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::path::MPath;
use mononoke_types::DateTime;
use mononoke_types::FileType;

#[derive(Debug)]
pub enum MetadataItem {
    Unknown,
    Directory(DirectoryMetadata),
    BinaryFile(FileMetadata),
}

#[derive(Debug)]
pub struct ItemHistory {
    /// The last time this item was modified
    pub last_author: String,
    /// The last author to modify this item
    pub last_modified_timestamp: DateTime,
}

#[derive(Debug)]
pub struct FileMetadata {
    /// The path of this file
    pub path: MPath,
    /// The history of this file
    pub history: ItemHistory,
    /// The size of this file in bytes
    pub file_size: u64,
    /// Whether this file is marked as executable
    pub is_executable: bool,
}

#[derive(Debug)]
pub struct DirectoryMetadata {
    /// The path of this directory
    pub path: MPath,
    /// The history of this directory
    pub history: ItemHistory,
    /// The number of files in this directory
    pub child_files_count: u64,
    /// The total size of the files in this directory
    pub child_files_total_size: u64,
    /// The number of subdirectories in this directory
    pub child_dirs_count: u64,
    /// The number of files in this directory and all of its recursive subdirectories
    pub descendant_files_count: u64,
    /// The total size of the files in this directory and all of its recursive subdirectories
    pub descendant_files_total_size: u64,
}

impl FileMetadata {
    pub(crate) fn new(path: MPath, info: ChangesetInfo, fsnode_file: FsnodeFile) -> Self {
        Self {
            path,
            history: ItemHistory {
                last_author: info.author().to_string(),
                last_modified_timestamp: *info.author_date(),
            },
            file_size: fsnode_file.size(),
            is_executable: *fsnode_file.file_type() == FileType::Executable,
        }
    }
}
