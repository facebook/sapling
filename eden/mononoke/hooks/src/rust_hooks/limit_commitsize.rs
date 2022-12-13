/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkName;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;
use regex::Regex;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileContentManager;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

#[derive(Default)]
pub struct LimitCommitsizeBuilder {
    commit_size_limit: Option<u64>,
    override_limit_path_regexes: Option<Vec<String>>,
    override_limits: Option<Vec<u64>>,
    ignore_path_regexes: Option<Vec<String>>,
    changed_files_limit: Option<u64>,
}

impl LimitCommitsizeBuilder {
    pub fn set_from_config(mut self, config: &HookConfig) -> Self {
        // Please note that the _i64 configs override any i32s one with the same key.
        if let Some(v) = config.ints.get("commitsizelimit") {
            self = self.commit_size_limit(*v as u64)
        }
        if let Some(v) = config.ints_64.get("commitsizelimit") {
            self = self.commit_size_limit(*v as u64)
        }
        if let Some(v) = config.string_lists.get("ignore_path_regexes") {
            self = self.ignore_path_regexes(v)
        }
        if let Some(v) = config.ints.get("changed_files_limit") {
            self = self.changed_files_limit(*v as u64)
        }
        if let Some(v) = config.ints_64.get("changed_files_limit") {
            self = self.changed_files_limit(*v as u64)
        }
        if let Some(v) = config.string_lists.get("override_limit_path_regexes") {
            self = self.override_limit_path_regexes(v);
        }
        if let Some(v) = config.int_lists.get("override_limits") {
            self = self.override_limits(v.iter().map(|i| *i as u64));
        }
        if let Some(v) = config.int_64_lists.get("override_limits") {
            self = self.override_limits(v.iter().map(|i| *i as u64));
        }
        self
    }

    pub fn commit_size_limit(mut self, limit: u64) -> Self {
        self.commit_size_limit = Some(limit);
        self
    }

    pub fn override_limit_path_regexes(
        mut self,
        strs: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.override_limit_path_regexes =
            Some(strs.into_iter().map(|s| String::from(s.as_ref())).collect());
        self
    }

    pub fn override_limits(mut self, limits: impl IntoIterator<Item = u64>) -> Self {
        self.override_limits = Some(limits.into_iter().collect());
        self
    }

    pub fn ignore_path_regexes(mut self, strs: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.ignore_path_regexes =
            Some(strs.into_iter().map(|s| String::from(s.as_ref())).collect());
        self
    }

    pub fn changed_files_limit(mut self, changed_files_limit: u64) -> Self {
        self.changed_files_limit = Some(changed_files_limit);
        self
    }

    pub fn build(self) -> Result<LimitCommitsize> {
        let regexes = self
            .override_limit_path_regexes
            .unwrap_or_default()
            .into_iter()
            .map(|s| Regex::new(&s))
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to create regex for override_limit_path_regexes")?;

        let limits = self
            .override_limits
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();

        if regexes.len() != limits.len() {
            return Err(anyhow!(
                "Failed to initialize limit_commitsize hook. Lists 'override_limit_path_regexes' and 'override_limits' have different sizes."
            ));
        }

        let regexes_with_limits: Vec<(Regex, u64)> =
            regexes.into_iter().zip(limits.into_iter()).collect();

        Ok(LimitCommitsize {
            commit_size_limit: self
                .commit_size_limit
                .ok_or_else(|| anyhow!("Missing commitsizelimit config"))?,
            override_limit_path_regexes_with_limits: regexes_with_limits,
            ignore_path_regexes: self
                .ignore_path_regexes
                .unwrap_or_default()
                .into_iter()
                .map(|s| Regex::new(&s))
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to create regex for ignore_path_regex")?,
            changed_files_limit: self.changed_files_limit,
        })
    }
}

pub struct LimitCommitsize {
    commit_size_limit: u64,
    ignore_path_regexes: Vec<Regex>,
    override_limit_path_regexes_with_limits: Vec<(Regex, u64)>,
    changed_files_limit: Option<u64>,
}

impl LimitCommitsize {
    pub fn builder() -> LimitCommitsizeBuilder {
        LimitCommitsizeBuilder::default()
    }
}

#[async_trait]
impl ChangesetHook for LimitCommitsize {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _bookmark: &BookmarkName,
        changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn FileContentManager,
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

        // find max commit size based on the files in the changeset
        let mut max_commit_size_limit = self.commit_size_limit;
        for (path, _) in changeset.file_changes() {
            let path = format!("{}", path);
            let path_size_limit = self
                .override_limit_path_regexes_with_limits
                .iter()
                .filter(|(regex, _)| regex.is_match(&path))
                .map(|(_, size)| size)
                .max();
            if let Some(limit) = path_size_limit {
                max_commit_size_limit = u64::max(max_commit_size_limit, *limit);
            }
        }

        let mut num_changed_files = 0;
        let mut totalsize = 0;
        for (path, file_change) in changeset.file_changes() {
            let path = format!("{}", path);
            if self
                .ignore_path_regexes
                .iter()
                .any(|regex| regex.is_match(&path))
            {
                continue;
            }

            num_changed_files += 1;
            totalsize += file_change.size().unwrap_or(0);
        }

        if let Some(changed_files_limit) = self.changed_files_limit {
            if num_changed_files > changed_files_limit {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Commit too large",
                    format!(
                        "Commit changed {} files but at most {} are allowed. See https://fburl.com/landing_big_diffs for instructions.",
                        num_changed_files, changed_files_limit,
                    ),
                )));
            }
        }

        if totalsize > max_commit_size_limit {
            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Commit too large",
                format!(
                    "Commit size limit is {} bytes.\n\
                     You tried to push a commit {} bytes in size that is over the limit.\n\
                     See https://fburl.com/landing_big_diffs for instructions.",
                    max_commit_size_limit, totalsize
                ),
            )));
        }

        Ok(HookExecution::Accepted)
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use anyhow::Error;
    use blobstore::Loadable;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use hooks_content_stores::RepoFileContentManager;
    use maplit::hashmap;
    use tests_utils::BasicTestRepo;
    use tests_utils::CreateCommitContext;

    use super::*;

    #[fbinit::test]
    async fn test_limitcommitsize(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .add_file("dir/c", "c")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let content_manager = RepoFileContentManager::new(&repo);
        let hook = build_hook(hashmap! {
            "commitsizelimit".to_string() => 3,
            "changed_files_limit".to_string() => 3,
        })?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);

        let hook = build_hook(hashmap! {
            "commitsizelimit".to_string() => 3,
            "changed_files_limit".to_string() => 1,
        })?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        match hook_execution {
            HookExecution::Rejected(info) => {
                assert!(info.long_description.contains("changed 3 files"));
            }
            HookExecution::Accepted => {
                return Err(anyhow!("should be rejected"));
            }
        };

        let hook = build_hook(hashmap! {
            "commitsizelimit".to_string() => 1,
            "changed_files_limit".to_string() => 3,
        })?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        match hook_execution {
            HookExecution::Rejected(info) => {
                assert!(info.long_description.contains("commit 3 bytes"));
            }
            HookExecution::Accepted => {
                return Err(anyhow!("should be rejected"));
            }
        };
        Ok(())
    }

    #[fbinit::test]
    async fn test_limitcommitsize_removed_files(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;
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

        let content_manager = RepoFileContentManager::new(&repo);
        let hook = build_hook(hashmap! {
            "commitsizelimit".to_string() => 100,
            "changed_files_limit".to_string() => 2,
        })?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);

        let hook = build_hook(hashmap! {
            "commitsizelimit".to_string() => 100,
            "changed_files_limit".to_string() => 1,
        })?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        match hook_execution {
            HookExecution::Rejected(_) => {}
            HookExecution::Accepted => {
                return Err(anyhow!("should be rejected"));
            }
        };
        Ok(())
    }

    #[fbinit::test]
    async fn test_limitcommitsize_override(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .add_file("odir/c", "c")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let content_manager = RepoFileContentManager::new(&repo);
        let hook = build_hook_with_limits(
            hashmap! {
                "commitsizelimit".to_string() => 1,
                "changed_files_limit".to_string() => 3,
            },
            hashmap! {
                "override_limit_path_regexes".to_string() => vec!["^dir/.*$".to_string()],
            },
            hashmap! {
                "override_limits".to_string() => vec![3],
            },
        )?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
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
    async fn test_limitcommitsize_override_hits_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .add_file("odir/c", "c")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let content_manager = RepoFileContentManager::new(&repo);

        let hook = build_hook_with_limits(
            hashmap! {
                "commitsizelimit".to_string() => 1,
                "changed_files_limit".to_string() => 3,
            },
            hashmap! {
                "override_limit_path_regexes".to_string() => vec!["^odir/.*$".to_string()],
            },
            hashmap! {
                "override_limits".to_string() => vec![2],
            },
        )?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
                &bcs,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        // override max size is 2 bytes, but commit has 3 in total
        match hook_execution {
            HookExecution::Rejected(_) => {}
            HookExecution::Accepted => {
                return Err(anyhow!("should be rejected"));
            }
        };

        Ok(())
    }

    fn build_hook(ints_64: HashMap<String, i64>) -> Result<LimitCommitsize> {
        build_hook_with_limits(ints_64, hashmap! {}, hashmap! {})
    }

    fn build_hook_with_limits(
        ints_64: HashMap<String, i64>,
        string_lists: HashMap<String, Vec<String>>,
        int_lists: HashMap<String, Vec<i32>>,
    ) -> Result<LimitCommitsize> {
        let config = HookConfig {
            bypass: None,
            strings: hashmap! {},
            ints_64,
            string_lists,
            int_lists,
            ..Default::default()
        };
        let mut builder = LimitCommitsize::builder();
        builder = builder.set_from_config(&config);
        builder.build()
    }
}
