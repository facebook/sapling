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
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::HookConfig;
use mononoke_types::BasicFileChange;
use mononoke_types::MPath;

#[derive(Default)]
pub struct NoBadExtensionsBuilder {
    illegal_extensions: Option<Vec<String>>,
}

impl NoBadExtensionsBuilder {
    pub fn set_from_config(mut self, config: &HookConfig) -> Self {
        if let Some(v) = config.strings.get("illegal_extensions") {
            self = self.illegal_extensions(v.split(','))
        }
        self
    }

    pub fn illegal_extensions(mut self, strs: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.illegal_extensions =
            Some(strs.into_iter().map(|s| String::from(s.as_ref())).collect());
        self
    }

    pub fn build(self) -> Result<NoBadExtensions> {
        Ok(NoBadExtensions {
            illegal_extensions: self
                .illegal_extensions
                .ok_or_else(|| anyhow!("Missing illegal_extensions config"))?,
        })
    }
}

pub struct NoBadExtensions {
    illegal_extensions: Vec<String>,
}

impl NoBadExtensions {
    pub fn builder() -> NoBadExtensionsBuilder {
        NoBadExtensionsBuilder::default()
    }
}

#[async_trait]
impl FileHook for NoBadExtensions {
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
        let lowercase_path = path.to_lowercase();
        for ext in &self.illegal_extensions {
            if lowercase_path.ends_with(ext) {
                return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Illegal filename",
                    format!(
                        "ABORT: Illegal filename: '{}'. You cannot commit {} files.",
                        path, ext
                    ),
                )));
            }
        }
        Ok(HookExecution::Accepted)
    }
}
