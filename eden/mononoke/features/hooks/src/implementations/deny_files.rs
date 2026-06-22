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

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::lua_pattern::LuaPattern;

#[derive(Default)]
pub struct DenyFilesBuilder {
    /// Deny patterns for all sources of pushes
    all_push_sources_deny_patterns: Option<Vec<String>>,
    /// Deny patterns for pushes, originally intended to this repo
    /// (as opposed to push-redirected ones)
    native_push_only_deny_patterns: Option<Vec<String>>,
    /// When true, deletions of paths matching `all_push_sources_deny_patterns`
    /// are also rejected. Defaults to false (matches the historical behavior
    /// where deletions were always permitted by `all_push_sources_deny_patterns`).
    block_deletions: bool,
}

impl DenyFilesBuilder {
    pub fn set_from_config(mut self, config: &HookConfig) -> Self {
        if let Some(v) = config.string_lists.get("deny_patterns") {
            self = self.all_push_sources_deny_patterns(v)
        }
        if let Some(v) = config.string_lists.get("native_push_only_deny_patterns") {
            self = self.native_push_only_deny_patterns(v)
        }
        if let Some(v) = config.ints_64.get("block_deletions") {
            self = self.block_deletions(*v != 0)
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

    pub fn block_deletions(mut self, block_deletions: bool) -> Self {
        self.block_deletions = block_deletions;
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
            block_deletions: self.block_deletions,
        })
    }
}

pub struct DenyFiles {
    /// Deny patterns for all sources of pushes
    pub all_push_sources_deny_patterns: Vec<LuaPattern>,
    /// Deny patterns for pushes, originally intended to this repo
    /// (as opposed to push-redirected ones)
    pub native_push_only_deny_patterns: Vec<LuaPattern>,
    /// When true, deletions matching `all_push_sources_deny_patterns` are
    /// rejected. Native-only patterns already block deletions unconditionally.
    pub block_deletions: bool,
}

impl DenyFiles {
    pub fn builder() -> DenyFilesBuilder {
        DenyFilesBuilder::default()
    }
}

#[async_trait]
impl FileHook for DenyFiles {
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

        deny_unacceptable_patterns(
            &self.all_push_sources_deny_patterns,
            &self.native_push_only_deny_patterns,
            self.block_deletions,
            path,
            cross_repo_push_source,
            change,
        )
    }
}

fn rejection<'a, 'b>(path: &'a String, pattern: &'b LuaPattern) -> HookExecution {
    HookExecution::rejected(HookRejectionInfo::new_long(
        "Denied filename matched name pattern",
        format!(
            "Denied filename '{path}' matched deny pattern '{pattern}'. This path is protected and your change must not modify it. To fix this, revert your changes to '{path}' so that it no longer appears in your diff, then re-submit."
        ),
    ))
}

/// Easily-testable business logic of the `DenyFiles` hook
fn deny_unacceptable_patterns<'a, 'b, 'c>(
    all_patterns: &'a [LuaPattern],
    native_patterns: &'a [LuaPattern],
    block_deletions: bool,
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
    if change.is_none() && !block_deletions {
        // Deletions are accepted by default; opt in via `block_deletions`.
        return Ok(HookExecution::accepted());
    }

    for pattern in all_patterns {
        if pattern.is_match(&path) {
            return Ok(rejection(&path, pattern));
        }
    }

    Ok(HookExecution::accepted())
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;
    use mononoke_types::FileType;
    use mononoke_types::GitLfs;
    use mononoke_types_mocks::contentid::TWOS_CTID;

    use super::*;

    fn setup_patterns() -> (Vec<LuaPattern>, Vec<LuaPattern>) {
        let all: Vec<LuaPattern> = vec!["all".try_into().unwrap()];
        let native: Vec<LuaPattern> = vec!["native".try_into().unwrap()];
        (all, native)
    }

    fn basic_change() -> BasicFileChange {
        BasicFileChange::new(TWOS_CTID, FileType::Regular, 10, GitLfs::FullContent)
    }

    fn mpath(s: &str) -> NonRootMPath {
        NonRootMPath::new(s).unwrap()
    }

    #[mononoke::test]
    fn test_denied_in_any_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("all/1");
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            false,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_rejected());

        let r = deny_unacceptable_patterns(
            &all,
            &native,
            false,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_rejected());
    }

    #[mononoke::test]
    fn test_denied_only_in_native_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("native/1");
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            false,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_rejected());

        let r = deny_unacceptable_patterns(
            &all,
            &native,
            false,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_accepted());
    }

    #[mononoke::test]
    fn test_remove_denied_only_in_native_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("native/1");
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            false,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            None,
        )
        .unwrap();
        assert!(r.is_rejected());

        let r = deny_unacceptable_patterns(
            &all,
            &native,
            false,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_accepted());
    }

    #[mononoke::test]
    fn test_allowed_in_any_push() {
        let (all, native) = setup_patterns();
        let mp = mpath("ababagalamaga");
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            false,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_accepted());

        let r = deny_unacceptable_patterns(
            &all,
            &native,
            false,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_accepted());
    }

    #[mononoke::test]
    fn test_deletion_allowed_when_block_deletions_off() {
        let (all, native) = setup_patterns();
        let mp = mpath("all/1");
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            false,
            &mp,
            CrossRepoPushSource::PushRedirected,
            None,
        )
        .unwrap();
        assert!(r.is_accepted());
    }

    #[mononoke::test]
    fn test_deletion_blocked_when_block_deletions_on() {
        let (all, native) = setup_patterns();
        let mp = mpath("all/1");
        // Native push: rejected (matches all_patterns regardless of source).
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            true,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            None,
        )
        .unwrap();
        assert!(r.is_rejected());

        // Push-redirected: also rejected — this is the new behavior.
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            true,
            &mp,
            CrossRepoPushSource::PushRedirected,
            None,
        )
        .unwrap();
        assert!(r.is_rejected());
    }

    #[mononoke::test]
    fn test_unmatched_deletion_allowed_when_block_deletions_on() {
        let (all, native) = setup_patterns();
        let mp = mpath("ababagalamaga");
        let r = deny_unacceptable_patterns(
            &all,
            &native,
            true,
            &mp,
            CrossRepoPushSource::PushRedirected,
            None,
        )
        .unwrap();
        assert!(r.is_accepted());
    }
}
