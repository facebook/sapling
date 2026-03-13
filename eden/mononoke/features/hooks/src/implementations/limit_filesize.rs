/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::BasicFileChange;
use mononoke_types::NonRootMPath;
use regex::Regex;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;

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
                path_regexes_with_limits: regexes.into_iter().zip(limits).collect(),
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
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
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

        let path = path.to_string();
        let change = match change {
            Some(c) => c,
            None => return Ok(HookExecution::Accepted),
        };

        if change.git_lfs().is_lfs_pointer() {
            // LFS pointers are not stored in the repo, so for now they don't count towards the limit
            // We might want to revise this policy in the future.
            return Ok(HookExecution::Accepted);
        }

        let len = hook_repo
            .get_file_metadata(ctx, change.content_id())
            .await?
            .total_size;
        for (regex, maybe_limit) in &self.path_regexes_with_limits {
            if !regex.is_match(&path) {
                continue;
            }
            match maybe_limit {
                None => return Ok(HookExecution::Accepted),
                Some(limit) if len <= *limit => return Ok(HookExecution::Accepted),
                Some(limit) => {
                    let ratio = len as f64 / *limit as f64;
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "File too large",
                        format!(
                            "File size limit is {} bytes. You tried to push file {} that is over the limit \
                            ({} bytes, {:.2}x the limit). This limit is enforced for files matching the \
                            following regex: \"{}\".\n\
                            \n\
                            WHY THIS IS BLOCKED: Large files have ongoing infrastructure costs — they impact \
                            caching systems, Mononoke, biggrep indexing, and permanent backups used by \
                            30,000+ engineers.\n\
                            \n\
                            ALTERNATIVES TO CONSIDER:\n\
                            - Manifold: Store large binaries in blob storage\n\
                            - Dotslash: Distribute large tools without checking them in\n\
                            - Buckify: Package binaries as Buck-managed dependencies\n\
                            - LFS: Use Git LFS for large files that must be versioned\n\
                            - Split files: Break large files into smaller pieces\n\
                            \n\
                            IF ALTERNATIVES DO NOT WORK:\n\
                            1. Add @allow-large-files to your commit message \
                            (using `sl amend -e`).\n\
                            2. Request bypass approval at https://fburl.com/support/sourcecontrol.\n\
                            \n\
                            See https://fburl.com/landing_big_diffs for more details.",
                            limit, path, len, ratio, regex
                        ),
                    )));
                }
            }
        }
        Ok(HookExecution::Accepted)
    }
}
