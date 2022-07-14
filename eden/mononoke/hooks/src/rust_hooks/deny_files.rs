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
use mononoke_types::MPath;

use super::LuaPattern;
use crate::CrossRepoPushSource;
use crate::FileContentManager;
use crate::FileHook;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

#[derive(Default)]
pub struct DenyFilesBuilder {
    /// Deny patterns for all sources of pushes
    all_push_sources_deny_patterns: Option<Vec<String>>,
    /// Deny patterns for pushes, originally intended to this repo
    /// (as opposed to push-redirected ones)
    native_push_only_deny_patterns: Option<Vec<String>>,
}

impl DenyFilesBuilder {
    pub fn set_from_config(mut self, config: &HookConfig) -> Self {
        if let Some(v) = config.string_lists.get("deny_patterns") {
            self = self.all_push_sources_deny_patterns(v)
        }
        if let Some(v) = config.string_lists.get("native_push_only_deny_patterns") {
            self = self.native_push_only_deny_patterns(v)
        }
        self
    }

    pub fn all_push_sources_deny_patterns(
        mut self,
        strs: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.all_push_sources_deny_patterns =
            Some(strs.into_iter().map(|s| String::from(s.as_ref())).collect());
        self
    }

    pub fn native_push_only_deny_patterns(
        mut self,
        strs: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.native_push_only_deny_patterns =
            Some(strs.into_iter().map(|s| String::from(s.as_ref())).collect());
        self
    }

    pub fn build(self) -> Result<DenyFiles> {
        Ok(DenyFiles {
            all_push_sources_deny_patterns: self
                .all_push_sources_deny_patterns
                .unwrap_or_default()
                .into_iter()
                .map(LuaPattern::try_from)
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to create LuaPattern for all_push_sources_deny_patterns")?,
            native_push_only_deny_patterns: self
                .native_push_only_deny_patterns
                .unwrap_or_default()
                .into_iter()
                .map(LuaPattern::try_from)
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to create LuaPattern for native_push_only_deny_patterns")?,
        })
    }
}

pub struct DenyFiles {
    /// Deny patterns for all sources of pushes
    all_push_sources_deny_patterns: Vec<LuaPattern>,
    /// Deny patterns for pushes, originally intended to this repo
    /// (as opposed to push-redirected ones)
    native_push_only_deny_patterns: Vec<LuaPattern>,
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
        _content_manager: &'fetcher dyn FileContentManager,
        change: Option<&'change BasicFileChange>,
        path: &'path MPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        if change.is_none() {
            // It is acceptable to delete any file
            return Ok(HookExecution::Accepted);
        }

        deny_unacceptable_patterns(
            &self.all_push_sources_deny_patterns,
            &self.native_push_only_deny_patterns,
            path,
            cross_repo_push_source,
        )
    }
}

/// Easily-testable business logic of the `DenyFiles` hook
fn deny_unacceptable_patterns<'a, 'b>(
    all_patterns: &'a [LuaPattern],
    native_patterns: &'a [LuaPattern],
    path: &'b MPath,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<HookExecution> {
    let patterns: Vec<&LuaPattern> = {
        let mut patterns: Vec<&LuaPattern> = all_patterns.iter().collect();

        if CrossRepoPushSource::NativeToThisRepo == cross_repo_push_source {
            patterns.extend(native_patterns.iter());
        }

        patterns
    };

    let path = path.to_string();
    for pattern in patterns {
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

#[cfg(test)]
mod test {
    use super::*;

    fn setup_patterns() -> (Vec<LuaPattern>, Vec<LuaPattern>) {
        let all: Vec<LuaPattern> = vec!["all".try_into().unwrap()];
        let native: Vec<LuaPattern> = vec!["native".try_into().unwrap()];
        (all, native)
    }

    fn mpath(s: &str) -> MPath {
        MPath::new(s).unwrap()
    }

    #[test]
    fn test_denied_in_any_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("all/1");
        let r =
            deny_unacceptable_patterns(&all, &native, &mp, CrossRepoPushSource::NativeToThisRepo)
                .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));

        let r = deny_unacceptable_patterns(&all, &native, &mp, CrossRepoPushSource::PushRedirected)
            .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));
    }

    #[test]
    fn test_denied_only_in_native_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("native/1");
        let r =
            deny_unacceptable_patterns(&all, &native, &mp, CrossRepoPushSource::NativeToThisRepo)
                .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));

        let r = deny_unacceptable_patterns(&all, &native, &mp, CrossRepoPushSource::PushRedirected)
            .unwrap();
        assert!(matches!(r, HookExecution::Accepted));
    }

    #[test]
    fn test_allowed_in_any_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("ababagalamaga");
        let r =
            deny_unacceptable_patterns(&all, &native, &mp, CrossRepoPushSource::NativeToThisRepo)
                .unwrap();
        assert!(matches!(r, HookExecution::Accepted));

        let r = deny_unacceptable_patterns(&all, &native, &mp, CrossRepoPushSource::PushRedirected)
            .unwrap();
        assert!(matches!(r, HookExecution::Accepted));
    }
}
