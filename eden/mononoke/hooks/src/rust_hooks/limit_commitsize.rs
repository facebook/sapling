/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{ChangesetHook, FileContentFetcher, HookConfig, HookExecution, HookRejectionInfo};

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
}

impl LimitCommitsizeBuilder {
    pub fn set_from_config(mut self, config: &HookConfig) -> Self {
        if let Some(v) = config.ints.get("commitsizelimit") {
            self = self.commit_size_limit(*v as u64)
        }
        if let Some(v) = config.string_lists.get("ignore_path_regexes") {
            self = self.ignore_path_regexes(v)
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
        })
    }
}

pub struct LimitCommitsize {
    commit_size_limit: u64,
    ignore_path_regexes: Vec<Regex>,
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
    ) -> Result<HookExecution> {
        let mut totalsize = 0;
        for (path, file_change) in changeset.file_changes() {
            let file = match file_change {
                None => continue,
                Some(file) => file,
            };

            let path = format!("{}", path);
            if self
                .ignore_path_regexes
                .iter()
                .any(|regex| regex.is_match(&path))
            {
                continue;
            }

            totalsize += file.size();
            if totalsize > self.commit_size_limit {
                return Ok(HookExecution::Rejected(
					HookRejectionInfo::new_long(
						"Commit too large",
						 format!("Commit size limit is {} bytes. You tried to push commit that is over the limit. See https://fburl.com/landing_big_diffs for instructions.", self.commit_size_limit)
						)
					)
				);
            }
        }
        Ok(HookExecution::Accepted)
    }
}
