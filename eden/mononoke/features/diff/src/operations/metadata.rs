/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Range;

use anyhow::Context;
use anyhow::anyhow;
use bytes::Bytes;
use context::CoreContext;
use futures::try_join;
use lazy_static::lazy_static;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use regex::Regex;
#[cfg(test)]
use mononoke_types::MPath;
#[cfg(test)]
use mononoke_types::NonRootMPath;

use crate::error::DiffError;
use crate::types::DiffContentType;
use crate::types::DiffFileType;
use crate::types::DiffGeneratedStatus;
use crate::types::DiffSingleInput;
use crate::types::Repo;
use crate::types::MetadataDiff;
use crate::types::MetadataFileInfo;
use crate::types::MetadataLinesCount;
use crate::utils::content::get_file_info_from_changeset_path;
use crate::utils::content::load_content;
use crate::utils::whitespace::strip_horizontal_whitespace;

// This logic comes from `mononoke_api/src/changeset_path_diff.rs`

lazy_static! {
    static ref BEGIN_MANUAL_SECTION_REGEX: Regex =
        Regex::new(r"^(\s|[[:punct:]])*BEGIN MANUAL SECTION").unwrap();
    static ref END_MANUAL_SECTION_REGEX: Regex =
        Regex::new(r"^(\s|[[:punct:]])*END MANUAL SECTION").unwrap();
}

#[derive(Clone, Debug)]
enum FileGeneratedSpan {
    FullyGenerated,
    PartiallyGenerated(Vec<Range<usize>>),
    NotGenerated,
}

#[derive(Clone, Debug)]
struct TextFile {
    file_content: Bytes,
    metadata: ContentMetadataV2,
    generated_span: FileGeneratedSpan,
}

#[derive(Clone, Debug)]
enum ParsedFileContent {
    Text(TextFile),
    NonUtf8,
    Binary,
}

impl FileGeneratedSpan {
    fn new(content: Bytes, metadata: &ContentMetadataV2) -> Result<Self, DiffError> {
        if !metadata.is_generated && !metadata.is_partially_generated {
            return Ok(FileGeneratedSpan::NotGenerated);
        }

        let content = std::str::from_utf8(&content)
            .context("Failed to parse valid UTF8 bytes for determining generated status")
            .map_err(DiffError::internal)?;

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

        Ok(match (
            found_generated_annotation,
            manual_sections_ranges.is_empty(),
        ) {
            (true, true) => FileGeneratedSpan::FullyGenerated,
            (true, false) => FileGeneratedSpan::PartiallyGenerated(manual_sections_ranges),
            (false, _) => FileGeneratedSpan::NotGenerated,
        })
    }
}

impl TextFile {
    fn new(file_content: Bytes, metadata: ContentMetadataV2) -> Result<Self, DiffError> {
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

impl ParsedFileContent {
    async fn new(
        ctx: &CoreContext,
        repo: &impl Repo,
        content_id: ContentId,
    ) -> Result<Self, DiffError> {
        // Load content metadata from blobstore
        let metadata = crate::utils::content::get_content_metadata(ctx, repo, &content_id)
            .await?;

        let parsed_content = if metadata.is_binary {
            ParsedFileContent::Binary
        } else if metadata.is_utf8 {
            // Load the actual content
            let file_content = {
                let blobstore = repo.repo_blobstore();
                let fetch_key = filestore::FetchKey::Canonical(content_id);
                filestore::fetch_concat_opt(blobstore, ctx, &fetch_key)
                    .await
                    .map_err(DiffError::internal)?
                    .ok_or_else(|| DiffError::content_not_found(content_id))?
            };
            ParsedFileContent::Text(TextFile::new(file_content, metadata)?)
        } else {
            ParsedFileContent::NonUtf8
        };
        Ok(parsed_content)
    }
}

async fn calculate_lines_count(
    old_parsed_file_content: Option<&ParsedFileContent>,
    new_parsed_file_content: Option<&ParsedFileContent>,
    old_file_type: Option<&DiffFileType>,
    new_file_type: Option<&DiffFileType>,
    ignore_whitespace: bool,
) -> Option<MetadataLinesCount> {
    if matches!(old_file_type, Some(&DiffFileType::GitSubmodule))
        || matches!(new_file_type, Some(&DiffFileType::GitSubmodule))
    {
        return None;
    }

    match (old_parsed_file_content, new_parsed_file_content) {
        (
            Some(ParsedFileContent::Text(old_text_file)),
            Some(ParsedFileContent::Text(new_text_file)),
        ) => Some(diff_files(old_text_file, new_text_file, ignore_whitespace)),
        (Some(ParsedFileContent::Text(old_text_file)), _) => Some(file_deleted(old_text_file)),
        (_, Some(ParsedFileContent::Text(new_text_file))) => Some(file_created(new_text_file)),
        _ => None,
    }
}

fn diff_files(old_text_file: &TextFile, new_text_file: &TextFile, ignore_whitespace: bool) -> MetadataLinesCount {
    let old_content = if ignore_whitespace {
        strip_horizontal_whitespace(&old_text_file.file_content)
    } else {
        old_text_file.file_content.clone()
    };

    let new_content = if ignore_whitespace {
        strip_horizontal_whitespace(&new_text_file.file_content)
    } else {
        new_text_file.file_content.clone()
    };

    let hunks = xdiff::diff_hunks(old_content, new_content);

    let mut added_lines = 0i64;
    let mut deleted_lines = 0i64;
    let mut significant_added_lines = 0i64;
    let mut significant_deleted_lines = 0i64;
    let mut first_added_line_number = None;

    for hunk in hunks {
        added_lines = added_lines.saturating_add(hunk.add.len() as i64);
        deleted_lines = deleted_lines.saturating_add(hunk.remove.len() as i64);
        significant_added_lines = significant_added_lines
            .saturating_add(new_text_file.significant_lines_count_in_a_range(&hunk.add) as i64);
        significant_deleted_lines = significant_deleted_lines
            .saturating_add(old_text_file.significant_lines_count_in_a_range(&hunk.remove) as i64);
        if !hunk.add.is_empty() && first_added_line_number.is_none() {
            first_added_line_number = Some(hunk.add.start.saturating_add(1) as i64); // +1 because hunk boundaries are 0-based.
        }
    }

    MetadataLinesCount {
        added_lines,
        deleted_lines,
        significant_added_lines,
        significant_deleted_lines,
        first_added_line_number,
    }
}

fn file_created(new_text_file: &TextFile) -> MetadataLinesCount {
    MetadataLinesCount {
        added_lines: new_text_file.lines() as i64,
        deleted_lines: 0,
        significant_added_lines: new_text_file.significant_lines_count() as i64,
        significant_deleted_lines: 0,
        first_added_line_number: if new_text_file.lines() > 0 {
            Some(1)
        } else {
            None
        },
    }
}

fn file_deleted(old_text_file: &TextFile) -> MetadataLinesCount {
    MetadataLinesCount {
        added_lines: 0,
        deleted_lines: old_text_file.lines() as i64,
        significant_added_lines: 0,
        significant_deleted_lines: old_text_file.significant_lines_count() as i64,
        first_added_line_number: None,
    }
}

fn convert_file_type_to_diff(file_type: mononoke_types::FileType) -> DiffFileType {
    match file_type {
        mononoke_types::FileType::Regular => DiffFileType::Regular,
        mononoke_types::FileType::Executable => DiffFileType::Executable,
        mononoke_types::FileType::Symlink => DiffFileType::Symlink,
        mononoke_types::FileType::GitSubmodule => DiffFileType::GitSubmodule,
    }
}

fn convert_content_type_to_diff(parsed_content: &ParsedFileContent) -> DiffContentType {
    match parsed_content {
        ParsedFileContent::Text(_) => DiffContentType::Text,
        ParsedFileContent::NonUtf8 => DiffContentType::NonUtf8,
        ParsedFileContent::Binary => DiffContentType::Binary,
    }
}

fn convert_generated_status_to_diff(generated_span: &FileGeneratedSpan) -> DiffGeneratedStatus {
    match generated_span {
        FileGeneratedSpan::FullyGenerated => DiffGeneratedStatus::Fully,
        FileGeneratedSpan::PartiallyGenerated(_) => DiffGeneratedStatus::Partially,
        FileGeneratedSpan::NotGenerated => DiffGeneratedStatus::NonGenerated,
    }
}

fn create_file_info(
    file_type: Option<DiffFileType>,
    parsed_content: Option<&ParsedFileContent>,
) -> MetadataFileInfo {
    let content_type = parsed_content.map(convert_content_type_to_diff);
    let generated_status = match parsed_content {
        Some(ParsedFileContent::Text(text_file)) => {
            Some(convert_generated_status_to_diff(&text_file.generated_span))
        }
        _ => None,
    };

    MetadataFileInfo {
        file_type,
        content_type,
        generated_status,
    }
}

async fn get_file_details_from_input(
    ctx: &CoreContext,
    repo: &impl Repo,
    input: &DiffSingleInput,
) -> Result<(Option<DiffFileType>, Option<ParsedFileContent>), DiffError> {
    match input {
        DiffSingleInput::ChangesetPath(changeset_input) => {
            let non_root_mpath = changeset_input.path.clone();

            let file_info = get_file_info_from_changeset_path(
                ctx,
                repo,
                changeset_input.changeset_id,
                non_root_mpath,
            ).await?;

            let (content_id, file_type) = match file_info {
                Some((cid, ft)) => (Some(cid), Some(convert_file_type_to_diff(ft))),
                None => (None, None),
            };

            // For Git submodules, we don't parse file content for metadata purposes
            let parsed_file_content = match (&content_id, &file_type) {
                (_, Some(DiffFileType::GitSubmodule)) => None,
                (Some(content_id), _) => {
                    Some(ParsedFileContent::new(ctx, repo, *content_id).await?)
                },
                (None, _) => None,
            };

            Ok((file_type, parsed_file_content))
        }
        DiffSingleInput::Content(content_input) => {
            let parsed_file_content = Some(ParsedFileContent::new(ctx, repo, content_input.content_id).await?);

            // For content-only inputs, we don't have file type information
            Ok((None, parsed_file_content))
        }
        DiffSingleInput::String(_string_input) => {
            let file_content = load_content(ctx, repo, input)
                .await?
                // For string inputs we will never get None here
                .ok_or_else(|| DiffError::internal(anyhow!("Failed to load content from String input")))?;

            let is_binary = file_content.contains(&0u8);
            let is_utf8 = std::str::from_utf8(&file_content).is_ok();

            // Create a lightweight metadata struct for string content analysis
            // We can't create a full ContentMetadataV2 without a content_id
            let newline_count = file_content.iter().filter(|&&b| b == b'\n').count() as u64;
            let ends_in_newline = file_content.last() == Some(&b'\n');

            // Build a minimal metadata for TextFile creation
            let metadata = ContentMetadataV2 {
                content_id: ContentId::from_bytes([0; 32]).unwrap(), // Placeholder - not used for string inputs
                total_size: file_content.len() as u64,
                sha1: mononoke_types::hash::Sha1::from_byte_array([0; 20]),
                sha256: mononoke_types::hash::Sha256::from_byte_array([0; 32]),
                git_sha1: mononoke_types::hash::RichGitSha1::from_sha1(
                    mononoke_types::hash::GitSha1::from_byte_array([0; 20]),
                    "blob",
                    file_content.len() as u64,
                ),
                seeded_blake3: mononoke_types::hash::Blake3::from_byte_array([0; 32]),
                is_binary,
                is_utf8,
                is_ascii: file_content.is_ascii(),
                ends_in_newline,
                newline_count,
                first_line: None,
                is_generated: false,
                is_partially_generated: false,
            };

            let parsed_file_content = if is_binary {
                Some(ParsedFileContent::Binary)
            } else if is_utf8 {
                Some(ParsedFileContent::Text(TextFile::new(file_content, metadata)?))
            } else {
                Some(ParsedFileContent::NonUtf8)
            };

            // String inputs would be a regular file by definition
            Ok((Some(DiffFileType::Regular), parsed_file_content))
        }
    }
}

pub async fn metadata(
    ctx: &CoreContext,
    repo: &impl Repo,
    base: Option<DiffSingleInput>,
    other: Option<DiffSingleInput>,
    ignore_whitespace: bool,
) -> Result<MetadataDiff, DiffError> {

    // Get file information directly from inputs
    let (base_file_details, other_file_details) = try_join!(
        async {
            if let Some(base_input) = &base {
                get_file_details_from_input(ctx, repo, base_input).await.map(Some)
            } else {
                Ok(Some((None, None)))
            }
        },
        async {
            if let Some(other_input) = &other {
                get_file_details_from_input(ctx, repo, other_input).await.map(Some)
            } else {
                Ok(Some((None, None)))
            }
        }
    )?;

    let (base_file_type, base_parsed_content) = base_file_details.unwrap_or((None, None));
    let (other_file_type, other_parsed_content) = other_file_details.unwrap_or((None, None));

    let lines_count = calculate_lines_count(
        base_parsed_content.as_ref(),
        other_parsed_content.as_ref(),
        base_file_type.as_ref(),
        other_file_type.as_ref(),
        ignore_whitespace,
    )
    .await;

    let base_file_info = create_file_info(base_file_type, base_parsed_content.as_ref());
    let other_file_info = create_file_info(other_file_type, other_parsed_content.as_ref());

    Ok(MetadataDiff {
        base_file_info,
        other_file_info,
        lines_count,
    })
}

#[cfg(test)]
mod tests {

    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use test_repo_factory;
    use tests_utils::BasicTestRepo;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::types::DiffInputChangesetPath;
    use crate::types::DiffSingleInput;

    async fn init_test_repo(ctx: &CoreContext) -> Result<BasicTestRepo, DiffError> {
        let repo = test_repo_factory::build_empty(ctx.fb)
            .await
            .map_err(DiffError::internal)?;
        Ok(repo)
    }

    fn create_non_root_path(path: &str) -> Result<NonRootMPath, DiffError> {
        let mpath = MPath::new(path)?;
        let non_root_mpath = NonRootMPath::try_from(mpath)?;
        Ok(non_root_mpath)
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_basic(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create test commits with different content
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("file.txt", "line1\nmodified line2\nline3\n")
            .commit()
            .await?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: create_non_root_path("file.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: create_non_root_path("file.txt")?,
            replacement_path: None,
        });

        let metadata_diff = metadata(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Check file info
        assert_eq!(
            metadata_diff.base_file_info.file_type,
            Some(DiffFileType::Regular)
        );
        assert_eq!(
            metadata_diff.base_file_info.content_type,
            Some(DiffContentType::Text)
        );
        assert_eq!(
            metadata_diff.base_file_info.generated_status,
            Some(DiffGeneratedStatus::NonGenerated)
        );

        assert_eq!(
            metadata_diff.other_file_info.file_type,
            Some(DiffFileType::Regular)
        );
        assert_eq!(
            metadata_diff.other_file_info.content_type,
            Some(DiffContentType::Text)
        );
        assert_eq!(
            metadata_diff.other_file_info.generated_status,
            Some(DiffGeneratedStatus::NonGenerated)
        );

        // Check lines count
        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 1);
        assert_eq!(lines_count.deleted_lines, 1);
        assert_eq!(lines_count.significant_added_lines, 1);
        assert_eq!(lines_count.significant_deleted_lines, 1);
        assert_eq!(lines_count.first_added_line_number, Some(2));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_binary_files(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create test commits with binary content (contains null bytes)
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("binary.bin", b"binary\x00content\x01here".as_slice())
            .commit()
            .await?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("binary.bin", b"different\x00binary\x02data".as_slice())
            .commit()
            .await?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: create_non_root_path("binary.bin")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: create_non_root_path("binary.bin")?,
            replacement_path: None,
        });

        let metadata_diff = metadata(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Check that content type is binary
        assert_eq!(
            metadata_diff.base_file_info.content_type,
            Some(DiffContentType::Binary)
        );
        assert_eq!(
            metadata_diff.other_file_info.content_type,
            Some(DiffContentType::Binary)
        );

        // Lines count should be None for binary files
        assert!(metadata_diff.lines_count.is_none());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_file_creation(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Test with one empty file and one with content
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .commit()
            .await?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("new_file.txt", "new content\nline2\n")
            .commit()
            .await?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: create_non_root_path("new_file.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: create_non_root_path("new_file.txt")?,
            replacement_path: None,
        });

        let metadata_diff = metadata(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Base file doesn't exist
        assert_eq!(metadata_diff.base_file_info.file_type, None);
        assert_eq!(metadata_diff.base_file_info.content_type, None);

        // Other file exists
        assert_eq!(
            metadata_diff.other_file_info.file_type,
            Some(DiffFileType::Regular)
        );
        assert_eq!(
            metadata_diff.other_file_info.content_type,
            Some(DiffContentType::Text)
        );

        // Check lines count for file creation
        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 2);
        assert_eq!(lines_count.deleted_lines, 0);
        assert_eq!(lines_count.first_added_line_number, Some(1));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_with_none_inputs(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create a test commit with content
        let cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "some content\nline2\n")
            .commit()
            .await?;

        let input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: cs,
            path: create_non_root_path("file.txt")?,
            replacement_path: None,
        });

        // Test None vs Some - should show addition
        let metadata_diff = metadata(
            &ctx,
            &repo,
            None,
            Some(input.clone()),
            false,
        )
        .await?;

        // Base file doesn't exist
        assert_eq!(metadata_diff.base_file_info.file_type, None);
        assert_eq!(metadata_diff.base_file_info.content_type, None);

        // Other file exists
        assert_eq!(
            metadata_diff.other_file_info.file_type,
            Some(DiffFileType::Regular)
        );
        assert_eq!(
            metadata_diff.other_file_info.content_type,
            Some(DiffContentType::Text)
        );

        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 2);
        assert_eq!(lines_count.deleted_lines, 0);

        // Test Some vs None - should show deletion
        let metadata_diff = metadata(
            &ctx,
            &repo,
            Some(input),
            None,
            false,
        )
        .await?;

        // Base file exists
        assert_eq!(
            metadata_diff.base_file_info.file_type,
            Some(DiffFileType::Regular)
        );
        assert_eq!(
            metadata_diff.base_file_info.content_type,
            Some(DiffContentType::Text)
        );

        // Other file doesn't exist
        assert_eq!(metadata_diff.other_file_info.file_type, None);
        assert_eq!(metadata_diff.other_file_info.content_type, None);

        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 0);
        assert_eq!(lines_count.deleted_lines, 2);

        // Test None vs None - should show no difference
        let metadata_diff = metadata(&ctx, &repo, None, None, false).await?;

        // Both files don't exist
        assert_eq!(metadata_diff.base_file_info.file_type, None);
        assert_eq!(metadata_diff.base_file_info.content_type, None);
        assert_eq!(metadata_diff.other_file_info.file_type, None);
        assert_eq!(metadata_diff.other_file_info.content_type, None);

        // No lines count for non-existent files
        assert!(metadata_diff.lines_count.is_none());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_generated_file(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create a generated file
        let generated_content = "// @generated\nGenerated content\nMore generated content\n";
        let cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("generated.txt", generated_content)
            .commit()
            .await?;

        let input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: cs,
            path: create_non_root_path("generated.txt")?,
            replacement_path: None,
        });

        let metadata_diff = metadata(&ctx, &repo, None, Some(input), false).await?;

        // Check that generated status is detected
        assert_eq!(
            metadata_diff.other_file_info.generated_status,
            Some(DiffGeneratedStatus::Fully)
        );
        assert_eq!(
            metadata_diff.other_file_info.content_type,
            Some(DiffContentType::Text)
        );

        // Lines count should consider significant lines (0 for fully generated)
        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 3);
        assert_eq!(lines_count.significant_added_lines, 0);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_partially_generated_file(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create a partially generated file with manual sections
        let partially_generated_content = concat!(
            "// @partially-generated\n",
            "Generated section 1\n",
            "// BEGIN MANUAL SECTION\n",
            "Manual code here\n",
            "More manual code\n",
            "// END MANUAL SECTION\n",
            "Generated section 2\n"
        );
        let cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("partial.txt", partially_generated_content)
            .commit()
            .await?;

        let input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: cs,
            path: create_non_root_path("partial.txt")?,
            replacement_path: None,
        });

        let metadata_diff = metadata(&ctx, &repo, None, Some(input), false).await?;

        // Check that partially generated status is detected
        assert_eq!(
            metadata_diff.other_file_info.generated_status,
            Some(DiffGeneratedStatus::Partially)
        );

        // Lines count should only count manual sections as significant
        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 7); // Total lines
        assert_eq!(lines_count.significant_added_lines, 2); // Only manual lines (lines 3-4, 0-indexed)

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_string_inputs(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with String inputs
        let base_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nline2\nline3\n".to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nmodified line2\nline3\n".to_string(),
        });

        let metadata_diff = metadata(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        assert_eq!(metadata_diff.base_file_info.file_type, Some(DiffFileType::Regular));
        assert_eq!(
            metadata_diff.base_file_info.content_type,
            Some(DiffContentType::Text)
        );
        assert_eq!(
            metadata_diff.base_file_info.generated_status,
            Some(DiffGeneratedStatus::NonGenerated)
        );

        assert_eq!(metadata_diff.other_file_info.file_type, Some(DiffFileType::Regular));
        assert_eq!(
            metadata_diff.other_file_info.content_type,
            Some(DiffContentType::Text)
        );
        assert_eq!(
            metadata_diff.other_file_info.generated_status,
            Some(DiffGeneratedStatus::NonGenerated)
        );

        // Check lines count
        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 1);
        assert_eq!(lines_count.deleted_lines, 1);
        assert_eq!(lines_count.significant_added_lines, 1);
        assert_eq!(lines_count.significant_deleted_lines, 1);
        assert_eq!(lines_count.first_added_line_number, Some(2));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_string_vs_none(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        let string_input = DiffSingleInput::String(DiffInputString {
            content: "some content\nline2\n".to_string(),
        });

        // Test None vs String - should show addition
        let metadata_diff = metadata(&ctx, &repo, None, Some(string_input.clone()), false).await?;

        // Base file doesn't exist
        assert_eq!(metadata_diff.base_file_info.file_type, None);
        assert_eq!(metadata_diff.base_file_info.content_type, None);

        // Other file exists
        assert_eq!(metadata_diff.other_file_info.file_type, Some(DiffFileType::Regular));
        assert_eq!(
            metadata_diff.other_file_info.content_type,
            Some(DiffContentType::Text)
        );

        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 2);
        assert_eq!(lines_count.deleted_lines, 0);

        // Test String vs None - should show deletion
        let metadata_diff = metadata(&ctx, &repo, Some(string_input), None, false).await?;

        // Base file exists
        assert_eq!(metadata_diff.base_file_info.file_type, Some(DiffFileType::Regular));
        assert_eq!(
            metadata_diff.base_file_info.content_type,
            Some(DiffContentType::Text)
        );

        // Other file doesn't exist
        assert_eq!(metadata_diff.other_file_info.file_type, None);
        assert_eq!(metadata_diff.other_file_info.content_type, None);

        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 0);
        assert_eq!(lines_count.deleted_lines, 2);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_string_binary(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with binary String inputs (contains null bytes)
        let base_input = DiffSingleInput::String(DiffInputString {
            content: String::from_utf8_lossy(b"binary\x00content").to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: String::from_utf8_lossy(b"different\x00binary").to_string(),
        });

        let metadata_diff = metadata(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Check that content type is binary
        assert_eq!(
            metadata_diff.base_file_info.content_type,
            Some(DiffContentType::Binary)
        );
        assert_eq!(
            metadata_diff.other_file_info.content_type,
            Some(DiffContentType::Binary)
        );

        // Lines count should be None for binary files
        assert!(metadata_diff.lines_count.is_none());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_string_empty(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with empty string inputs
        let empty_input = DiffSingleInput::String(DiffInputString {
            content: String::new(),
        });
        let non_empty_input = DiffSingleInput::String(DiffInputString {
            content: "some content\n".to_string(),
        });

        let metadata_diff = metadata(&ctx, &repo, Some(empty_input.clone()), Some(non_empty_input), false).await?;

        // Both should be text files
        assert_eq!(
            metadata_diff.base_file_info.content_type,
            Some(DiffContentType::Text)
        );
        assert_eq!(
            metadata_diff.other_file_info.content_type,
            Some(DiffContentType::Text)
        );

        // Check lines count
        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 1);
        assert_eq!(lines_count.deleted_lines, 0);

        // Test two empty strings
        let metadata_diff = metadata(&ctx, &repo, Some(empty_input.clone()), Some(empty_input), false).await?;
        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(lines_count.added_lines, 0);
        assert_eq!(lines_count.deleted_lines, 0);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_string_special_chars(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with various special characters and Unicode
        let special_input = DiffSingleInput::String(DiffInputString {
            content: "Hello 世界\nTab:\t\nEmoji: 🚀\nQuotes: \"test\" 'test'\n".to_string(),
        });
        let plain_input = DiffSingleInput::String(DiffInputString {
            content: "Plain text\n".to_string(),
        });

        let metadata_diff = metadata(&ctx, &repo, Some(special_input), Some(plain_input), false).await?;

        // Should handle special characters as text
        assert_eq!(
            metadata_diff.base_file_info.content_type,
            Some(DiffContentType::Text)
        );
        assert_eq!(
            metadata_diff.other_file_info.content_type,
            Some(DiffContentType::Text)
        );

        // Lines count should work correctly
        let lines_count = metadata_diff.lines_count.unwrap();
        assert!(lines_count.added_lines > 0 || lines_count.deleted_lines > 0);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_ignore_whitespace_only(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with only whitespace differences
        let base_input = DiffSingleInput::String(DiffInputString {
            content: "hello world\nfoo bar\n".to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: "hello  world\nfoo\tbar\n".to_string(),
        });

        // With ignore_whitespace: false, should show differences
        let metadata_diff = metadata(
            &ctx,
            &repo,
            Some(base_input.clone()),
            Some(other_input.clone()),
            false,
        )
        .await?;
        let lines_count = metadata_diff.lines_count.unwrap();
        assert!(
            lines_count.added_lines > 0 || lines_count.deleted_lines > 0,
            "Should show whitespace differences when ignore_whitespace=false"
        );

        // With ignore_whitespace: true, should show no changes in line counts
        // (After stripping whitespace, "helloworld\nfoobar\n" should match on both sides)
        let metadata_diff = metadata(
            &ctx,
            &repo,
            Some(base_input),
            Some(other_input),
            true,
        )
        .await?;
        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(
            lines_count.added_lines, 0,
            "Should show no added lines when ignore_whitespace=true for whitespace-only changes"
        );
        assert_eq!(
            lines_count.deleted_lines, 0,
            "Should show no deleted lines when ignore_whitespace=true for whitespace-only changes"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_ignore_whitespace_mixed(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with both whitespace and content differences
        let base_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nline2\nline3\n".to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nmodified  line2\nline3\n".to_string(),
        });

        // With ignore_whitespace: true, should still show content change
        // After stripping whitespace: "line2" vs "modifiedline2" (real difference!)
        let metadata_diff = metadata(
            &ctx,
            &repo,
            Some(base_input),
            Some(other_input),
            true,
        )
        .await?;
        let lines_count = metadata_diff.lines_count.unwrap();
        assert_eq!(
            lines_count.added_lines, 1,
            "Should show content changes even with ignore_whitespace=true"
        );
        assert_eq!(
            lines_count.deleted_lines, 1,
            "Should show content changes even with ignore_whitespace=true"
        );
        assert_eq!(lines_count.first_added_line_number, Some(2));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_metadata_ignore_whitespace_binary(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test that binary files are not affected by whitespace stripping
        let base_input = DiffSingleInput::String(DiffInputString {
            content: String::from_utf8_lossy(b"binary\x00 content").to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: String::from_utf8_lossy(b"binary\x00  content").to_string(),
        });

        // Even with ignore_whitespace: true, binary files should not use line counting
        let metadata_diff = metadata(
            &ctx,
            &repo,
            Some(base_input),
            Some(other_input),
            true,
        )
        .await?;

        // Binary files should not have line counts
        assert!(metadata_diff.lines_count.is_none(), "Binary files should not have line counts");
        assert_eq!(metadata_diff.base_file_info.content_type, Some(DiffContentType::Binary));
        assert_eq!(metadata_diff.other_file_info.content_type, Some(DiffContentType::Binary));

        Ok(())
    }
}
