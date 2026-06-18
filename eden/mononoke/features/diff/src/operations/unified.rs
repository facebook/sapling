/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use bytes::Bytes;
use context::CoreContext;
use futures::try_join;
use mononoke_macros::mononoke;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;

use crate::error::DiffError;
use crate::types::DiffSingleInput;
use crate::types::Repo;
use crate::types::UnifiedDiff;
use crate::types::UnifiedDiffOpts;
use crate::utils::content::DiffFileOpts;
use crate::utils::content::LoadDiffFileResult;
use crate::utils::content::LoadedDiffFileKind;
use crate::utils::content::load_diff_file;
use crate::utils::whitespace::strip_horizontal_whitespace;

const LFS_NON_TEXT_SENTINEL: &str = "Git LFS: none (non-text content omitted)\n";

/// Compute unified diff between two inputs.
///
/// Accepts separate repos for base and other inputs to support cross-bubble diffs.
/// Each repo should be bound to the bubble that contains its changeset (if any).
pub async fn unified(
    ctx: &CoreContext,
    base_pair: Option<(DiffSingleInput, &impl Repo)>,
    other_pair: Option<(DiffSingleInput, &impl Repo)>,
    options: UnifiedDiffOpts,
) -> Result<UnifiedDiff, DiffError> {
    let diff_file_opts = DiffFileOpts {
        file_type: options.file_type,
        inspect_lfs_pointers: options.inspect_lfs_pointers,
        omit_content: options.omit_content,
    };

    let (mut base_result, mut other_result) = try_join!(
        async {
            if let Some((base_input, base_repo)) = base_pair {
                let default_path =
                    to_non_root_path("base_path").context("The hardcoded path was not valid")?;
                load_diff_file(ctx, base_repo, base_input, default_path, &diff_file_opts).await
            } else {
                Ok(LoadDiffFileResult {
                    diff_file: None,
                    is_binary: false,
                    kind: LoadedDiffFileKind::Other,
                })
            }
        },
        async {
            if let Some((other_input, other_repo)) = other_pair {
                let default_path =
                    to_non_root_path("other_path").context("The hardcoded path was not valid")?;
                load_diff_file(ctx, other_repo, other_input, default_path, &diff_file_opts).await
            } else {
                Ok(LoadDiffFileResult {
                    diff_file: None,
                    is_binary: false,
                    kind: LoadedDiffFileKind::Other,
                })
            }
        }
    )?;

    let rendered_lfs_non_text_sentinel =
        render_lfs_non_text_sentinel(&mut base_result, &mut other_result);

    let has_binary_input = base_result.is_binary || other_result.is_binary;

    let (base_file, other_file) = (base_result.diff_file, other_result.diff_file);

    if base_file.is_none() && other_file.is_none() {
        return Err(DiffError::empty_inputs());
    }

    let (base_file, other_file) =
        if !has_binary_input && !rendered_lfs_non_text_sentinel && options.ignore_whitespace {
            (
                base_file.map(strip_whitespace_from_diff_file),
                other_file.map(strip_whitespace_from_diff_file),
            )
        } else {
            (base_file, other_file)
        };

    let xdiff_opts = xdiff::DiffOpts::from(options);
    let raw_diff =
        mononoke::spawn_blocking(move || xdiff::diff_unified(base_file, other_file, xdiff_opts))
            .await
            .map_err(|e| DiffError::internal(anyhow::anyhow!("spawn_blocking failed: {e}")))?;

    Ok(UnifiedDiff {
        raw_diff,
        is_binary: has_binary_input && !rendered_lfs_non_text_sentinel,
    })
}

fn to_non_root_path(path: &str) -> Result<NonRootMPath, DiffError> {
    let mpath = MPath::new(path)?;
    let non_root_mpath = NonRootMPath::try_from(mpath)?;
    Ok(non_root_mpath)
}

/// When a path flips between non-text content and an LFS pointer, replace the
/// non-text side's contents with a short textual sentinel so the diff surfaces
/// the LFS state transition instead of dumping raw bytes. Returns whether the
/// sentinel was rendered.
fn render_lfs_non_text_sentinel(
    base: &mut LoadDiffFileResult,
    other: &mut LoadDiffFileResult,
) -> bool {
    let non_text_side = match (base.kind, other.kind) {
        (LoadedDiffFileKind::NonText, LoadedDiffFileKind::LfsPointer) => Some(base),
        (LoadedDiffFileKind::LfsPointer, LoadedDiffFileKind::NonText) => Some(other),
        _ => None,
    };

    if let Some(file) = non_text_side.and_then(|side| side.diff_file.as_mut()) {
        file.contents =
            xdiff::FileContent::Inline(Bytes::from_static(LFS_NON_TEXT_SENTINEL.as_bytes()));
        true
    } else {
        false
    }
}

/// Strip horizontal whitespace from inline content in a DiffFile
fn strip_whitespace_from_diff_file(
    file: xdiff::DiffFile<String, Bytes>,
) -> xdiff::DiffFile<String, Bytes> {
    let contents = match file.contents {
        xdiff::FileContent::Inline(bytes) => {
            let stripped = strip_horizontal_whitespace(&bytes);
            xdiff::FileContent::Inline(stripped)
        }
        // For Omitted and Submodule, no whitespace stripping needed
        other => other,
    };

    xdiff::DiffFile {
        path: file.path,
        contents,
        file_type: file.file_type,
    }
}

#[cfg(test)]
mod tests {

    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use test_repo_factory;
    use tests_utils::BasicTestRepo;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::types::DiffCopyInfo;
    use crate::types::DiffFileType;
    use crate::types::DiffInputChangesetPath;
    use crate::types::DiffSingleInput;

    async fn init_test_repo(ctx: &CoreContext) -> Result<BasicTestRepo, DiffError> {
        let repo = test_repo_factory::build_empty(ctx.fb)
            .await
            .map_err(DiffError::internal)?;
        Ok(repo)
    }

    async fn init_test_repo_with_lfs(ctx: &CoreContext) -> Result<BasicTestRepo, DiffError> {
        let mut factory =
            test_repo_factory::TestRepoFactory::new(ctx.fb).map_err(DiffError::internal)?;
        factory.with_config_override(|config| {
            config.git_configs.git_lfs_interpret_pointers = true;
        });
        let repo = factory.build().await.map_err(DiffError::internal)?;
        Ok(repo)
    }

    #[mononoke::fbinit_test]
    async fn test_unified_basic(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create test commits with different content
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("file.txt", "line1\nmodified line2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        // Test the unified diff function
        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: to_non_root_path("file.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path("file.txt")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: false,
        };

        let diff = unified(
            &ctx,
            Some((base_input, &repo)),
            Some((other_input, &repo)),
            options,
        )
        .await?;

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);

        // The unified diff should contain the change we made
        assert!(diff_str.contains("-line2"));
        assert!(diff_str.contains("+modified line2"));
        assert!(diff_str.contains("@@ -1,3 +1,3 @@"));
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_binary_files(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

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
            path: to_non_root_path("binary.bin")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path("binary.bin")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: false,
        };

        let diff = unified(
            &ctx,
            Some((base_input, &repo)),
            Some((other_input, &repo)),
            options,
        )
        .await?;

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(
            diff_str,
            "diff --git a/binary.bin b/binary.bin\nBinary file binary.bin has changed\n"
        );
        assert!(diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_empty_files(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Test with one empty file and one with content
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("new_file.txt", "new content\nline2\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: to_non_root_path("new_file.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path("new_file.txt")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: false,
        };

        let diff = unified(
            &ctx,
            Some((base_input, &repo)),
            Some((other_input, &repo)),
            options,
        )
        .await?;

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("+new content"));
        assert!(diff_str.contains("+line2"));
        assert!(diff_str.contains("@@ -0,0 +1,2 @@"));
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_omit_content(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("file.txt", "line1\nmodified line2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: to_non_root_path("file.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path("file.txt")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: true,
            ignore_whitespace: false,
        };

        let diff = unified(
            &ctx,
            Some((base_input, &repo)),
            Some((other_input, &repo)),
            options,
        )
        .await?;

        // When omit = true, xdiff assumes that we don't want to display the content because they
        // are binaries.
        let expected = "diff --git a/file.txt b/file.txt\nBinary file file.txt has changed\n";
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(diff_str, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_with_none_inputs(fb: FacebookInit) -> Result<(), DiffError> {
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
            path: to_non_root_path("file.txt")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: false,
        };

        // Test None vs Some - should show addition
        let diff = unified(
            &ctx,
            None::<(DiffSingleInput, &BasicTestRepo)>,
            Some((input.clone(), &repo)),
            options.clone(),
        )
        .await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("+some content"));
        assert!(diff_str.contains("+line2"));
        assert!(!diff.is_binary);

        // Test Some vs None - should show deletion
        let diff = unified(
            &ctx,
            Some((input, &repo)),
            None::<(DiffSingleInput, &BasicTestRepo)>,
            options.clone(),
        )
        .await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("-some content"));
        assert!(diff_str.contains("-line2"));
        assert!(!diff.is_binary);

        // Test None vs None - should return an error
        let result = unified(
            &ctx,
            None::<(DiffSingleInput, &BasicTestRepo)>,
            None::<(DiffSingleInput, &BasicTestRepo)>,
            options,
        )
        .await;
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert_eq!(
            error_message,
            "All inputs to the headerless diff were empty"
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_lfs_inspect_pointers(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo_with_lfs(&ctx).await?;

        use mononoke_types::FileType;
        use mononoke_types::GitLfs;

        // Create test commits with LFS files containing different content
        // Pass the actual large file content, not LFS pointers - the system will create pointers automatically
        let base_content =
            "This is a large file that should be stored in LFS backend. It contains base content.";
        let other_content = "This is a large file that should be stored in LFS backend. It contains modified content.";

        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file_with_type_and_lfs(
                "large_file.bin",
                base_content,
                FileType::Regular,
                GitLfs::canonical_pointer(),
            )
            .commit()
            .await?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file_with_type_and_lfs(
                "large_file.bin",
                other_content,
                FileType::Regular,
                GitLfs::canonical_pointer(),
            )
            .commit()
            .await?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: to_non_root_path("large_file.bin")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path("large_file.bin")?,
            replacement_path: None,
        });

        // Test with inspect_lfs_pointers = true (should load actual content and diff it)
        let options_inspect_true = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: false,
        };

        let diff = unified(
            &ctx,
            Some((base_input.clone(), &repo)),
            Some((other_input.clone(), &repo)),
            options_inspect_true,
        )
        .await?;
        let diff_str_true = String::from_utf8_lossy(&diff.raw_diff);

        // Test with inspect_lfs_pointers = true (should load and diff actual content)
        assert!(!diff.is_binary);
        assert_eq!(
            diff_str_true,
            r#"diff --git a/large_file.bin b/large_file.bin
--- a/large_file.bin
+++ b/large_file.bin
@@ -1,1 +1,1 @@
-This is a large file that should be stored in LFS backend. It contains base content.
\ No newline at end of file
+This is a large file that should be stored in LFS backend. It contains modified content.
\ No newline at end of file
"#
        );

        // Test with inspect_lfs_pointers = false (should compare LFS pointers, not content)
        let options_inspect_false = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: false,
            omit_content: false,
            ignore_whitespace: false,
        };

        let diff = unified(
            &ctx,
            Some((base_input, &repo)),
            Some((other_input, &repo)),
            options_inspect_false,
        )
        .await?;
        let diff_str_false = String::from_utf8_lossy(&diff.raw_diff);

        // Should show diff of LFS pointers themselves, not the actual content
        assert!(!diff.is_binary);
        assert_eq!(
            diff_str_false,
            r#"diff --git a/large_file.bin b/large_file.bin
--- a/large_file.bin
+++ b/large_file.bin
@@ -1,3 +1,3 @@
 version https://git-lfs.github.com/spec/v1
-oid sha256:5b7f3960805f82ce1c00d3206b7de147124a8d3e40df3878effa1cbc96cc25a6
-size 84
+oid sha256:f37ab305e2cc7f495449ba70a65b1bdc833247fd4f3c21c1706bef0f3a3c6406
+size 88
"#
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_string_inputs(fb: FacebookInit) -> Result<(), DiffError> {
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

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: false,
        };

        let diff = unified(
            &ctx,
            Some((base_input, &repo)),
            Some((other_input, &repo)),
            options,
        )
        .await?;

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);

        // The unified diff should contain the change we made
        assert!(diff_str.contains("-line2"));
        assert!(diff_str.contains("+modified line2"));
        assert!(diff_str.contains("@@ -1,3 +1,3 @@"));
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_string_vs_none(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        let string_input = DiffSingleInput::String(DiffInputString {
            content: "some content\nline2\nline3\n".to_string(),
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: false,
        };

        // Test None vs String - should show addition
        let diff = unified(
            &ctx,
            None::<(DiffSingleInput, &BasicTestRepo)>,
            Some((string_input.clone(), &repo)),
            options.clone(),
        )
        .await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("+some content"));
        assert!(diff_str.contains("+line2"));
        assert!(diff_str.contains("+line3"));
        assert!(!diff.is_binary);

        // Test String vs None - should show deletion
        let diff = unified(
            &ctx,
            Some((string_input, &repo)),
            None::<(DiffSingleInput, &BasicTestRepo)>,
            options,
        )
        .await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("-some content"));
        assert!(diff_str.contains("-line2"));
        assert!(diff_str.contains("-line3"));
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_ignore_whitespace_only(fb: FacebookInit) -> Result<(), DiffError> {
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
        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: false,
        };
        let diff = unified(
            &ctx,
            Some((base_input.clone(), &repo)),
            Some((other_input.clone(), &repo)),
            options,
        )
        .await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(
            diff_str,
            r#"diff --git a/base_path b/other_path
--- a/base_path
+++ b/other_path
@@ -1,2 +1,2 @@
-hello world
-foo bar
+hello  world
+foo	bar
"#
        );

        // With ignore_whitespace: true, should show no differences
        // (After stripping whitespace, "helloworld\nfoobar\n" should match on both sides)
        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: true,
        };
        let diff = unified(
            &ctx,
            Some((base_input, &repo)),
            Some((other_input, &repo)),
            options,
        )
        .await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        // After stripping whitespace, content should be identical - no changes
        assert_eq!(diff_str, "");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_ignore_whitespace_mixed(fb: FacebookInit) -> Result<(), DiffError> {
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
        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: true,
        };
        let diff = unified(
            &ctx,
            Some((base_input, &repo)),
            Some((other_input, &repo)),
            options,
        )
        .await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        // Should show a diff because there's actual content difference beyond whitespace
        assert_eq!(
            diff_str,
            r#"diff --git a/base_path b/other_path
--- a/base_path
+++ b/other_path
@@ -1,3 +1,3 @@
 line1
-line2
+modifiedline2
 line3
"#
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_ignore_whitespace_binary(fb: FacebookInit) -> Result<(), DiffError> {
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

        // Even with ignore_whitespace: true, binary files should be detected as binary
        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: true,
        };
        let diff = unified(
            &ctx,
            Some((base_input, &repo)),
            Some((other_input, &repo)),
            options,
        )
        .await?;

        // Should be detected as binary
        assert!(diff.is_binary);
        assert_eq!(
            String::from_utf8_lossy(&diff.raw_diff),
            "diff --git a/base_path b/other_path\nBinary files a/base_path and b/other_path differ\n"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_binary_base_text_other(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.bin", b"binary\x00content".as_slice())
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("file.bin", "now this is text\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: to_non_root_path("file.bin")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path("file.bin")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
            ignore_whitespace: false,
        };

        let diff = unified(
            &ctx,
            Some((base_input, &repo)),
            Some((other_input, &repo)),
            options,
        )
        .await?;

        assert!(
            diff.is_binary,
            "is_binary should be true when base is binary"
        );

        Ok(())
    }

    /// When: compute the unified diff for `path` across two changesets with
    /// `inspect_lfs_pointers = false`, so the LFS side renders as pointer text
    /// and the flip becomes a `NonText` vs `LfsPointer` pairing.
    async fn when_unified_diff_without_inspecting_pointers(
        ctx: &CoreContext,
        repo: &BasicTestRepo,
        base_cs: mononoke_types::ChangesetId,
        other_cs: mononoke_types::ChangesetId,
        path: &str,
    ) -> Result<UnifiedDiff, DiffError> {
        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: to_non_root_path(path)?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path(path)?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: false,
            omit_content: false,
            ignore_whitespace: false,
        };

        unified(
            ctx,
            Some((base_input, repo)),
            Some((other_input, repo)),
            options,
        )
        .await
    }

    /// Then: the non-text side is replaced by the sentinel (no raw bytes leak)
    /// and the flip is not reported as binary. `sentinel_on_minus` is true when
    /// the base (`-`) side is the non-text one (enabling LFS), false otherwise.
    fn then_flip_is_rendered_as_sentinel(diff: &UnifiedDiff, sentinel_on_minus: bool) {
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);

        assert!(!diff.is_binary, "LFS flip reported as binary: {diff_str}");
        assert!(
            !diff_str.contains("raw binary here"),
            "raw content leaked into diff: {diff_str}"
        );
        assert!(!diff_str.contains('\u{0}'), "NUL byte leaked: {diff_str}");

        let sentinel = "Git LFS: none (non-text content omitted)";
        let pointer = "version https://git-lfs.github.com/spec/v1";
        if sentinel_on_minus {
            assert!(diff_str.contains(&format!("-{sentinel}")), "{diff_str}");
            assert!(diff_str.contains(&format!("+{pointer}")), "{diff_str}");
        } else {
            assert!(diff_str.contains(&format!("+{sentinel}")), "{diff_str}");
            assert!(diff_str.contains(&format!("-{pointer}")), "{diff_str}");
        }
    }

    #[mononoke::fbinit_test]
    async fn test_unified_lfs_flip_binary_to_pointer(fb: FacebookInit) -> Result<(), DiffError> {
        use mononoke_types::FileType;
        use mononoke_types::GitLfs;

        // Given: a path that is raw binary content (NUL byte, so non-text) in the
        // base commit and an LFS pointer to the same bytes in the other commit.
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo_with_lfs(&ctx).await?;
        let shared = b"\x00raw binary here".to_vec();
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("flip.bin", shared.clone())
            .commit()
            .await?;
        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file_with_type_and_lfs(
                "flip.bin",
                shared,
                FileType::Regular,
                GitLfs::canonical_pointer(),
            )
            .commit()
            .await?;

        // When: we diff the flip without inspecting LFS pointers.
        let diff = when_unified_diff_without_inspecting_pointers(
            &ctx, &repo, base_cs, other_cs, "flip.bin",
        )
        .await?;

        // Then: the raw binary side (`-`, enabling LFS) shows the sentinel.
        then_flip_is_rendered_as_sentinel(&diff, /* sentinel_on_minus */ true);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_lfs_flip_pointer_to_binary(fb: FacebookInit) -> Result<(), DiffError> {
        use mononoke_types::FileType;
        use mononoke_types::GitLfs;

        // Given: the reverse direction -- an LFS pointer in the base commit and
        // raw binary content in the other. Exercises the `(LfsPointer, NonText)`
        // match arm.
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo_with_lfs(&ctx).await?;
        let shared = b"\x00raw binary here".to_vec();
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file_with_type_and_lfs(
                "flip.bin",
                shared.clone(),
                FileType::Regular,
                GitLfs::canonical_pointer(),
            )
            .commit()
            .await?;
        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("flip.bin", shared)
            .commit()
            .await?;

        // When: we diff the flip without inspecting LFS pointers.
        let diff = when_unified_diff_without_inspecting_pointers(
            &ctx, &repo, base_cs, other_cs, "flip.bin",
        )
        .await?;

        // Then: the raw binary side (`+`, disabling LFS) shows the sentinel.
        then_flip_is_rendered_as_sentinel(&diff, /* sentinel_on_minus */ false);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_lfs_flip_nonutf8_to_pointer(fb: FacebookInit) -> Result<(), DiffError> {
        use mononoke_types::FileType;
        use mononoke_types::GitLfs;

        // Given: a raw side with no NUL byte but invalid UTF-8 (leading 0xFF), so
        // it is classified non-text via `is_utf8` rather than the NUL-based
        // binary check -- the case the legacy NUL-only check would have leaked.
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo_with_lfs(&ctx).await?;
        let shared = b"\xffraw binary here".to_vec();
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("flip.bin", shared.clone())
            .commit()
            .await?;
        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file_with_type_and_lfs(
                "flip.bin",
                shared,
                FileType::Regular,
                GitLfs::canonical_pointer(),
            )
            .commit()
            .await?;

        // When: we diff the flip without inspecting LFS pointers.
        let diff = when_unified_diff_without_inspecting_pointers(
            &ctx, &repo, base_cs, other_cs, "flip.bin",
        )
        .await?;

        // Then: the non-UTF-8 side (`-`, enabling LFS) shows the sentinel.
        then_flip_is_rendered_as_sentinel(&diff, /* sentinel_on_minus */ true);
        Ok(())
    }
}
