/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use context::CoreContext;
use futures::try_join;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;

use crate::error::DiffError;
use crate::types::DiffSingleInput;
use crate::types::Repo;
use crate::types::UnifiedDiff;
use crate::types::UnifiedDiffOpts;
use crate::utils::content::DiffFileOpts;
use crate::utils::content::load_diff_file;
use crate::utils::whitespace::strip_horizontal_whitespace;

pub async fn unified(
    ctx: &CoreContext,
    repo: &impl Repo,
    base: Option<DiffSingleInput>,
    other: Option<DiffSingleInput>,
    options: UnifiedDiffOpts,
) -> Result<UnifiedDiff, DiffError> {
    let diff_file_opts = DiffFileOpts {
        file_type: options.file_type,
        inspect_lfs_pointers: options.inspect_lfs_pointers,
        omit_content: options.omit_content,
    };

    let (base_file, other_file) = try_join!(
        async {
            if let Some(base_input) = &base {
                let default_path =
                    to_non_root_path("base_path").context("The hardcoded path was not valid")?;
                load_diff_file(ctx, repo, base_input, default_path, &diff_file_opts).await
            } else {
                Ok(None)
            }
        },
        async {
            if let Some(other_input) = &other {
                let default_path =
                    to_non_root_path("other_path").expect("The hardcoded path was not valid");
                load_diff_file(ctx, repo, other_input, default_path, &diff_file_opts).await
            } else {
                Ok(None)
            }
        }
    )?;

    if base_file.is_none() && other_file.is_none() {
        return Err(DiffError::empty_inputs());
    }

    let is_binary = xdiff::file_is_binary(&base_file) || xdiff::file_is_binary(&other_file);

    let (base_file, other_file) = if !is_binary && options.ignore_whitespace {
        (
            base_file.map(strip_whitespace_from_diff_file),
            other_file.map(strip_whitespace_from_diff_file),
        )
    } else {
        (base_file, other_file)
    };

    let xdiff_opts = xdiff::DiffOpts::from(options);
    let raw_diff =
        tokio::task::spawn_blocking(move || xdiff::diff_unified(base_file, other_file, xdiff_opts))
            .await
            .map_err(|e| DiffError::internal(anyhow::anyhow!("spawn_blocking failed: {}", e)))?;

    Ok(UnifiedDiff {
        raw_diff,
        is_binary,
    })
}

fn to_non_root_path(path: &str) -> Result<NonRootMPath, DiffError> {
    let mpath = MPath::new(path)?;
    let non_root_mpath = NonRootMPath::try_from(mpath)?;
    Ok(non_root_mpath)
}

/// Strip horizontal whitespace from inline content in a DiffFile
fn strip_whitespace_from_diff_file(
    file: xdiff::DiffFile<String, Vec<u8>>,
) -> xdiff::DiffFile<String, Vec<u8>> {
    let contents = match file.contents {
        xdiff::FileContent::Inline(bytes) => {
            let bytes_ref = bytes::Bytes::from(bytes);
            let stripped = strip_horizontal_whitespace(&bytes_ref);
            xdiff::FileContent::Inline(stripped.to_vec())
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

        let diff = unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

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

        let diff = unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

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

        let diff = unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

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

        let diff = unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

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
        let diff = unified(&ctx, &repo, None, Some(input.clone()), options.clone()).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("+some content"));
        assert!(diff_str.contains("+line2"));
        assert!(!diff.is_binary);

        // Test Some vs None - should show deletion
        let diff = unified(&ctx, &repo, Some(input), None, options.clone()).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("-some content"));
        assert!(diff_str.contains("-line2"));
        assert!(!diff.is_binary);

        // Test None vs None - should return an error
        let result = unified(&ctx, &repo, None, None, options).await;
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
            &repo,
            Some(base_input.clone()),
            Some(other_input.clone()),
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
            &repo,
            Some(base_input),
            Some(other_input),
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

        let diff = unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

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
            &repo,
            None,
            Some(string_input.clone()),
            options.clone(),
        )
        .await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("+some content"));
        assert!(diff_str.contains("+line2"));
        assert!(diff_str.contains("+line3"));
        assert!(!diff.is_binary);

        // Test String vs None - should show deletion
        let diff = unified(&ctx, &repo, Some(string_input), None, options).await?;
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
            &repo,
            Some(base_input.clone()),
            Some(other_input.clone()),
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
        let diff = unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;
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
        let diff = unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;
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
        let diff = unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

        // Should be detected as binary
        assert!(diff.is_binary);
        assert_eq!(
            String::from_utf8_lossy(&diff.raw_diff),
            "diff --git a/base_path b/other_path\nBinary files a/base_path and b/other_path differ\n"
        );

        Ok(())
    }
}
