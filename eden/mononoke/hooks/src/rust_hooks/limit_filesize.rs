/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{
    CrossRepoPushSource, FileContentFetcher, FileHook, HookConfig, HookExecution, HookRejectionInfo,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::{FileChange, MPath};
use regex::Regex;

#[derive(Default)]
pub struct LimitFilesizeBuilder {
    file_size_limit: Option<u64>,
    ignore_path_regexes: Option<Vec<String>>,
}

impl LimitFilesizeBuilder {
    pub fn set_from_config(mut self, config: &HookConfig) -> Self {
        if let Some(v) = config.ints.get("filesizelimit") {
            self = self.file_size_limit(*v as u64)
        }
        if let Some(v) = config.string_lists.get("ignore_path_regexes") {
            self = self.ignore_path_regexes(v)
        }
        self
    }

    pub fn file_size_limit(mut self, limit: u64) -> Self {
        self.file_size_limit = Some(limit);
        self
    }

    pub fn ignore_path_regexes(mut self, strs: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.ignore_path_regexes =
            Some(strs.into_iter().map(|s| String::from(s.as_ref())).collect());
        self
    }

    pub fn build(self) -> Result<LimitFilesize> {
        Ok(LimitFilesize {
            file_size_limit: self
                .file_size_limit
                .ok_or_else(|| anyhow!("Missing filesizelimit config"))?,
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

pub struct LimitFilesize {
    file_size_limit: u64,
    ignore_path_regexes: Vec<Regex>,
}

impl LimitFilesize {
    pub fn builder() -> LimitFilesizeBuilder {
        LimitFilesizeBuilder::default()
    }
}

#[async_trait]
impl FileHook for LimitFilesize {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_fetcher: &'fetcher dyn FileContentFetcher,
        change: Option<&'change FileChange>,
        path: &'path MPath,
        cross_repo_push_source: CrossRepoPushSource,
    ) -> Result<HookExecution> {
        if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
            // For push-redirected commits, we rely on running source-repo hooks
            return Ok(HookExecution::Accepted);
        }

        let path = format!("{}", path);
        if self
            .ignore_path_regexes
            .iter()
            .any(|regex| regex.is_match(&path))
        {
            return Ok(HookExecution::Accepted);
        }

        if let Some(change) = change {
            let len = content_fetcher
                .get_file_size(ctx, change.content_id())
                .await?;
            if len > self.file_size_limit {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "File too large",
                    format!(
                        "File size limit is {} bytes. \
You tried to push file {} that is over the limit ({} bytes).  See https://fburl.com/landing_big_diffs for instructions.",
                        self.file_size_limit, path, len
                    ),
                )));
            }
        }
        Ok(HookExecution::Accepted)
    }
}
