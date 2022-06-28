/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::CrossRepoPushSource;
use crate::FileContentManager;
use crate::FileHook;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::BasicFileChange;
use mononoke_types::MPath;
use regex::Regex;

#[derive(Default)]
pub struct LimitFilesizeBuilder {
    path_regexes: Option<Vec<String>>,
    limits: Option<Vec<i32>>,
}

impl LimitFilesizeBuilder {
    pub fn set_from_config(mut self, config: &HookConfig) -> Self {
        if let Some(v) = config.string_lists.get("filesize_limits_regexes") {
            self = self.filesize_limits_regexes(v)
        }

        if let Some(v) = config.int_lists.get("filesize_limits_values") {
            self.limits = Some(v.clone())
        }

        self
    }

    pub fn filesize_limits_regexes(
        mut self,
        strs: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.path_regexes = Some(strs.into_iter().map(|s| String::from(s.as_ref())).collect());
        self
    }

    pub fn build(self) -> Result<LimitFilesize> {
        if let (Some(regexes_str), Some(limits)) = (self.path_regexes, self.limits) {
            if regexes_str.is_empty() || limits.is_empty() {
                return Err(anyhow!(
                    "Failed to initialize limit_filesize hook. Either 'filesize_limits_regexes' or 'filesize_limits_values' list is empty."
                ));
            }
            let regexes = regexes_str
                .into_iter()
                .map(|s| Regex::new(&s))
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to create regex for path_regexes")?;

            let limits: Vec<Option<u64>> = limits.into_iter().map(|n| n.try_into().ok()).collect();

            return Ok(LimitFilesize {
                path_regexes_with_limits: regexes.into_iter().zip(limits.into_iter()).collect(),
            });
        }
        Err(anyhow!(
            "Failed to initialize limit_filesize hook. Either 'filesize_limits_regexes' or 'filesize_limits_values' option is missing."
        ))
    }
}

pub struct LimitFilesize {
    path_regexes_with_limits: Vec<(Regex, Option<u64>)>,
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
        content_manager: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        path: &'path MPath,
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

        let path = format!("{}", path);
        let change = match change {
            Some(c) => c,
            None => return Ok(HookExecution::Accepted),
        };

        let len = content_manager
            .get_file_size(ctx, change.content_id())
            .await?;
        for (regex, maybe_limit) in &self.path_regexes_with_limits {
            if !regex.is_match(&path) {
                continue;
            }
            match maybe_limit {
                None => return Ok(HookExecution::Accepted),
                Some(limit) if len <= *limit => return Ok(HookExecution::Accepted),
                Some(limit) => {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "File too large",
                        format!(
                            "File size limit is {} bytes. You tried to push file {} that is over the limit ({} bytes). This limit is enforced for files matching the following regex: \"{}\". See https://fburl.com/landing_big_diffs for instructions.",
                            limit, path, len, regex
                        ),
                    )));
                }
            }
        }
        Ok(HookExecution::Accepted)
    }
}
