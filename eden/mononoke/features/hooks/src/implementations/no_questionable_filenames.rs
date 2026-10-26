/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::HookConfig;
use mononoke_types::BasicFileChange;
use mononoke_types::NonRootMPath;
use regex::Regex;
use serde::Deserialize;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::hook_options::parse_hook_options;

/// Configuration for the `no_questionable_filenames` hook.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct NoQuestionableFilenamesConfig {
    /// Regex for paths that may contain braces.
    pub allowlist_for_braces: Option<String>,
    /// Regex for paths that may contain whitespace or apostrophes, or have a component that
    /// starts with a hyphen.
    pub allowlist_for_cmd_line: Option<String>,
}

impl NoQuestionableFilenamesConfig {
    /// Builds the config from the `allowlist_for_braces` and `allowlist_for_cmd_line` strings.
    fn from_typed(config: &HookConfig) -> Result<Self> {
        Ok(Self {
            allowlist_for_braces: config.strings.get("allowlist_for_braces").cloned(),
            allowlist_for_cmd_line: config.strings.get("allowlist_for_cmd_line").cloned(),
        })
    }
}

pub struct NoQuestionableFilenames {
    allowlist_for_braces: Option<Regex>,
    braces: Regex,
    allowlist_for_cmd_line: Option<Regex>,
    cmd_line: Regex,
}

impl NoQuestionableFilenames {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(parse_hook_options(
            config,
            "no_questionable_filenames",
            NoQuestionableFilenamesConfig::from_typed,
        )?)
    }

    pub fn with_config(config: NoQuestionableFilenamesConfig) -> Result<Self> {
        Ok(Self {
            allowlist_for_braces: config
                .allowlist_for_braces
                .as_deref()
                .map(Regex::new)
                .transpose()
                .context("Failed to create allowlist regex for braces")?,
            braces: Regex::new(r"[{}]")?,
            allowlist_for_cmd_line: config
                .allowlist_for_cmd_line
                .as_deref()
                .map(Regex::new)
                .transpose()
                .context("Failed to create allowlist regex for cmd_line")?,
            // Disallow spaces, apostrophes, and files that start with hyphens
            cmd_line: Regex::new(r"\s|'|(^|/)-")?,
        })
    }
}

#[async_trait]
impl FileHook for NoQuestionableFilenames {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::accepted());
        }
        if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
            // For push-redirected pushes we rely on the hook
            // running in the original repo
            return Ok(HookExecution::accepted());
        }
        if change.is_none() {
            return Ok(HookExecution::accepted());
        }

        let path = path.to_string();
        if self.braces.is_match(&path) {
            match self.allowlist_for_braces {
                Some(ref allow) if allow.is_match(&path) => {}
                _ => {
                    return Ok(HookExecution::rejected(HookRejectionInfo::new_long(
                        "Illegal filename",
                        format!(
                            "ABORT: Illegal filename: {path:?}. The file name cannot include brace(s).",
                        ),
                    )));
                }
            }
        }

        if self.cmd_line.is_match(&path) {
            match self.allowlist_for_cmd_line {
                Some(ref allow) if allow.is_match(&path) => {}
                _ => {
                    return Ok(HookExecution::rejected(HookRejectionInfo::new_long(
                        "Illegal filename",
                        format!(
                            "ABORT: Illegal filename: {path}. The file name cannot include spaces, apostrophes or start with hyphens."
                        ),
                    )));
                }
            }
        }

        Ok(HookExecution::accepted())
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_typed_and_json_configs_match() -> Result<()> {
        let parse = |config: &HookConfig| {
            parse_hook_options(
                config,
                "no_questionable_filenames",
                NoQuestionableFilenamesConfig::from_typed,
            )
        };
        let typed = HookConfig {
            strings: [
                ("allowlist_for_braces".to_owned(), "^braces/".to_owned()),
                ("allowlist_for_cmd_line".to_owned(), "^spaces/".to_owned()),
            ]
            .into(),
            ..Default::default()
        };
        let json = HookConfig {
            options: Some(
                r#"{"allowlist_for_braces": "^braces/", "allowlist_for_cmd_line": "^spaces/"}"#
                    .to_owned(),
            ),
            ..Default::default()
        };
        let expected = NoQuestionableFilenamesConfig {
            allowlist_for_braces: Some("^braces/".to_owned()),
            allowlist_for_cmd_line: Some("^spaces/".to_owned()),
        };
        assert_eq!(parse(&typed)?, expected);
        assert_eq!(parse(&json)?, expected);

        let empty_json = HookConfig {
            options: Some("{}".to_owned()),
            ..Default::default()
        };
        assert_eq!(parse(&HookConfig::default())?, parse(&empty_json)?);
        Ok(())
    }
}
