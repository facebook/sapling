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

use crate::lua_pattern::LuaPattern;
use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookFileContentProvider;
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
        _content_manager: &'fetcher dyn HookFileContentProvider,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }

        deny_unacceptable_patterns(
            &self.all_push_sources_deny_patterns,
            &self.native_push_only_deny_patterns,
            path,
            cross_repo_push_source,
            change,
        )
    }
}

fn rejection<'a, 'b>(path: &'a String, pattern: &'b LuaPattern) -> HookExecution {
    HookExecution::Rejected(HookRejectionInfo::new_long(
        "Denied filename matched name pattern",
        format!(
            "Denied filename '{}' matched name pattern '{}'. Rename or remove this file and try again.",
            path, pattern
        ),
    ))
}

/// Easily-testable business logic of the `DenyFiles` hook
fn deny_unacceptable_patterns<'a, 'b, 'c>(
    all_patterns: &'a [LuaPattern],
    native_patterns: &'a [LuaPattern],
    path: &'b NonRootMPath,
    cross_repo_push_source: CrossRepoPushSource,
    change: Option<&'c BasicFileChange>,
) -> Result<HookExecution> {
    let path = path.to_string();
    if CrossRepoPushSource::NativeToThisRepo == cross_repo_push_source {
        for pattern in native_patterns {
            if pattern.is_match(&path) {
                return Ok(rejection(&path, pattern));
            }
        }
    }
    if change.is_none() {
        // It is acceptable to delete any file
        return Ok(HookExecution::Accepted);
    }

    for pattern in all_patterns {
        if pattern.is_match(&path) {
            return Ok(rejection(&path, pattern));
        }
    }

    Ok(HookExecution::Accepted)
}

#[cfg(test)]
mod test {
    use mononoke_types::FileType;
    use mononoke_types_mocks::contentid::TWOS_CTID;

    use super::*;

    fn setup_patterns() -> (Vec<LuaPattern>, Vec<LuaPattern>) {
        let all: Vec<LuaPattern> = vec!["all".try_into().unwrap()];
        let native: Vec<LuaPattern> = vec!["native".try_into().unwrap()];
        (all, native)
    }

    fn basic_change() -> BasicFileChange {
        BasicFileChange::new(TWOS_CTID, FileType::Regular, 10)
    }

    fn mpath(s: &str) -> NonRootMPath {
        NonRootMPath::new(s).unwrap()
    }

    #[test]
    fn test_denied_in_any_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("all/1");
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));

        let r = deny_unacceptable_patterns(
            &all,
            &native,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));
    }

    #[test]
    fn test_denied_only_in_native_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("native/1");
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));

        let r = deny_unacceptable_patterns(
            &all,
            &native,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Accepted));
    }

    #[test]
    fn test_remove_denied_only_in_native_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("native/1");
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            None,
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));

        let r = deny_unacceptable_patterns(
            &all,
            &native,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Accepted));
    }

    #[test]
    fn test_allowed_in_any_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("ababagalamaga");
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Accepted));

        let r = deny_unacceptable_patterns(
            &all,
            &native,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Accepted));
    }
}
