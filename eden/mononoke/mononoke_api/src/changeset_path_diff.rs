/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use context::CoreContext;
use derivative::Derivative;
use diff::operations::metadata::metadata;
use diff::operations::unified::unified;
use diff::types::DiffCopyInfo;
use diff::types::DiffFileType;
use diff::types::DiffInputChangesetPath;
use diff::types::DiffSingleInput;
use diff::types::UnifiedDiffOpts;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
pub use xdiff::CopyInfo;

use crate::ChangesetContext;
use crate::MononokeRepo;
use crate::changeset_path::ChangesetPathContentContext;
use crate::errors::MononokeError;
use crate::file::FileType;

/// A path difference between two commits.
///
/// A ChangesetPathDiffContext shows the difference between a path in a
/// commit ("new") and its corresponding location in another commit ("old").
#[derive(Derivative)]
#[derivative(Clone, Debug(bound = ""))]
pub struct ChangesetPathDiffContext<R: MononokeRepo> {
    changeset: ChangesetContext<R>,
    path: MPath,
    is_tree: bool,
    /// If None, the path was deleted.
    new_content: Option<ChangesetPathContentContext<R>>,
    /// If None, the path was added.
    old_content: Option<ChangesetPathContentContext<R>>,
    /// Whether the file was marked as copied or moved.
    copy_info: CopyInfo,
    /// If the path was copied via subtree copy, this is the replacement path for the "old" file.
    subtree_copy_dest_path: Option<MPath>,
}

/// A diff between two files in extended unified diff format
pub struct UnifiedDiff {
    /// Raw diff as bytes.
    pub raw_diff: Vec<u8>,
    /// One of the diffed files is binary, raw diff contains just a placeholder.
    pub is_binary: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum UnifiedDiffMode {
    /// Unified diff is generated inline as normal.
    Inline,
    /// Content is not fetched - instead a placeholder diff like
    ///
    /// diff --git a/file.txt b/file.txt
    /// Binary file file.txt has changed
    ///
    /// is generated
    OmitContent,
}

/// Metadata about the differences between two files that is useful to
/// Phabricator.
pub struct MetadataDiff {
    /// Information about the file before the change.
    pub old_file_info: MetadataDiffFileInfo,

    /// Information about the file after the change.
    pub new_file_info: MetadataDiffFileInfo,

    /// Lines count in the diff between the two files.
    pub lines_count: Option<MetadataDiffLinesCount>,
}

/// File information that concerns the metadata diff.
pub struct MetadataDiffFileInfo {
    /// File type (file, exec, or link)
    pub file_type: Option<FileType>,

    /// File content type (text, non-utf8, or binary)
    pub file_content_type: Option<FileContentType>,

    /// File generated status (fully, partially, or not generated)
    pub file_generated_status: Option<FileGeneratedStatus>,
}

/// Lines count in a diff for the metadata diff.
#[derive(Default)]
pub struct MetadataDiffLinesCount {
    /// Number of added lines.
    pub added_lines_count: usize,

    /// Number of deleted lines.
    pub deleted_lines_count: usize,

    /// Number of significant (not generated) added lines.
    pub significant_added_lines_count: usize,

    /// Number of significant (not generated) deleted lines.
    pub significant_deleted_lines_count: usize,

    /// Line number of the first added line (1-based).
    pub first_added_line_number: Option<usize>,
}

pub enum FileContentType {
    Text,
    NonUtf8,
    Binary,
}

pub enum FileGeneratedStatus {
    /// File is fully generated.
    FullyGenerated,
    /// File is partially generated.
    PartiallyGenerated,
    /// File is not generated.
    NotGenerated,
}

impl<R: MononokeRepo> ChangesetPathDiffContext<R> {
    /// Create a new path diff context that compares the contents of two
    /// changeset paths.
    ///
    /// Copy information must be provided if the file has been copied or
    /// moved.
    pub fn new_file(
        changeset: ChangesetContext<R>,
        path: MPath,
        new_content: Option<ChangesetPathContentContext<R>>,
        old_content: Option<ChangesetPathContentContext<R>>,
        copy_info: CopyInfo,
        subtree_copy_dest_path: Option<MPath>,
    ) -> Result<Self, MononokeError> {
        if copy_info != CopyInfo::None && (new_content.is_none() || old_content.is_none())
            || (new_content.is_none() && old_content.is_none())
        {
            return Err(anyhow!(
                "Invalid changeset path diff context parameters: {:?}",
                (new_content, old_content, copy_info)
            )
            .into());
        }
        Ok(Self {
            changeset,
            path,
            is_tree: false,
            new_content,
            old_content,
            copy_info,
            subtree_copy_dest_path,
        })
    }

    /// Create a new path diff context that compares the contents of two
    /// changeset paths that are trees.
    pub fn new_tree(
        changeset: ChangesetContext<R>,
        path: MPath,
        new_content: Option<ChangesetPathContentContext<R>>,
        old_content: Option<ChangesetPathContentContext<R>>,
        subtree_copy_dest_path: Option<MPath>,
    ) -> Result<Self, MononokeError> {
        if new_content.is_none() && old_content.is_none() {
            return Err(anyhow!(
                "Invalid changeset path diff context parameters: {:?}",
                (new_content, old_content)
            )
            .into());
        }
        Ok(Self {
            changeset,
            path,
            is_tree: true,
            new_content,
            old_content,
            copy_info: CopyInfo::None,
            subtree_copy_dest_path,
        })
    }

    /// Return the changeset that this path is being compared in.
    pub fn changeset(&self) -> &ChangesetContext<R> {
        &self.changeset
    }

    pub fn subtree_copy_dest_path(&self) -> Option<&MPath> {
        self.subtree_copy_dest_path.as_ref()
    }

    /// Return the new path content that is being compared.  This is the
    /// contents after modification.
    pub fn get_new_content(&self) -> Option<&ChangesetPathContentContext<R>> {
        self.new_content.as_ref()
    }

    /// Return the old path content that is being compared against.  This
    /// is the contents before modification.
    pub fn get_old_content(&self) -> Option<&ChangesetPathContentContext<R>> {
        self.old_content.as_ref()
    }

    /// Return the main path for this difference.  This is the added,
    /// removed or changed path, or the new path (destination) in the
    /// case of copies, or moves.
    pub fn path(&self) -> &MPath {
        &self.path
    }

    /// Return the file copy information for this difference.
    pub fn copy_info(&self) -> CopyInfo {
        self.copy_info.clone()
    }

    pub fn is_tree(&self) -> bool {
        self.is_tree
    }

    pub fn is_file(&self) -> bool {
        !self.is_tree
    }

    /// Converts a ChangesetPathContentContext to DiffSingleInput for use with the diff crate
    fn convert_changeset_path_to_diff_input(
        path: Option<&ChangesetPathContentContext<R>>,
        replacement_path: Option<&MPath>,
    ) -> Option<DiffSingleInput> {
        path.and_then(|p| {
            // Use the original path from the context
            let original_path = p.path();

            // Convert the original MPath to NonRootMPath for the path field
            let non_root_path = NonRootMPath::try_from(original_path.clone()).ok()?;

            // Convert replacement_path if provided
            let replacement_non_root_path =
                replacement_path.and_then(|rp| NonRootMPath::try_from(rp.clone()).ok());

            Some(DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
                changeset_id: p.changeset().id(),
                path: non_root_path,
                replacement_path: replacement_non_root_path,
            }))
        })
    }

    /// Renders the diff (in the git diff format).
    ///
    /// If `mode` is `Placeholder` then `unified_diff(...)` doesn't fetch content,
    /// but just generates a placeholder diff that says that the files differ.
    ///
    /// If `ignore_whitespace` is true, horizontal whitespace (spaces, tabs, carriage returns)
    /// will be stripped before computing the diff.
    pub async fn unified_diff(
        &self,
        ctx: &CoreContext,
        context_lines: usize,
        mode: UnifiedDiffMode,
        ignore_whitespace: bool,
    ) -> Result<UnifiedDiff, MononokeError> {
        // Convert changeset path contexts to DiffSingleInput for the diff crate
        let new_input = Self::convert_changeset_path_to_diff_input(self.get_new_content(), None);
        let old_input = Self::convert_changeset_path_to_diff_input(
            self.get_old_content(),
            self.subtree_copy_dest_path.as_ref(),
        );

        // Determine file type from the new content
        let file_type = if let Some(new_content) = self.get_new_content() {
            if let Ok(Some(ft)) = new_content.file_type().await {
                match ft {
                    FileType::Regular => DiffFileType::Regular,
                    FileType::Executable => DiffFileType::Executable,
                    FileType::Symlink => DiffFileType::Symlink,
                    FileType::GitSubmodule => DiffFileType::GitSubmodule,
                }
            } else {
                DiffFileType::Regular
            }
        } else {
            DiffFileType::Regular
        };

        // Convert copy info from xdiff to diff crate format
        let copy_info = match self.copy_info() {
            CopyInfo::None => DiffCopyInfo::None,
            CopyInfo::Move => DiffCopyInfo::Move,
            CopyInfo::Copy => DiffCopyInfo::Copy,
        };

        // Create unified diff options
        let options = UnifiedDiffOpts {
            context: context_lines,
            copy_info,
            file_type,
            inspect_lfs_pointers: false,
            omit_content: mode == UnifiedDiffMode::OmitContent,
            ignore_whitespace,
        };

        // Call the unified function from the diff crate
        let diff_result = unified(
            ctx,
            self.changeset.repo_ctx().repo(),
            old_input,
            new_input,
            options,
        )
        .await
        .map_err(|e| MononokeError::from(anyhow::anyhow!("Diff error: {}", e)))?;

        Ok(UnifiedDiff {
            raw_diff: diff_result.raw_diff,
            is_binary: diff_result.is_binary,
        })
    }

    /// Computes metadata about the differences between two files.
    ///
    /// If `ignore_whitespace` is true, horizontal whitespace (spaces, tabs, carriage returns)
    /// will be stripped before computing line counts.
    pub async fn metadata_diff(
        &self,
        ctx: &CoreContext,
        ignore_whitespace: bool,
    ) -> Result<MetadataDiff, MononokeError> {
        let new_input = Self::convert_changeset_path_to_diff_input(self.get_new_content(), None);
        let old_input = Self::convert_changeset_path_to_diff_input(
            self.get_old_content(),
            self.subtree_copy_dest_path.as_ref(),
        );

        let diff_metadata = metadata(
            ctx,
            self.changeset.repo_ctx().repo(),
            old_input,
            new_input,
            ignore_whitespace,
        )
        .await
        .map_err(|e| MononokeError::from(anyhow::anyhow!("Metadata diff error: {e:#}")))?;

        Ok(MetadataDiff {
            old_file_info: Self::convert_metadata_file_info(&diff_metadata.base_file_info),
            new_file_info: Self::convert_metadata_file_info(&diff_metadata.other_file_info),
            lines_count: diff_metadata
                .lines_count
                .map(Self::convert_metadata_lines_count),
        })
    }

    fn convert_metadata_file_info(
        diff_file_info: &diff::types::MetadataFileInfo,
    ) -> MetadataDiffFileInfo {
        MetadataDiffFileInfo {
            file_type: diff_file_info
                .file_type
                .map(Self::convert_diff_file_type_to_api),
            file_content_type: diff_file_info
                .content_type
                .map(Self::convert_diff_content_type_to_api),
            file_generated_status: diff_file_info
                .generated_status
                .map(Self::convert_diff_generated_status_to_api),
        }
    }

    fn convert_metadata_lines_count(
        diff_lines_count: diff::types::MetadataLinesCount,
    ) -> MetadataDiffLinesCount {
        MetadataDiffLinesCount {
            added_lines_count: diff_lines_count.added_lines as usize,
            deleted_lines_count: diff_lines_count.deleted_lines as usize,
            significant_added_lines_count: diff_lines_count.significant_added_lines as usize,
            significant_deleted_lines_count: diff_lines_count.significant_deleted_lines as usize,
            first_added_line_number: diff_lines_count.first_added_line_number.map(|n| n as usize),
        }
    }

    fn convert_diff_file_type_to_api(diff_file_type: diff::types::DiffFileType) -> FileType {
        match diff_file_type {
            diff::types::DiffFileType::Regular => FileType::Regular,
            diff::types::DiffFileType::Executable => FileType::Executable,
            diff::types::DiffFileType::Symlink => FileType::Symlink,
            diff::types::DiffFileType::GitSubmodule => FileType::GitSubmodule,
        }
    }

    fn convert_diff_content_type_to_api(
        diff_content_type: diff::types::DiffContentType,
    ) -> FileContentType {
        match diff_content_type {
            diff::types::DiffContentType::Text => FileContentType::Text,
            diff::types::DiffContentType::NonUtf8 => FileContentType::NonUtf8,
            diff::types::DiffContentType::Binary => FileContentType::Binary,
        }
    }

    fn convert_diff_generated_status_to_api(
        diff_generated_status: diff::types::DiffGeneratedStatus,
    ) -> FileGeneratedStatus {
        match diff_generated_status {
            diff::types::DiffGeneratedStatus::NonGenerated => FileGeneratedStatus::NotGenerated,
            diff::types::DiffGeneratedStatus::Partially => FileGeneratedStatus::PartiallyGenerated,
            diff::types::DiffGeneratedStatus::Fully => FileGeneratedStatus::FullyGenerated,
        }
    }
}
