/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::CrossRepoPushSource;
use crate::FileContentManager;
use crate::FileHook;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::HookConfig;
use mononoke_types::BasicFileChange;
use mononoke_types::MPath;
use regex::Regex;

#[derive(Default)]
pub struct NoBadFilenamesBuilder<'a> {
    allowlist_regex: Option<&'a str>,
    illegal_regex: Option<&'a str>,
}

impl<'a> NoBadFilenamesBuilder<'a> {
    pub fn set_from_config(mut self, config: &'a HookConfig) -> Self {
        if let Some(v) = config.strings.get("allowlist_regex") {
            self = self.allowlist_regex(v)
        }
        if let Some(v) = config.strings.get("illegal_regex") {
            self = self.illegal_regex(v)
        }
        self
    }

    pub fn allowlist_regex(mut self, regex: &'a str) -> Self {
        self.allowlist_regex = Some(regex);
        self
    }

    pub fn illegal_regex(mut self, regex: &'a str) -> Self {
        self.illegal_regex = Some(regex);
        self
    }

    pub fn build(self) -> Result<NoBadFilenames> {
        Ok(NoBadFilenames {
            allowlist_regex: self
                .allowlist_regex
                .map(Regex::new)
                .transpose()
                .context("Failed to create regex for allowlist")?,
            illegal_regex: Regex::new(
                self.illegal_regex
                    .ok_or_else(|| anyhow!("Missing illegal_regex config"))?,
            )
            .context("Failed to create regex for illegal")?,
        })
    }
}

pub struct NoBadFilenames {
    allowlist_regex: Option<Regex>,
    illegal_regex: Regex,
}

impl NoBadFilenames {
    pub fn builder<'a>() -> NoBadFilenamesBuilder<'a> {
        NoBadFilenamesBuilder::default()
    }
}

#[async_trait]
impl FileHook for NoBadFilenames {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _content_manager: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        path: &'path MPath,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        if change.is_none() {
            return Ok(HookExecution::Accepted);
        }

        let path = format!("{}", path);
        if self.illegal_regex.is_match(&path) {
            match self.allowlist_regex {
                Some(ref allow) if allow.is_match(&path) => {}
                _ => {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "Illegal filename",
                        format!(
                            "ABORT: Illegal filename: '{}'. Filenames must not match '{}'.",
                            path, self.illegal_regex
                        ),
                    )));
                }
            }
        }
        Ok(HookExecution::Accepted)
    }
}
