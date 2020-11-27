/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{
    ChangesetHook, CrossRepoPushSource, FileContentFetcher, HookConfig, HookExecution,
    HookRejectionInfo,
};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use bookmarks::BookmarkName;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;
use regex::Regex;

#[derive(Default)]
pub struct LimitCommitsizeBuilder {
    commit_size_limit: Option<u64>,
    ignore_path_regexes: Option<Vec<String>>,
    changed_files_limit: Option<u64>,
}

impl LimitCommitsizeBuilder {
    pub fn set_from_config(mut self, config: &HookConfig) -> Self {
        if let Some(v) = config.ints.get("commitsizelimit") {
            self = self.commit_size_limit(*v as u64)
        }
        if let Some(v) = config.string_lists.get("ignore_path_regexes") {
            self = self.ignore_path_regexes(v)
        }
        if let Some(v) = config.ints.get("changed_files_limit") {
            self = self.changed_files_limit(*v as u64)
        }
        self
    }

    pub fn commit_size_limit(mut self, limit: u64) -> Self {
        self.commit_size_limit = Some(limit);
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
        Ok(LimitCommitsize {
            commit_size_limit: self
                .commit_size_limit
                .ok_or_else(|| anyhow!("Missing commitsizelimit config"))?,
            ignore_path_regexes: self
                .ignore_path_regexes
                .unwrap_or_else(Vec::new)
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
        _content_fetcher: &'fetcher dyn FileContentFetcher,
        cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<HookExecution> {
        if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
            // For push-redirected commits, we rely on running source-repo hooks
            return Ok(HookExecution::Accepted);
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
            if let Some(changed_files_limit) = self.changed_files_limit {
                if num_changed_files > changed_files_limit {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "Commit too large",
                        format!(
                            "Commit changed {} files but at most {} are allowed. Reach out to Source control @ fb for instructions.",
                            num_changed_files, changed_files_limit,
                        ),
                    )));
                }
            }

            let file = match file_change {
                None => continue,
                Some(file) => file,
            };

            totalsize += file.size();
            if totalsize > self.commit_size_limit {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Commit too large",
                    format!(
                        "Commit size limit is {} bytes. You tried to push commit that is over the limit. See https://fburl.com/landing_big_diffs for instructions.",
                        self.commit_size_limit
                    ),
                )));
            }
        }
        Ok(HookExecution::Accepted)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::Error;
    use blobrepo_factory::new_memblob_empty;
    use blobstore::Loadable;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use hooks_content_stores::BlobRepoFileContentFetcher;
    use maplit::hashmap;
    use std::collections::HashMap;
    use tests_utils::CreateCommitContext;

    #[fbinit::compat_test]
    async fn test_limitcommitsize(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = new_memblob_empty(None)?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, repo.blobstore()).await?;

        let content_fetcher = BlobRepoFileContentFetcher::new(repo.clone());
        let hook = build_hook(hashmap! {
            "commitsizelimit".to_string() => 2,
            "changed_files_limit".to_string() => 2,
        })?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
                &bcs,
                &content_fetcher,
                CrossRepoPushSource::NativeToThisRepo,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);


        let hook = build_hook(hashmap! {
            "commitsizelimit".to_string() => 2,
            "changed_files_limit".to_string() => 1,
        })?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
                &bcs,
                &content_fetcher,
                CrossRepoPushSource::NativeToThisRepo,
            )
            .await?;
        match hook_execution {
            HookExecution::Rejected(_) => {}
            HookExecution::Accepted => {
                return Err(anyhow!("should be rejected"));
            }
        };

        let hook = build_hook(hashmap! {
            "commitsizelimit".to_string() => 1,
            "changed_files_limit".to_string() => 2,
        })?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
                &bcs,
                &content_fetcher,
                CrossRepoPushSource::NativeToThisRepo,
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

    #[fbinit::compat_test]
    async fn test_limitcommitsize_removed_files(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = new_memblob_empty(None)?;
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

        let bcs = cs_id.load(ctx, repo.blobstore()).await?;

        let content_fetcher = BlobRepoFileContentFetcher::new(repo.clone());
        let hook = build_hook(hashmap! {
            "commitsizelimit".to_string() => 100,
            "changed_files_limit".to_string() => 2,
        })?;
        let hook_execution = hook
            .run(
                ctx,
                &BookmarkName::new("book")?,
                &bcs,
                &content_fetcher,
                CrossRepoPushSource::NativeToThisRepo,
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
                &content_fetcher,
                CrossRepoPushSource::NativeToThisRepo,
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

    fn build_hook(ints: HashMap<String, i32>) -> Result<LimitCommitsize> {
        let config = HookConfig {
            bypass: None,
            strings: hashmap! {},
            ints,
            string_lists: hashmap! {},
            int_lists: hashmap! {},
        };
        let mut builder = LimitCommitsize::builder();
        builder = builder.set_from_config(&config);
        builder.build()
    }
}
