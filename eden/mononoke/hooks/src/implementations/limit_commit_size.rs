/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;
use regex::Regex;
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookFileContentProvider;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

#[derive(Deserialize, Clone, Debug)]
pub struct LimitCommitSizeConfig {
    /// The total number of bytes of added or modified files.
    #[serde(default)]
    commit_size_limit: Option<u64>,

    /// The total number of changed files.
    #[serde(default)]
    changed_files_limit: Option<u64>,

    /// Ignore paths.  These paths will be ignored for the purposes of
    /// calculating the commit size or number of files.
    #[serde(default, with = "serde_regex")]
    ignore_path_regexes: Vec<Regex>,

    /// Path-based overrides.  The limits can be increased if paths match
    /// specific values.
    #[serde(default)]
    path_overrides: Vec<LimitCommitSizeOverride>,

    /// Message to include in the hook rejection if the changed files limit is
    /// exceeded, with the following replacements:
    ///    ${limit} => the limit used
    ///    ${count} => the number of changed files
    too_many_files_message: String,

    /// Message to include in the hook rejection if the commit size limit is
    /// exceeded, with the following replacements:
    ///    ${limit} => the limit used in bytes
    ///    ${size} => the size of the commit in bytes
    too_large_message: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct LimitCommitSizeOverride {
    /// This override will increase the size limit if any path matches.
    #[serde(with = "serde_regex")]
    path_regex: Regex,

    /// This override will increase the size limit to at least this value
    /// (other overrides may increase it further).
    #[serde(default)]
    commit_size_limit: Option<u64>,
}

/// Hook to block commits that exceed a size limit, either in terms of bytes
/// or number of files.
#[derive(Clone, Debug)]
pub struct LimitCommitSizeHook {
    config: LimitCommitSizeConfig,
}

pub fn legacy_limit_commitsize(hook_config: &HookConfig) -> Result<LimitCommitSizeHook> {
    let mut config = LimitCommitSizeConfig {
        commit_size_limit: None,
        changed_files_limit: None,
        ignore_path_regexes: Vec::new(),
        path_overrides: Vec::new(),
        too_many_files_message: String::from(concat!(
            "Commit changed ${count} files but at most ${limit} are allowed. ",
            "See https://fburl.com/landing_big_diffs for instructions.",
        )),
        too_large_message: String::from(concat!(
            "Commit size limit is ${limit} bytes.\n",
            "You tried to push a commit ${size} bytes in size that is over the limit.\n",
            "See https://fburl.com/landing_big_diffs for instructions.",
        )),
    };

    // Please note that the _i64 configs override any i32s one with the same key.
    if let Some(v) = hook_config.ints.get("commitsizelimit") {
        config.commit_size_limit = Some(*v as u64);
    }
    if let Some(v) = hook_config.ints_64.get("commitsizelimit") {
        config.commit_size_limit = Some(*v as u64);
    }
    if let Some(v) = hook_config.string_lists.get("ignore_path_regexes") {
        config.ignore_path_regexes = v
            .iter()
            .map(|r| anyhow::Ok(Regex::new(r)?))
            .collect::<Result<_>>()?;
    }
    if let Some(v) = hook_config.ints.get("changed_files_limit") {
        config.changed_files_limit = Some(*v as u64);
    }
    if let Some(v) = hook_config.ints_64.get("changed_files_limit") {
        config.changed_files_limit = Some(*v as u64);
    }
    if let Some(paths) = hook_config.string_lists.get("override_limit_path_regexes") {
        let limits = if let Some(v) = hook_config.int_lists.get("override_limits") {
            v.iter().map(|i| *i as u64).collect::<Vec<_>>()
        } else if let Some(v) = hook_config.int_64_lists.get("override_limits") {
            v.iter().map(|i| *i as u64).collect::<Vec<_>>()
        } else {
            bail!("List 'override_limit_path_regexes' requires list 'override_limits'.");
        };
        if paths.len() != limits.len() {
            bail!(
                "Lists 'override_limit_path_regexes' and 'override_limits' have different sizes."
            );
        }
        config.path_overrides = paths
            .iter()
            .zip(limits.iter())
            .map(|(path, limit)| {
                Ok(LimitCommitSizeOverride {
                    path_regex: Regex::new(path)?,
                    commit_size_limit: Some(*limit),
                })
            })
            .collect::<Result<_>>()?;
    }
    LimitCommitSizeHook::with_config(config)
}

impl LimitCommitSizeHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        let options = config
            .options
            .as_ref()
            .ok_or_else(|| anyhow!("Missing hook options"))?;
        let config = serde_json::from_str(options).context("Invalid hook config")?;
        Self::with_config(config)
    }

    pub fn with_config(config: LimitCommitSizeConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl ChangesetHook for LimitCommitSizeHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn HookFileContentProvider,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
            // For push-redirected commits, we rely on running source-repo hooks
            return Ok(HookExecution::Accepted);
        }

        let mut commit_size_limit = self.config.commit_size_limit;
        let mut commit_size = 0;
        let mut changed_files = 0;

        for (path, file_change) in changeset.file_changes() {
            let path = path.to_string();

            if let Some(override_commit_size_limit) = self
                .config
                .path_overrides
                .iter()
                .filter_map(|path_override| {
                    path_override
                        .commit_size_limit
                        .filter(|_| path_override.path_regex.is_match(&path))
                })
                .max()
            {
                commit_size_limit = Some(
                    commit_size_limit.map_or(override_commit_size_limit, |commit_size_limit| {
                        u64::max(commit_size_limit, override_commit_size_limit)
                    }),
                );
            }

            if self
                .config
                .ignore_path_regexes
                .iter()
                .any(|regex| regex.is_match(&path))
            {
                continue;
            }

            changed_files += 1;
            commit_size += file_change.size().unwrap_or(0);
        }

        if let Some(changed_files_limit) = self.config.changed_files_limit {
            if changed_files > changed_files_limit {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Commit too large",
                    self.config
                        .too_many_files_message
                        .replace("${count}", &changed_files.to_string())
                        .replace("${limit}", &changed_files_limit.to_string()),
                )));
            }
        }

        if let Some(commit_size_limit) = commit_size_limit {
            if commit_size > commit_size_limit {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Commit too large",
                    self.config
                        .too_large_message
                        .replace("${size}", &commit_size.to_string())
                        .replace("${limit}", &commit_size_limit.to_string()),
                )));
            }
        }

        Ok(HookExecution::Accepted)
    }
}

#[cfg(test)]
mod test {
    use anyhow::Error;
    use blobstore::Loadable;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use repo_hook_file_content_provider::RepoHookFileContentProvider;
    use tests_utils::BasicTestRepo;
    use tests_utils::CreateCommitContext;

    use super::*;

    /// Create default test config that each test can customize.
    fn make_test_config() -> LimitCommitSizeConfig {
        LimitCommitSizeConfig {
            commit_size_limit: None,
            changed_files_limit: None,
            ignore_path_regexes: Vec::new(),
            path_overrides: Vec::new(),
            too_many_files_message: String::from("Too many files: ${count} > ${limit}."),
            too_large_message: String::from("Commit too large: ${size} > ${limit}."),
        }
    }

    #[fbinit::test]
    async fn test_limit_commit_size(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .add_file("dir/c", "c")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let content_manager = RepoHookFileContentProvider::new(&repo);

        let config = make_test_config();
        let hook = LimitCommitSizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkKey::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);

        let mut config = make_test_config();
        config.commit_size_limit = Some(3);
        config.changed_files_limit = Some(3);
        let hook = LimitCommitSizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkKey::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);

        let mut config = make_test_config();
        config.commit_size_limit = Some(3);
        config.changed_files_limit = Some(1);
        let hook = LimitCommitSizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkKey::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        match hook_execution {
            HookExecution::Rejected(info) => {
                assert_eq!(info.long_description, "Too many files: 3 > 1.");
            }
            HookExecution::Accepted => {
                return Err(anyhow!("should be rejected"));
            }
        };

        let mut config = make_test_config();
        config.commit_size_limit = Some(1);
        config.changed_files_limit = Some(3);
        let hook = LimitCommitSizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkKey::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        match hook_execution {
            HookExecution::Rejected(info) => {
                assert_eq!(info.long_description, "Commit too large: 3 > 1.");
            }
            HookExecution::Accepted => {
                return Err(anyhow!("should be rejected"));
            }
        };
        Ok(())
    }

    #[fbinit::test]
    async fn test_limit_commit_size_removed_files(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let parent_cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .commit()
            .await?;

        let cs_id = CreateCommitContext::new(ctx, repo, vec![parent_cs_id])
            .delete_file("dir/a")
            .delete_file("dir/b")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let content_manager = RepoHookFileContentProvider::new(&repo);
        let mut config = make_test_config();
        config.commit_size_limit = Some(100);
        config.changed_files_limit = Some(2);
        let hook = LimitCommitSizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkKey::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);

        let mut config = make_test_config();
        config.commit_size_limit = Some(100);
        config.changed_files_limit = Some(1);
        let hook = LimitCommitSizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkKey::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        match hook_execution {
            HookExecution::Rejected(info) => {
                assert_eq!(info.long_description, "Too many files: 2 > 1.");
            }
            HookExecution::Accepted => {
                return Err(anyhow!("should be rejected"));
            }
        };
        Ok(())
    }

    #[fbinit::test]
    async fn test_limit_commit_size_override(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .add_file("odir/c", "c")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let content_manager = RepoHookFileContentProvider::new(&repo);
        let mut config = make_test_config();
        config.commit_size_limit = Some(1);
        config.changed_files_limit = Some(3);
        config.path_overrides.push(LimitCommitSizeOverride {
            path_regex: Regex::new("^dir/.*$")?,
            commit_size_limit: Some(3),
        });
        let hook = LimitCommitSizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkKey::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        // override max size is 3 bytes which is enough for 3 bytes commit
        assert_eq!(hook_execution, HookExecution::Accepted);

        Ok(())
    }

    #[fbinit::test]
    async fn test_limit_commit_size_override_hits_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .add_file("odir/c", "c")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let content_manager = RepoHookFileContentProvider::new(&repo);

        let mut config = make_test_config();
        config.commit_size_limit = Some(1);
        config.changed_files_limit = Some(3);
        config.path_overrides.push(LimitCommitSizeOverride {
            path_regex: Regex::new("^odir/.*$")?,
            commit_size_limit: Some(2),
        });
        let hook = LimitCommitSizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkKey::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        // override max size is 2 bytes, but commit has 3 in total
        match hook_execution {
            HookExecution::Rejected(info) => {
                assert_eq!(info.long_description, "Commit too large: 3 > 2.");
            }
            HookExecution::Accepted => {
                return Err(anyhow!("should be rejected"));
            }
        };

        Ok(())
    }

    #[fbinit::test]
    async fn test_limit_commit_size_ignored_files(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .add_file("ignored_dir/project/c", "c")
            .add_file("something/ignored_dir/project/d", "d")
            .add_file("test_ignored_dir/schemas/e", "e")
            .add_file("ignored_dir/schemas/f", "f")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let content_manager = RepoHookFileContentProvider::new(&repo);
        let mut config = make_test_config();
        config.commit_size_limit = Some(2);
        config.changed_files_limit = Some(2);
        config
            .ignore_path_regexes
            .push(Regex::new(r"(^|/)(test_)?ignored_dir/(project|schemas)")?);
        let hook = LimitCommitSizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkKey::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;

        match hook_execution {
            HookExecution::Rejected(_) => {
                return Err(anyhow!(
                    "files in ignored_dir should not count towards limit"
                ));
            }
            HookExecution::Accepted => {}
        };

        Ok(())
    }
}
