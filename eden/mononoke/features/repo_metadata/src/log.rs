/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use s_c_m_repo_metadata_rust_logger::SCMRepoMetadataLogger;

use crate::types::BlamedTextFileMetadata;
use crate::types::DirectoryMetadata;
use crate::types::FileMetadata;
use crate::types::ItemHistory;
use crate::types::MetadataItem;
use crate::types::SymlinkMetadata;
use crate::types::TextFileMetadata;

impl MetadataItem {
    pub fn set_fields(&self, logger: &mut SCMRepoMetadataLogger) {
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
    fn set_fields(&self, logger: &mut SCMRepoMetadataLogger) {
        logger.set_last_author(self.last_author.to_string());
        logger.set_last_modified_timestamp(self.last_modified_timestamp.timestamp_secs());
    }
}

impl FileMetadata {
    fn set_fields(&self, logger: &mut SCMRepoMetadataLogger) {
        logger.set_is_file(true);
        logger.set_is_directory(false);

        logger.set_path(self.path.to_string());
        self.history.set_fields(logger);
        logger.set_file_size(self.file_size as i64);
        logger.set_executable(self.is_executable);
    }
}

impl DirectoryMetadata {
    fn set_fields(&self, logger: &mut SCMRepoMetadataLogger) {
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
    }
}

impl TextFileMetadata {
    fn set_fields(&self, logger: &mut SCMRepoMetadataLogger) {
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
    fn set_fields(&self, logger: &mut SCMRepoMetadataLogger) {
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
    fn set_fields(&self, logger: &mut SCMRepoMetadataLogger) {
        logger.set_is_symlink(true);

        self.file_metadata.set_fields(logger);
        logger.set_symlink_target(self.symlink_target.clone());
    }
}
