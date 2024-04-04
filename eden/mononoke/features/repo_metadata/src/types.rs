/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Result;
use bytes::Bytes;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use dedupmap::DedupMap;
use futures::stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use mononoke_types::blame_v2::BlameData;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::path::MPath;
use mononoke_types::path::NonRootMPath;
use mononoke_types::ContentMetadataV2;
use mononoke_types::DateTime;
use mononoke_types::FileType;

use crate::Repo;

#[derive(Debug)]
pub enum MetadataItem {
    Directory(DirectoryMetadata),
    BinaryFile(FileMetadata),
    TextFile(TextFileMetadata),
    BlamedTextFile(BlamedTextFileMetadata),
    Symlink(SymlinkMetadata),
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

#[derive(Debug)]
pub struct BlamedTextFileMetadata {
    pub text_file_metadata: TextFileMetadata,
    /// Approx count of commits that modified this file
    pub approx_commit_count: u32,
    /// Number of distinct blame ranges
    pub distinct_range_count: usize,
    /// List of all historical paths this file has been located at. Only includes paths for
    /// changes that are in the current file.
    pub historical_paths: Vec<NonRootMPath>,
    /// List of authors who modified the file. Only includes authors whose
    /// changes are in the current file.
    pub historical_authors: Vec<String>,
    /// List of dates at which the file was modified. Only includes
    /// modifications that resulted in a current line.
    pub modified_timestamps: Vec<DateTime>,
}

impl MetadataItem {
    pub fn is_root(&self) -> bool {
        if let MetadataItem::Directory(DirectoryMetadata { path, .. }) = self {
            if path.is_root() {
                return true;
            }
        }
        false
    }
}

#[derive(Debug)]
pub struct SymlinkMetadata {
    pub file_metadata: FileMetadata,
    /// The target path
    pub symlink_target: String,
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

impl SymlinkMetadata {
    pub(crate) fn new(file_metadata: FileMetadata, content: Bytes) -> Self {
        Self {
            file_metadata,
            symlink_target: String::from_utf8_lossy(&content).to_string(),
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

impl BlamedTextFileMetadata {
    pub(crate) async fn new(
        ctx: &CoreContext,
        repo: &impl Repo,
        text_file_metadata: TextFileMetadata,
        blame: BlameData,
    ) -> Result<Self> {
        let cs_ids = blame.csid_index();
        let info: HashMap<_, _> = stream::iter(cs_ids.values())
            .map(|cs_id| {
                repo.repo_derived_data()
                    .derive::<ChangesetInfo>(ctx, *cs_id)
                    .map_ok(|info| (*cs_id, info))
            })
            .boxed()
            .buffered(200)
            .try_collect()
            .await?;

        let mut historical_paths: DedupMap<NonRootMPath> = Default::default();
        let mut historical_authors: DedupMap<String> = Default::default();
        let mut modified_timestamps: DedupMap<DateTime> = Default::default();

        for blame_line in blame.lines() {
            historical_paths.insert(Cow::Borrowed(blame_line.path));

            let line_info = info.get(blame_line.changeset_id).ok_or_else(|| {
                anyhow!(
                    "Failed to get changeset info for changeset id: {:?}",
                    blame_line.changeset_id
                )
            })?;
            historical_authors.insert(line_info.author());
            modified_timestamps.insert(Cow::Borrowed(line_info.author_date()));
        }

        Ok(Self {
            text_file_metadata,
            approx_commit_count: blame.changeset_count(),
            distinct_range_count: blame.ranges().len(),
            historical_paths: historical_paths.into_items(),
            historical_authors: historical_authors.into_items(),
            modified_timestamps: modified_timestamps.into_items(),
        })
    }
}
