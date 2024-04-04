/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::path::MPath;
use mononoke_types::DateTime;

#[derive(Debug)]
pub enum MetadataItem {
    Unknown,
    Directory(DirectoryMetadata),
}

#[derive(Debug)]
pub struct ItemHistory {
    /// The last time this item was modified
    pub last_author: String,
    /// The last author to modify this item
    pub last_modified_timestamp: DateTime,
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
