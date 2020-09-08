/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryFrom;

use anyhow::{Context, Result};
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::HookConfig;
use mononoke_types::{FileChange, MPath};

use super::LuaPattern;
use crate::{FileContentFetcher, FileHook, HookExecution, HookRejectionInfo};

#[derive(Default)]
pub struct DenyFilesBuilder {
    deny_patterns: Option<Vec<String>>,
}

impl DenyFilesBuilder {
    pub fn set_from_config(mut self, config: &HookConfig) -> Self {
        if let Some(v) = config.string_lists.get("deny_patterns") {
            self = self.deny_patterns(v)
        }
        self
    }

    pub fn deny_patterns(mut self, strs: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.deny_patterns = Some(strs.into_iter().map(|s| String::from(s.as_ref())).collect());
        self
    }

    pub fn build(self) -> Result<DenyFiles> {
        Ok(DenyFiles {
            deny_patterns: self
                .deny_patterns
                .unwrap_or_else(Vec::new)
                .into_iter()
                .map(LuaPattern::try_from)
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to create LuaPattern for deny_patterns")?,
        })
    }
}

pub struct DenyFiles {
    deny_patterns: Vec<LuaPattern>,
}

impl DenyFiles {
    pub fn builder() -> DenyFilesBuilder {
        DenyFilesBuilder::default()
    }
}

#[async_trait]
impl FileHook for DenyFiles {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _content_fetcher: &'fetcher dyn FileContentFetcher,
        change: Option<&'change FileChange>,
        path: &'path MPath,
    ) -> Result<HookExecution> {
        if change.is_none() {
            // It is acceptable to delete any file
            return Ok(HookExecution::Accepted);
        }

        let path = path.to_string();
        for pattern in &self.deny_patterns {
            if pattern.is_match(&path) {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Denied filename matched name pattern",
                    format!(
                        "Denied filename '{}' matched name pattern '{}'. Rename or remove this file and try again.",
                        path, pattern
                    ),
                )));
            }
        }
        Ok(HookExecution::Accepted)
    }
}
