/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use s_c_m_repo_metadata_v2_full_rust_logger::SCMRepoMetadataV2FullLogger;
use s_c_m_repo_metadata_v2_rust_logger::SCMRepoMetadataV2Logger;

use crate::types::BlamedTextFileMetadata;
use crate::types::ChangeType;
use crate::types::DirectoryMetadata;
use crate::types::FileMetadata;
use crate::types::ItemHistory;
use crate::types::MetadataItem;
use crate::types::SymlinkMetadata;
use crate::types::TextFileMetadata;

pub enum RepoMetadataLogger {
    /// V2 logger for incremental mode
    V2(SCMRepoMetadataV2Logger),
    /// V2 Full logger for full mode
    V2Full(SCMRepoMetadataV2FullLogger),
}

macro_rules! delegate_setter {
    ($method:ident, $param:ident: $param_ty:ty) => {
        pub fn $method(&mut self, $param: $param_ty) {
            match self {
                Self::V2(logger) => {
                    logger.$method($param);
                }
                Self::V2Full(logger) => {
                    logger.$method($param);
                }
            }
        }
    };
}

macro_rules! delegate_result_method {
    ($method:ident) => {
        pub fn $method(&mut self) -> anyhow::Result<()> {
            match self {
                Self::V2(logger) => logger.$method()?,
                Self::V2Full(logger) => logger.$method()?,
            }
            Ok(())
        }
    };
}

impl RepoMetadataLogger {
    // Delegate all setter methods using the macro
    delegate_setter!(set_repo_name, repo_name: String);
    delegate_setter!(set_last_author, author: String);
    delegate_setter!(set_last_modified_timestamp, timestamp: i64);
    delegate_setter!(set_is_file, is_file: bool);
    delegate_setter!(set_is_directory, is_directory: bool);
    delegate_setter!(set_is_symlink, is_symlink: bool);
    delegate_setter!(set_path, path: String);
    delegate_setter!(set_file_size, size: i64);
    delegate_setter!(set_executable, executable: bool);
    delegate_setter!(set_child_files_count, count: i64);
    delegate_setter!(set_child_files_total_size, size: i64);
    delegate_setter!(set_child_dirs_count, count: i64);
    delegate_setter!(set_descendant_files_count, count: i64);
    delegate_setter!(set_descendant_files_total_size, size: i64);
    delegate_setter!(set_is_ascii, is_ascii: bool);
    delegate_setter!(set_is_utf8, is_utf8: bool);
    delegate_setter!(set_line_count, count: i64);
    delegate_setter!(set_ends_in_newline, ends_in_newline: bool);
    delegate_setter!(set_newline_count, count: i64);
    delegate_setter!(set_is_generated, is_generated: bool);
    delegate_setter!(set_is_partially_generated, is_partially_generated: bool);
    delegate_setter!(set_approx_commit_count, count: i64);
    delegate_setter!(set_distinct_range_count, count: i64);
    delegate_setter!(set_historical_paths, paths: Vec<String>);
    delegate_setter!(set_historical_authors, authors: Vec<String>);
    delegate_setter!(set_modified_timestamps, timestamps: Vec<i64>);
    delegate_setter!(set_symlink_target, target: String);
    delegate_setter!(set_is_added, is_added: bool);
    delegate_setter!(set_is_deleted, is_deleted: bool);
    delegate_setter!(set_is_modified, is_modified: bool);

    delegate_result_method!(log_async);
}

impl MetadataItem {
    pub fn set_fields(&self, logger: &mut RepoMetadataLogger) {
        match self {
            MetadataItem::Directory(metadata) => {
                metadata.set_fields(logger);
            }
            MetadataItem::BinaryFile(metadata) => {
                metadata.set_fields(logger);
            }
            MetadataItem::TextFile(metadata) => {
                metadata.set_fields(logger);
            }
            MetadataItem::BlamedTextFile(metadata) => {
                metadata.set_fields(logger);
            }
            MetadataItem::Symlink(metadata) => {
                metadata.set_fields(logger);
            }
        }
    }
}

impl ItemHistory {
    fn set_fields(&self, logger: &mut RepoMetadataLogger) {
        logger.set_last_author(self.last_author.to_string());
        logger.set_last_modified_timestamp(self.last_modified_timestamp.timestamp_secs());
    }
}

impl FileMetadata {
    fn set_fields(&self, logger: &mut RepoMetadataLogger) {
        logger.set_is_file(true);
        logger.set_is_directory(false);

        logger.set_path(self.path.to_string());
        self.history.set_fields(logger);
        logger.set_file_size(self.file_size as i64);
        logger.set_executable(self.is_executable);
        self.change_type.set_fields(logger);
    }
}

impl DirectoryMetadata {
    fn set_fields(&self, logger: &mut RepoMetadataLogger) {
        logger.set_is_file(false);
        logger.set_is_directory(true);
        logger.set_is_symlink(false);

        logger.set_path(self.path.to_string());
        self.history.set_fields(logger);
        logger.set_child_files_count(self.child_files_count as i64);
        logger.set_child_files_total_size(self.child_files_total_size as i64);
        logger.set_child_dirs_count(self.child_dirs_count as i64);
        logger.set_descendant_files_count(self.descendant_files_count as i64);
        logger.set_descendant_files_total_size(self.descendant_files_total_size as i64);
        self.change_type.set_fields(logger);
    }
}

impl TextFileMetadata {
    fn set_fields(&self, logger: &mut RepoMetadataLogger) {
        logger.set_is_symlink(false);

        self.file_metadata.set_fields(logger);
        logger.set_is_ascii(self.is_ascii);
        logger.set_is_utf8(self.is_utf8);
        logger.set_line_count(self.line_count as i64);
        logger.set_ends_in_newline(self.ends_in_newline);
        logger.set_newline_count(self.newline_count as i64);
        logger.set_is_generated(self.is_generated);
        logger.set_is_partially_generated(self.is_partially_generated);
    }
}

impl BlamedTextFileMetadata {
    fn set_fields(&self, logger: &mut RepoMetadataLogger) {
        self.text_file_metadata.set_fields(logger);
        logger.set_approx_commit_count(self.approx_commit_count as i64);
        logger.set_distinct_range_count(self.distinct_range_count as i64);
        logger.set_historical_paths(
            self.historical_paths
                .iter()
                .map(|p| p.to_string())
                .collect(),
        );
        logger.set_historical_authors(self.historical_authors.clone());
        logger.set_modified_timestamps(
            self.modified_timestamps
                .iter()
                .map(|t| t.timestamp_secs())
                .collect(),
        );
    }
}

impl SymlinkMetadata {
    fn set_fields(&self, logger: &mut RepoMetadataLogger) {
        logger.set_is_symlink(true);

        self.file_metadata.set_fields(logger);
        logger.set_symlink_target(self.symlink_target.clone());
    }
}

impl ChangeType {
    fn set_fields(&self, logger: &mut RepoMetadataLogger) {
        match self {
            ChangeType::Unknown => {}
            ChangeType::Added => {
                logger.set_is_added(true);
            }
            ChangeType::Deleted => {
                logger.set_is_deleted(true);
            }
            ChangeType::Modified => {
                logger.set_is_modified(true);
            }
        }
    }
}
