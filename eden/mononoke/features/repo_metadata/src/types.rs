/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use changeset_info::ChangesetInfo;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::path::MPath;
use mononoke_types::ContentMetadataV2;
use mononoke_types::DateTime;
use mononoke_types::FileType;

#[derive(Debug)]
pub enum MetadataItem {
    Unknown,
    Directory(DirectoryMetadata),
    BinaryFile(FileMetadata),
    TextFile(TextFileMetadata),
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

#[derive(Debug)]
pub struct TextFileMetadata {
    pub file_metadata: FileMetadata,
    /// Is the file all ASCII
    pub is_ascii: bool,
    /// Is the file valid UTF-8
    pub is_utf8: bool,
    /// The number of lines in the file
    pub line_count: u64,
    /// True if this file ends in a newline character
    pub ends_in_newline: bool,
    /// The number of newline characters in this file
    pub newline_count: u64,
    /// Does the file contain the generated-content marker.
    pub is_generated: bool,
    /// Does the file contain the partially-generated-content marker.
    pub is_partially_generated: bool,
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

impl TextFileMetadata {
    pub(crate) fn new(file_metadata: FileMetadata, content_metadata: ContentMetadataV2) -> Self {
        let line_count = if file_metadata.file_size == 0 {
            0
        } else if content_metadata.ends_in_newline {
            content_metadata.newline_count
        } else {
            content_metadata.newline_count + 1
        };
        Self {
            file_metadata,
            is_ascii: content_metadata.is_ascii,
            is_utf8: content_metadata.is_utf8,
            line_count,
            ends_in_newline: content_metadata.ends_in_newline,
            newline_count: content_metadata.newline_count,
            is_generated: content_metadata.is_generated,
            is_partially_generated: content_metadata.is_partially_generated,
        }
    }
}
