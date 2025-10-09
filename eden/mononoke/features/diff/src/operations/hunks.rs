/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use context::CoreContext;
use futures::try_join;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;

use crate::error::DiffError;
use crate::types::DiffSingleInput;
use crate::types::HunkData;
use crate::utils::content::load_content;

pub async fn hunks<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    base: Option<DiffSingleInput>,
    other: Option<DiffSingleInput>,
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

    let (base_content, other_content) = match (base_bytes, other_bytes) {
        (None, None) => return Err(DiffError::empty_inputs()),
        (Some(base), None) => (base, Bytes::new()),
        (None, Some(other)) => (Bytes::new(), other),
        (Some(base), Some(other)) => (base, other),
    };

    let hunks = xdiff::diff_hunks(&base_content, &other_content)
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
    use std::sync::Arc;

    use fbinit::FacebookInit;
    use mononoke_api::Repo;
    use mononoke_api::RepoContext;
    use mononoke_macros::mononoke;
    use test_repo_factory;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::types::DiffInputChangesetPath;
    use crate::types::DiffSingleInput;

    async fn init_test_repo(ctx: &CoreContext) -> Result<RepoContext<Repo>, DiffError> {
        let repo: Repo = test_repo_factory::build_empty(ctx.fb)
            .await
            .map_err(DiffError::internal)?;
        let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo))
            .await
            .map_err(DiffError::internal)?;
        Ok(repo_ctx)
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_basic(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create test commits with different content
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("file.txt", "line1\nline2\nline3\nline4\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
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

        let result = hunks(&ctx, &repo_ctx, Some(base_input), Some(other_input)).await?;

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
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create test commits with binary content (contains null bytes)
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("binary.bin", b"binary\x00content\x01here".as_slice())
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
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

        let result = hunks(&ctx, &repo_ctx, Some(base_input), Some(other_input)).await?;

        // Binary files should produce hunks (xdiff operates on byte level)
        assert!(!result.is_empty());
        assert!(!result.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_file_creation(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create commit with empty base and file in other
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
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

        let result = hunks(&ctx, &repo_ctx, Some(base_input), Some(other_input)).await?;

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
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create a test commit with content
        let cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
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
        let result = hunks(&ctx, &repo_ctx, Some(input), None).await?;

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
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create test commits with identical content
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
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

        let result = hunks(&ctx, &repo_ctx, Some(base_input), Some(other_input)).await?;

        // Identical files should produce no hunks
        assert_eq!(result.len(), 0);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_hunks_with_none_inputs(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create a test commit with content
        let cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
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
        let result = hunks(&ctx, &repo_ctx, None, Some(input.clone())).await?;
        assert_eq!(result.len(), 1);

        let hunk = &result[0];
        assert_eq!(hunk.add_range.start, 0);
        assert_eq!(hunk.add_range.end, 2); // Two lines added
        assert_eq!(hunk.delete_range.start, 0);
        assert_eq!(hunk.delete_range.end, 0); // No lines deleted

        // Test Some vs None - Should show file deletion
        let result = hunks(&ctx, &repo_ctx, Some(input), None).await?;
        assert_eq!(result.len(), 1);

        let hunk = &result[0];
        assert_eq!(hunk.add_range.start, 0);
        assert_eq!(hunk.add_range.end, 0); // No lines added
        assert_eq!(hunk.delete_range.start, 0);
        assert_eq!(hunk.delete_range.end, 2); // Two lines deleted

        // Test None vs None - should return an error
        let result = hunks(&ctx, &repo_ctx, None, None).await;
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
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create test commits with multi-line changes
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("file.txt", "line1\nline2\nline3\nline4\nline5\nline6\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
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

        let result = hunks(&ctx, &repo_ctx, Some(base_input), Some(other_input)).await?;

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
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create test commits with empty files
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("empty.txt", "")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
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

        let result = hunks(&ctx, &repo_ctx, Some(base_input), Some(other_input)).await?;

        // Empty files with no changes should produce no hunks
        assert_eq!(result.len(), 0);

        Ok(())
    }
}
