/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Range;

use anyhow::Context;
use anyhow::Error;
use anyhow::anyhow;
use bytes::Bytes;
use context::CoreContext;
use derivative::Derivative;
use futures::try_join;
use git_types::git_lfs::format_lfs_pointer;
use lazy_static::lazy_static;
use mononoke_types::ContentMetadataV2;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::hash::GitSha1;
use regex::Regex;
pub use xdiff::CopyInfo;

use crate::ChangesetContext;
use crate::FileContext;
use crate::MononokeRepo;
use crate::changeset_path::ChangesetPathContentContext;
use crate::errors::MononokeError;
use crate::file::FileType;

lazy_static! {
    static ref BEGIN_MANUAL_SECTION_REGEX: Regex =
        Regex::new(r"^(\s|[[:punct:]])*BEGIN MANUAL SECTION").unwrap();
    static ref END_MANUAL_SECTION_REGEX: Regex =
        Regex::new(r"^(\s|[[:punct:]])*END MANUAL SECTION").unwrap();
}

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

impl MetadataDiffFileInfo {
    fn new(file_type: Option<FileType>, parsed_file_content: Option<&ParsedFileContent>) -> Self {
        let file_generated_status = match parsed_file_content {
            Some(ParsedFileContent::Text(text_file)) => Some((&text_file.generated_span).into()),
            _ => None,
        };

        MetadataDiffFileInfo {
            file_type,
            file_content_type: parsed_file_content.map(FileContentType::from),
            file_generated_status,
        }
    }
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

impl MetadataDiffLinesCount {
    fn new(
        old_parsed_file_content: Option<&ParsedFileContent>,
        new_parsed_file_content: Option<&ParsedFileContent>,
    ) -> Option<Self> {
        match (old_parsed_file_content, new_parsed_file_content) {
            (
                Some(ParsedFileContent::Text(old_text_file)),
                Some(ParsedFileContent::Text(new_text_file)),
            ) => Some(Self::diff_files(old_text_file, new_text_file)),
            (Some(ParsedFileContent::Text(old_text_file)), _) => {
                Some(Self::file_deleted(old_text_file))
            }
            (_, Some(ParsedFileContent::Text(new_text_file))) => {
                Some(Self::file_created(new_text_file))
            }
            _ => None,
        }
    }

    fn diff_files(old_text_file: &TextFile, new_text_file: &TextFile) -> Self {
        xdiff::diff_hunks(
            old_text_file.file_content.clone(),
            new_text_file.file_content.clone(),
        )
        .into_iter()
        .fold(
            Default::default(),
            |mut acc: MetadataDiffLinesCount, hunk| {
                acc.add_to_added_lines_count(hunk.add.len());
                acc.add_to_deleted_lines_count(hunk.remove.len());
                acc.add_to_significant_added_lines_count(
                    new_text_file.significant_lines_count_in_a_range(&hunk.add),
                );
                acc.add_to_significant_deleted_lines_count(
                    old_text_file.significant_lines_count_in_a_range(&hunk.remove),
                );
                if !hunk.add.is_empty() {
                    acc.first_added_line_number
                        .get_or_insert(hunk.add.start.saturating_add(1)); // +1 because hunk boundaries are 0-based.
                }

                acc
            },
        )
    }

    fn file_created(new_text_file: &TextFile) -> Self {
        Self {
            added_lines_count: new_text_file.lines(),
            significant_added_lines_count: new_text_file.significant_lines_count(),
            first_added_line_number: if new_text_file.lines() > 0 {
                Some(1)
            } else {
                None
            },
            ..Default::default()
        }
    }

    fn file_deleted(old_text_file: &TextFile) -> Self {
        Self {
            deleted_lines_count: old_text_file.lines(),
            significant_deleted_lines_count: old_text_file.significant_lines_count(),
            ..Default::default()
        }
    }

    fn add_to_added_lines_count(&mut self, count: usize) {
        self.added_lines_count = self.added_lines_count.saturating_add(count);
    }

    fn add_to_deleted_lines_count(&mut self, count: usize) {
        self.deleted_lines_count = self.deleted_lines_count.saturating_add(count);
    }

    fn add_to_significant_added_lines_count(&mut self, count: usize) {
        self.significant_added_lines_count =
            self.significant_added_lines_count.saturating_add(count);
    }

    fn add_to_significant_deleted_lines_count(&mut self, count: usize) {
        self.significant_deleted_lines_count =
            self.significant_deleted_lines_count.saturating_add(count);
    }
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

enum ParsedFileContent {
    Text(TextFile),
    NonUtf8,
    Binary,
}

impl From<&ParsedFileContent> for FileContentType {
    fn from(parsed_file_content: &ParsedFileContent) -> Self {
        match parsed_file_content {
            ParsedFileContent::Text(_) => FileContentType::Text,
            ParsedFileContent::NonUtf8 => FileContentType::NonUtf8,
            ParsedFileContent::Binary => FileContentType::Binary,
        }
    }
}

impl From<&FileGeneratedSpan> for FileGeneratedStatus {
    fn from(file_generated_span: &FileGeneratedSpan) -> Self {
        match file_generated_span {
            FileGeneratedSpan::FullyGenerated => FileGeneratedStatus::FullyGenerated,
            FileGeneratedSpan::PartiallyGenerated(_) => FileGeneratedStatus::PartiallyGenerated,
            FileGeneratedSpan::NotGenerated => FileGeneratedStatus::NotGenerated,
        }
    }
}

impl ParsedFileContent {
    async fn new<R: MononokeRepo>(file: FileContext<R>) -> Result<Self, MononokeError> {
        let metadata = file.metadata().await?;
        let parsed_content = if metadata.is_binary {
            ParsedFileContent::Binary
        } else if metadata.is_utf8 {
            let file_content = file.content_concat().await?;
            ParsedFileContent::Text(TextFile::new(file_content, metadata)?)
        } else {
            ParsedFileContent::NonUtf8
        };
        Ok(parsed_content)
    }
}

struct TextFile {
    file_content: Bytes,
    metadata: ContentMetadataV2,
    generated_span: FileGeneratedSpan,
}

impl TextFile {
    fn new(file_content: Bytes, metadata: ContentMetadataV2) -> Result<Self, MononokeError> {
        Ok(TextFile {
            generated_span: FileGeneratedSpan::new(file_content.clone(), &metadata)?,
            file_content,
            metadata,
        })
    }

    fn lines(&self) -> usize {
        if self.metadata.ends_in_newline {
            // For files that end in newline, the number of lines is equal to the number of newlines.
            self.metadata.newline_count as usize
        } else if self.metadata.total_size == 0 {
            // For empty files, the number of lines is zero.
            0
        } else {
            // For non-empty files that don't end in a newline, the number of lines is equal to the number of newlines plus one for the last line.
            (self.metadata.newline_count + 1) as usize
        }
    }

    fn significant_lines_count(&self) -> usize {
        match &self.generated_span {
            FileGeneratedSpan::FullyGenerated => 0usize,
            FileGeneratedSpan::PartiallyGenerated(manual_sections) => manual_sections
                .iter()
                .fold(0usize, |acc, section| acc.saturating_add(section.len())),
            FileGeneratedSpan::NotGenerated => self.lines(),
        }
    }

    fn significant_lines_count_in_a_range(&self, range: &Range<usize>) -> usize {
        match &self.generated_span {
            FileGeneratedSpan::FullyGenerated => 0usize,
            FileGeneratedSpan::PartiallyGenerated(manual_sections) => {
                manual_sections.iter().fold(0usize, |acc, section| {
                    acc.saturating_add(
                        section
                            .end
                            .min(range.end)
                            .saturating_sub(section.start.max(range.start)),
                    )
                })
            }
            FileGeneratedSpan::NotGenerated => range.len(),
        }
    }
}

enum FileGeneratedSpan {
    FullyGenerated,
    PartiallyGenerated(Vec<Range<usize>>),
    NotGenerated,
}

impl FileGeneratedSpan {
    fn new(content: Bytes, metadata: &ContentMetadataV2) -> Result<Self, MononokeError> {
        if !metadata.is_generated && !metadata.is_partially_generated {
            return Ok(FileGeneratedSpan::NotGenerated);
        }
        let content = std::str::from_utf8(&content)
            .context("Failed to parse valid UTF8 bytes for determining generated status")?;
        let mut found_generated_annotation = false;
        let mut manual_sections_ranges = Vec::new();
        let mut manual_section_start = None;

        for (line_number, line) in content.lines().enumerate() {
            if line.contains(concat!("@", "generated"))
                || line.contains(concat!("@", "partially-generated"))
            // The redundant concat is used to avoid marking this file as generated.
            {
                found_generated_annotation = true;
            }

            if END_MANUAL_SECTION_REGEX.is_match(line) {
                if let Some(manual_section_start) = manual_section_start {
                    manual_sections_ranges.push(manual_section_start..line_number);
                }
                manual_section_start = None;
            }

            if BEGIN_MANUAL_SECTION_REGEX.is_match(line) {
                manual_section_start = Some(line_number + 1);
            }
        }

        Ok(
            match (
                found_generated_annotation,
                manual_sections_ranges.is_empty(),
            ) {
                (true, true) => FileGeneratedSpan::FullyGenerated,
                (true, false) => FileGeneratedSpan::PartiallyGenerated(manual_sections_ranges),
                (false, _) => FileGeneratedSpan::NotGenerated,
            },
        )
    }
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

    // Helper for getting file information.
    async fn get_file_data(
        _ctx: &CoreContext,
        path: Option<&ChangesetPathContentContext<R>>,
        mode: UnifiedDiffMode,
    ) -> Result<Option<xdiff::DiffFile<String, Bytes>>, MononokeError> {
        match path {
            Some(path) => {
                if let Some(file_type) = path.file_type().await? {
                    let file = path.file().await?.ok_or_else(|| {
                        MononokeError::from(Error::msg("assertion error: file should exist"))
                    })?;
                    let metadata = file.metadata().await?;
                    let git_lfs_pointer = if path
                        .repo_ctx()
                        .config()
                        .git_configs
                        .git_lfs_interpret_pointers
                    {
                        Self::get_git_lfs_pointer(path, &metadata).await?
                    } else {
                        None
                    };
                    let diff_mode =
                        if mode == UnifiedDiffMode::OmitContent || git_lfs_pointer.is_some() {
                            UnifiedDiffMode::OmitContent
                        } else {
                            mode
                        };

                    let file_type = match file_type {
                        FileType::Regular => xdiff::FileType::Regular,
                        FileType::Executable => xdiff::FileType::Executable,
                        FileType::Symlink => xdiff::FileType::Symlink,
                        FileType::GitSubmodule => xdiff::FileType::GitSubmodule,
                    };
                    let contents = match (file_type, diff_mode) {
                        (xdiff::FileType::GitSubmodule, _) => {
                            let commit_hash_bytes = file.content_concat().await?;
                            let commit_hash = GitSha1::from_bytes(commit_hash_bytes)
                                .with_context(|| {
                                    format!("Invalid commit hash for submodule at {}", path.path())
                                })?
                                .to_string();
                            xdiff::FileContent::Submodule { commit_hash }
                        }
                        (_, UnifiedDiffMode::Inline) => {
                            let contents = file.content_concat().await?;
                            xdiff::FileContent::Inline(contents)
                        }
                        (_, UnifiedDiffMode::OmitContent) => xdiff::FileContent::Omitted {
                            content_hash: format!("{}", metadata.content_id),
                            git_lfs_pointer,
                        },
                    };
                    Ok(Some(xdiff::DiffFile {
                        path: path.path().to_string(),
                        contents,
                        file_type,
                    }))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    async fn get_git_lfs_pointer(
        path: &ChangesetPathContentContext<R>,
        metadata: &ContentMetadataV2,
    ) -> Result<Option<String>, MononokeError> {
        let file_change = match path.file_change().await? {
            Some(file_change) => Some(file_change),
            None => {
                // If the file is not touched in the current changeset,
                // try checking the last changeset that touched the file
                let non_root_mpath = NonRootMPath::try_from(path.path().clone())?;
                let last_modified_cs = path
                    .changeset()
                    .path_with_history(non_root_mpath.clone())
                    .await?
                    .last_modified()
                    .await?;
                match last_modified_cs {
                    Some(last_modified_cs) => last_modified_cs
                        .file_changes()
                        .await?
                        .get(&non_root_mpath)
                        .cloned(),
                    None => None,
                }
            }
        };
        let git_lfs_pointer = file_change.and_then(|fc| {
            fc.git_lfs().and_then(|git_lfs| {
                if git_lfs.is_lfs_pointer() {
                    Some(format_lfs_pointer(
                        metadata.sha256,
                        fc.size().unwrap_or_default(),
                    ))
                } else {
                    None
                }
            })
        });

        Ok(git_lfs_pointer)
    }

    /// Renders the diff (in the git diff format).
    ///
    /// If `mode` is `Placeholder` then `unified_diff(...)` doesn't fetch content,
    /// but just generates a placeholder diff that says that the files differ.
    pub async fn unified_diff(
        &self,
        ctx: &CoreContext,
        context_lines: usize,
        mode: UnifiedDiffMode,
    ) -> Result<UnifiedDiff, MononokeError> {
        let (new_file_data, mut old_file_data) = try_join!(
            Self::get_file_data(ctx, self.get_new_content(), mode),
            Self::get_file_data(ctx, self.get_old_content(), mode)
        )?;
        if let (Some(replacement_path), Some(old_file_data)) =
            (&self.subtree_copy_dest_path, &mut old_file_data)
        {
            // Override the old path with the replacement path after the subtree copy.
            old_file_data.path = replacement_path.to_string();
        }
        let is_binary =
            xdiff::file_is_binary(&new_file_data) || xdiff::file_is_binary(&old_file_data);
        let copy_info = self.copy_info();
        let opts = xdiff::DiffOpts {
            context: context_lines,
            copy_info,
        };
        // Generate a unified diff from old to new.
        let raw_diff = tokio::task::spawn_blocking(move || {
            xdiff::diff_unified(old_file_data, new_file_data, opts)
        })
        .await?;
        Ok(UnifiedDiff {
            raw_diff,
            is_binary,
        })
    }

    pub async fn metadata_diff(&self, _ctx: &CoreContext) -> Result<MetadataDiff, MononokeError> {
        let (new_file_type, mut new_file) = match self.get_new_content() {
            Some(path) => try_join!(path.file_type(), path.file())?,
            None => (None, None),
        };
        let new_parsed_file_content = match new_file.take() {
            Some(file) => Some(ParsedFileContent::new(file).await?),
            _ => None,
        };

        let (old_file_type, mut old_file) = match self.get_old_content() {
            Some(path) => try_join!(path.file_type(), path.file())?,
            None => (None, None),
        };
        let old_parsed_file_content = match old_file.take() {
            Some(file) => Some(ParsedFileContent::new(file).await?),
            _ => None,
        };

        Ok(MetadataDiff {
            old_file_info: MetadataDiffFileInfo::new(
                old_file_type,
                old_parsed_file_content.as_ref(),
            ),
            new_file_info: MetadataDiffFileInfo::new(
                new_file_type,
                new_parsed_file_content.as_ref(),
            ),
            lines_count: MetadataDiffLinesCount::new(
                old_parsed_file_content.as_ref(),
                new_parsed_file_content.as_ref(),
            ),
        })
    }
}
