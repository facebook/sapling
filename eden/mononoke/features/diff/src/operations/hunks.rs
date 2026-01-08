/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use context::CoreContext;
use futures::try_join;

use crate::error::DiffError;
use crate::types::DiffSingleInput;
use crate::types::HunkData;
use crate::types::Repo;
use crate::utils::content::load_content;
use crate::utils::whitespace::strip_horizontal_whitespace;

pub async fn hunks(
    ctx: &CoreContext,
    repo: &impl Repo,
    base: Option<DiffSingleInput>,
    other: Option<DiffSingleInput>,
    ignore_whitespace: bool,
) -> Result<Vec<HunkData>, DiffError> {
    let (base_bytes, other_bytes) = try_join!(
        async {
            if let Some(base_input) = &base {
                load_content(ctx, repo, base_input).await
            } else {
                Ok(None)
            }
        },
        async {
            if let Some(other_input) = &other {
                load_content(ctx, repo, other_input).await
            } else {
                Ok(None)
            }
        }
    )?;

    let (mut base_content, mut other_content) = match (base_bytes, other_bytes) {
        (None, None) => return Err(DiffError::empty_inputs()),
        (Some(base), None) => (base, Bytes::new()),
        (None, Some(other)) => (Bytes::new(), other),
        (Some(base), Some(other)) => (base, other),
    };

    let is_binary = base_content.contains(&0) || other_content.contains(&0);

    // Only strip whitespace if NOT binary and ignore_whitespace is enabled
    if !is_binary && ignore_whitespace {
        base_content = strip_horizontal_whitespace(&base_content);
        other_content = strip_horizontal_whitespace(&other_content);
    }

    let hunks =
        tokio::task::spawn_blocking(move || xdiff::diff_hunks(&base_content, &other_content))
            .await
            .map_err(|e| DiffError::internal(anyhow::anyhow!("spawn_blocking failed: {}", e)))?
            .into_iter()
            .map(HunkData::from)
            .collect();

    Ok(hunks)
}

#[cfg(test)]
fn create_non_root_path(path: &str) -> Result<mononoke_types::NonRootMPath, DiffError> {
    let mpath = mononoke_types::MPath::new(path)?;
    let non_root_mpath = mononoke_types::NonRootMPath::try_from(mpath)?;
    Ok(non_root_mpath)
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

    #[mononoke::fbinit_test]
    async fn test_hunks_basic(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create test commits with different content
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "line1\nline2\nline3\nline4\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("file.txt", "line1\nmodified line2\nline3\nline5\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

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

        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Should have 2 hunks: one for line2 modification and one for line4->line5 change
        assert_eq!(result.len(), 2);

        // First hunk: line2 modification (line index 1)
        let first_hunk = &result[0];
        assert_eq!(first_hunk.add_range.start, 1);
        assert_eq!(first_hunk.add_range.end, 2);
        assert_eq!(first_hunk.delete_range.start, 1);
        assert_eq!(first_hunk.delete_range.end, 2);

        // Second hunk: line4->line5 change (line index 3)
        let second_hunk = &result[1];
        assert_eq!(second_hunk.add_range.start, 3);
        assert_eq!(second_hunk.add_range.end, 4);
        assert_eq!(second_hunk.delete_range.start, 3);
        assert_eq!(second_hunk.delete_range.end, 4);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_binary_files(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create test commits with binary content (contains null bytes)
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("binary.bin", b"binary\x00content\x01here".as_slice())
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("binary.bin", b"different\x00binary\x02data".as_slice())
            .commit()
            .await
            .map_err(DiffError::internal)?;

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

        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Binary files should produce hunks (xdiff operates on byte level)
        assert!(!result.is_empty());
        assert!(!result.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_file_creation(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create commit with empty base and file in other
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("new_file.txt", "line1\nline2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

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

        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Should have exactly one hunk representing the entire file addition
        assert_eq!(result.len(), 1);

        let hunk = &result[0];
        // Addition ranges from line 0 to 3 (3 lines)
        assert_eq!(hunk.add_range.start, 0);
        assert_eq!(hunk.add_range.end, 3);
        // Deletion range is empty (no base content)
        assert_eq!(hunk.delete_range.start, 0);
        assert_eq!(hunk.delete_range.end, 0);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_file_deletion(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create a test commit with content
        let cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("deleted_file.txt", "line1\nline2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: cs,
            path: create_non_root_path("deleted_file.txt")?,
            replacement_path: None,
        });

        // Test Some vs None - should show file deletion
        let result = hunks(&ctx, &repo, Some(input), None, false).await?;

        // Should have exactly one hunk representing the entire file deletion
        assert_eq!(result.len(), 1);

        let hunk = &result[0];
        // Addition range is empty (no new content)
        assert_eq!(hunk.add_range.start, 0);
        assert_eq!(hunk.add_range.end, 0);
        // Deletion ranges from line 0 to 3 (3 lines deleted)
        assert_eq!(hunk.delete_range.start, 0);
        assert_eq!(hunk.delete_range.end, 3);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_identical_files(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create test commits with identical content
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

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

        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Identical files should produce no hunks
        assert_eq!(result.len(), 0);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_with_none_inputs(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create a test commit with content
        let cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "some content\nline2\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: cs,
            path: create_non_root_path("file.txt")?,
            replacement_path: None,
        });

        // Test None vs Some - Should show file addition
        let result = hunks(&ctx, &repo, None, Some(input.clone()), false).await?;
        assert_eq!(result.len(), 1);

        let hunk = &result[0];
        assert_eq!(hunk.add_range.start, 0);
        assert_eq!(hunk.add_range.end, 2); // Two lines added
        assert_eq!(hunk.delete_range.start, 0);
        assert_eq!(hunk.delete_range.end, 0); // No lines deleted

        // Test Some vs None - Should show file deletion
        let result = hunks(&ctx, &repo, Some(input), None, false).await?;
        assert_eq!(result.len(), 1);

        let hunk = &result[0];
        assert_eq!(hunk.add_range.start, 0);
        assert_eq!(hunk.add_range.end, 0); // No lines added
        assert_eq!(hunk.delete_range.start, 0);
        assert_eq!(hunk.delete_range.end, 2); // Two lines deleted

        // Test None vs None - should return an error
        let result = hunks(&ctx, &repo, None, None, false).await;
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert_eq!(
            error_message,
            "All inputs to the headerless diff were empty"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_multi_line_changes(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create test commits with multi-line changes
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "line1\nline2\nline3\nline4\nline5\nline6\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file(
                "file.txt",
                "line1\nnew line2\nnew line3\nline4\nline5\nmodified line6\n",
            )
            .commit()
            .await
            .map_err(DiffError::internal)?;

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

        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Should have 2 hunks: one for lines 2-3 modification and one for line 6 modification
        assert_eq!(result.len(), 2);

        // First hunk: lines 2-3 modification
        let first_hunk = &result[0];
        assert_eq!(first_hunk.add_range.start, 1);
        assert_eq!(first_hunk.add_range.end, 3);
        assert_eq!(first_hunk.delete_range.start, 1);
        assert_eq!(first_hunk.delete_range.end, 3);

        // Second hunk: line 6 modification
        let second_hunk = &result[1];
        assert_eq!(second_hunk.add_range.start, 5);
        assert_eq!(second_hunk.add_range.end, 6);
        assert_eq!(second_hunk.delete_range.start, 5);
        assert_eq!(second_hunk.delete_range.end, 6);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_empty_files(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create test commits with empty files
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("empty.txt", "")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("empty.txt", "")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: create_non_root_path("empty.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: create_non_root_path("empty.txt")?,
            replacement_path: None,
        });

        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Empty files with no changes should produce no hunks
        assert_eq!(result.len(), 0);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_string_inputs(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with String inputs
        let base_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nline2\nline3\nline4\n".to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nmodified line2\nline3\nline5\n".to_string(),
        });

        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Should have 2 hunks: one for line2 modification and one for line4->line5 change
        assert_eq!(result.len(), 2);

        // First hunk: line2 modification (line index 1)
        let first_hunk = &result[0];
        assert_eq!(first_hunk.add_range.start, 1);
        assert_eq!(first_hunk.add_range.end, 2);
        assert_eq!(first_hunk.delete_range.start, 1);
        assert_eq!(first_hunk.delete_range.end, 2);

        // Second hunk: line4->line5 change (line index 3)
        let second_hunk = &result[1];
        assert_eq!(second_hunk.add_range.start, 3);
        assert_eq!(second_hunk.add_range.end, 4);
        assert_eq!(second_hunk.delete_range.start, 3);
        assert_eq!(second_hunk.delete_range.end, 4);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_string_vs_none(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        let string_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nline2\nline3\n".to_string(),
        });

        // Test None vs String - Should show file addition
        let result = hunks(&ctx, &repo, None, Some(string_input.clone()), false).await?;
        assert_eq!(result.len(), 1);

        let hunk = &result[0];
        assert_eq!(hunk.add_range.start, 0);
        assert_eq!(hunk.add_range.end, 3); // Three lines added
        assert_eq!(hunk.delete_range.start, 0);
        assert_eq!(hunk.delete_range.end, 0); // No lines deleted

        // Test String vs None - Should show file deletion
        let result = hunks(&ctx, &repo, Some(string_input), None, false).await?;
        assert_eq!(result.len(), 1);

        let hunk = &result[0];
        assert_eq!(hunk.add_range.start, 0);
        assert_eq!(hunk.add_range.end, 0); // No lines added
        assert_eq!(hunk.delete_range.start, 0);
        assert_eq!(hunk.delete_range.end, 3); // Three lines deleted

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_string_identical(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with identical String inputs
        let base_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nline2\nline3\n".to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nline2\nline3\n".to_string(),
        });

        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), false).await?;

        // Identical strings should produce no hunks
        assert_eq!(result.len(), 0);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_ignore_whitespace_only(fb: FacebookInit) -> Result<(), DiffError> {
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
        let result = hunks(
            &ctx,
            &repo,
            Some(base_input.clone()),
            Some(other_input.clone()),
            false,
        )
        .await?;
        assert!(
            !result.is_empty(),
            "Hunks should show whitespace differences when ignore_whitespace=false"
        );

        // With ignore_whitespace: true, should show no hunks
        // (After stripping whitespace, "helloworld\nfoobar\n" should match on both sides)
        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), true).await?;
        assert_eq!(
            result.len(),
            0,
            "Hunks should show no changes when ignore_whitespace=true for whitespace-only changes"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_ignore_whitespace_mixed(fb: FacebookInit) -> Result<(), DiffError> {
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
        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), true).await?;
        assert!(
            !result.is_empty(),
            "Hunks should show content changes even with ignore_whitespace=true"
        );

        // Should have exactly one hunk for the modified line
        assert_eq!(result.len(), 1);
        let hunk = &result[0];
        assert_eq!(hunk.add_range.start, 1);
        assert_eq!(hunk.add_range.end, 2);
        assert_eq!(hunk.delete_range.start, 1);
        assert_eq!(hunk.delete_range.end, 2);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_ignore_whitespace_binary(fb: FacebookInit) -> Result<(), DiffError> {
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

        // Even with ignore_whitespace: true, binary files should still show differences
        // because whitespace stripping is not applied to binary files
        let result = hunks(&ctx, &repo, Some(base_input), Some(other_input), true).await?;

        // Binary files should produce hunks
        assert!(
            !result.is_empty(),
            "Binary files should still produce hunks even with ignore_whitespace=true"
        );

        Ok(())
    }
}
