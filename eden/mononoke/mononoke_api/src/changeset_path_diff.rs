/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Range;

use anyhow::anyhow;
use anyhow::Error;
use bytes::Bytes;
use futures::try_join;
use lazy_static::lazy_static;
use regex::Regex;
pub use xdiff::CopyInfo;

use crate::changeset_path::ChangesetPathContentContext;
use crate::errors::MononokeError;
use crate::file::FileType;

lazy_static! {
    static ref BEGIN_MANUAL_SECTION_REGEX: Regex =
        Regex::new(r"^\s*[[:punct:]]*\s*BEGIN MANUAL SECTION").unwrap();
    static ref END_MANUAL_SECTION_REGEX: Regex =
        Regex::new(r"^\s*[[:punct:]]*\s*END MANUAL SECTION").unwrap();
}

/// A path difference between two commits.
///
/// A ChangesetPathDiffContext shows the difference between two corresponding
/// files in the commits.
///
/// The changed, copied and moved variants contain the items in the same
/// order as the commits that were compared, i.e. in `a.diff(b)`, they
/// will contain `(a, b)`.  This usually means the destination is first.
#[derive(Clone, Debug)]
pub enum ChangesetPathDiffContext {
    Added(ChangesetPathContentContext),
    Removed(ChangesetPathContentContext),
    Changed(ChangesetPathContentContext, ChangesetPathContentContext),
    Copied(ChangesetPathContentContext, ChangesetPathContentContext),
    Moved(ChangesetPathContentContext, ChangesetPathContentContext),
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
            old_text_file.content.as_bytes(),
            new_text_file.content.as_bytes(),
        )
        .into_iter()
        .fold(
            Default::default(),
            |mut acc: MetadataDiffLinesCount, hunk| {
                acc.add_to_added_lines_count(hunk.add.len());
                acc.add_to_deleted_lines_count(hunk.remove.len());
                acc.add_to_significant_added_lines_count(
                    new_text_file.significant_lines_count_in_a_range(hunk.add),
                );
                acc.add_to_significant_deleted_lines_count(
                    old_text_file.significant_lines_count_in_a_range(hunk.remove),
                );
                acc
            },
        )
    }

    fn file_created(new_text_file: &TextFile) -> Self {
        Self {
            added_lines_count: new_text_file.content.lines().count(),
            significant_added_lines_count: new_text_file.significant_lines_count(),
            ..Default::default()
        }
    }

    fn file_deleted(old_text_file: &TextFile) -> Self {
        Self {
            deleted_lines_count: old_text_file.content.lines().count(),
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

enum ParsedFileContent<'a> {
    Text(TextFile<'a>),
    NonUtf8,
    Binary,
}

impl From<&ParsedFileContent<'_>> for FileContentType {
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

impl<'a> ParsedFileContent<'a> {
    fn new(content: &'a Bytes) -> Self {
        if let Ok(parsed_content) = std::str::from_utf8(content) {
            ParsedFileContent::Text(TextFile::new(parsed_content))
        } else if content.contains(&0) {
            ParsedFileContent::Binary
        } else {
            ParsedFileContent::NonUtf8
        }
    }
}

struct TextFile<'a> {
    content: &'a str,
    generated_span: FileGeneratedSpan,
}

impl<'a> TextFile<'a> {
    fn new(content: &'a str) -> Self {
        TextFile {
            content,
            generated_span: FileGeneratedSpan::new(content),
        }
    }

    fn significant_lines_count(&self) -> usize {
        match &self.generated_span {
            FileGeneratedSpan::FullyGenerated => 0usize,
            FileGeneratedSpan::PartiallyGenerated(manual_sections) => manual_sections
                .iter()
                .fold(0usize, |acc, section| acc.saturating_add(section.len())),
            FileGeneratedSpan::NotGenerated => self.content.lines().count(),
        }
    }

    fn significant_lines_count_in_a_range(&self, range: Range<usize>) -> usize {
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
    fn new<'a>(content: &'a str) -> Self {
        let mut found_generated_annotation = false;
        let mut manual_sections_ranges = Vec::new();
        let mut manual_section_start = None;

        for (line_number, line) in content.lines().enumerate() {
            if line.contains(concat!("{}{}", "@", "generated"))
                || line.contains(concat!("{}{}", "@", "partially-generated"))
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

        match (
            found_generated_annotation,
            manual_sections_ranges.is_empty(),
        ) {
            (true, true) => FileGeneratedSpan::FullyGenerated,
            (true, false) => FileGeneratedSpan::PartiallyGenerated(manual_sections_ranges),
            (false, _) => FileGeneratedSpan::NotGenerated,
        }
    }
}

impl ChangesetPathDiffContext {
    /// Create a new path diff context that compares the contents of two
    /// changeset paths.
    ///
    /// Copy information must be provided if the file has been copied or
    /// moved.
    pub fn new(
        base: Option<ChangesetPathContentContext>,
        other: Option<ChangesetPathContentContext>,
        copy_info: CopyInfo,
    ) -> Result<Self, MononokeError> {
        match (base, other, copy_info) {
            (Some(base), None, CopyInfo::None) => Ok(Self::Added(base)),
            (None, Some(other), CopyInfo::None) => Ok(Self::Removed(other)),
            (Some(base), Some(other), CopyInfo::None) => Ok(Self::Changed(base, other)),
            (Some(base), Some(other), CopyInfo::Copy) => Ok(Self::Copied(base, other)),
            (Some(base), Some(other), CopyInfo::Move) => Ok(Self::Moved(base, other)),
            invalid_args => Err(anyhow!(
                "Invalid changeset path diff context parameters: {:?}",
                invalid_args
            )
            .into()),
        }
    }

    /// Return the base path that is being compared.  This is the
    /// contents after modification.
    pub fn base(&self) -> Option<&ChangesetPathContentContext> {
        match self {
            Self::Added(base)
            | Self::Changed(base, _)
            | Self::Copied(base, _)
            | Self::Moved(base, _) => Some(base),
            Self::Removed(_) => None,
        }
    }

    /// Return the other path that is being compared against.  This
    /// is the contents before modification.
    pub fn other(&self) -> Option<&ChangesetPathContentContext> {
        match self {
            Self::Removed(other)
            | Self::Changed(_, other)
            | Self::Copied(_, other)
            | Self::Moved(_, other) => Some(other),
            Self::Added(_) => None,
        }
    }

    /// Return the main path for this difference.  This is the added or
    /// removed path, or the base (destination) in the case of modifications,
    /// copies, or moves.
    pub fn path(&self) -> &ChangesetPathContentContext {
        match self {
            Self::Added(base)
            | Self::Changed(base, _)
            | Self::Copied(base, _)
            | Self::Moved(base, _) => base,
            Self::Removed(other) => other,
        }
    }

    /// Return the copy information for this difference.
    pub fn copy_info(&self) -> CopyInfo {
        match self {
            Self::Added(_) | Self::Removed(_) | Self::Changed(_, _) => CopyInfo::None,
            Self::Copied(_, _) => CopyInfo::Copy,
            Self::Moved(_, _) => CopyInfo::Move,
        }
    }

    // Helper for getting file information.
    async fn get_file_data(
        path: Option<&ChangesetPathContentContext>,
        mode: UnifiedDiffMode,
    ) -> Result<Option<xdiff::DiffFile<String, Bytes>>, MononokeError> {
        match path {
            Some(path) => {
                if let Some(file_type) = path.file_type().await? {
                    let file = path.file().await?.ok_or_else(|| {
                        MononokeError::from(Error::msg("assertion error: file should exist"))
                    })?;
                    let file_type = match file_type {
                        FileType::Regular => xdiff::FileType::Regular,
                        FileType::Executable => xdiff::FileType::Executable,
                        FileType::Symlink => xdiff::FileType::Symlink,
                    };
                    let contents = match mode {
                        UnifiedDiffMode::Inline => {
                            let contents = file.content_concat().await?;
                            xdiff::FileContent::Inline(contents)
                        }
                        UnifiedDiffMode::OmitContent => {
                            let content_id = file.metadata().await?.content_id;
                            xdiff::FileContent::Omitted {
                                content_hash: format!("{}", content_id),
                            }
                        }
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

    /// Renders the diff (in the git diff format).
    ///
    /// If `mode` is `Placeholder` then `unified_diff(...)` doesn't fetch content,
    /// but just generates a placeholder diff that says that the files differ.
    pub async fn unified_diff(
        &self,
        context_lines: usize,
        mode: UnifiedDiffMode,
    ) -> Result<UnifiedDiff, MononokeError> {
        let (base_file, other_file) = try_join!(
            Self::get_file_data(self.base(), mode),
            Self::get_file_data(self.other(), mode)
        )?;
        let is_binary = xdiff::file_is_binary(&base_file) || xdiff::file_is_binary(&other_file);
        let copy_info = self.copy_info();
        let opts = xdiff::DiffOpts {
            context: context_lines,
            copy_info,
        };
        // The base is the target, so we diff in the opposite direction.
        let raw_diff = xdiff::diff_unified(other_file, base_file, opts);
        Ok(UnifiedDiff {
            raw_diff,
            is_binary,
        })
    }

    pub async fn metadata_diff(&self) -> Result<MetadataDiff, MononokeError> {
        let (new_file_type, new_file_content) = match self.base() {
            Some(path) => try_join!(path.file_type(), path.file_content())?,
            None => (None, None),
        };
        let new_parsed_file_content = new_file_content.as_ref().map(ParsedFileContent::new);

        let (old_file_type, old_file_content) = match self.other() {
            Some(path) => try_join!(path.file_type(), path.file_content())?,
            None => (None, None),
        };
        let old_parsed_file_content = old_file_content.as_ref().map(ParsedFileContent::new);

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
