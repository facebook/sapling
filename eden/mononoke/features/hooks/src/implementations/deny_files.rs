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
use fancy_regex::Regex;
use metaconfig_types::HookConfig;
use mononoke_types::BasicFileChange;
use mononoke_types::NonRootMPath;
use serde::Deserialize;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::hook_options::parse_hook_options;
use crate::lua_pattern::lua_pattern_to_regex;

/// Configuration for the `deny_files` hook. Each regex is matched against the path of every
/// changed file.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DenyFilesConfig {
    /// Paths that no push may add or modify.
    pub deny_path_regexes: Vec<String>,
    /// Paths that pushes made directly to this repo may not add, modify or delete. Pushes
    /// redirected from another repo are not checked against these.
    pub native_push_only_deny_path_regexes: Vec<String>,
    /// Also reject deletions of paths matching `deny_path_regexes`.
    pub block_deletions: bool,
    /// Text appended to the rejection message.
    pub additional_message: Option<String>,
}

impl DenyFilesConfig {
    /// Builds the config from the `deny_patterns` and `native_push_only_deny_patterns` string
    /// lists of Lua patterns (regexes when prefixed with `re:`), the `block_deletions` 64-bit int
    /// (non-zero means true) and the `additional_message` string.
    fn from_typed(config: &HookConfig) -> Result<Self> {
        let regexes = |key: &str| {
            config
                .string_lists
                .get(key)
                .map(|patterns| {
                    patterns
                        .iter()
                        .map(|pattern| lua_pattern_to_regex(pattern))
                        .collect()
                })
                .unwrap_or_default()
        };
        Ok(Self {
            deny_path_regexes: regexes("deny_patterns"),
            native_push_only_deny_path_regexes: regexes("native_push_only_deny_patterns"),
            block_deletions: config
                .ints_64
                .get("block_deletions")
                .is_some_and(|v| *v != 0),
            additional_message: config.strings.get("additional_message").cloned(),
        })
    }
}

pub struct DenyFiles {
    /// Deny patterns for all sources of pushes
    pub all_push_sources_deny_patterns: Vec<Regex>,
    /// Deny patterns for pushes, originally intended to this repo
    /// (as opposed to push-redirected ones)
    pub native_push_only_deny_patterns: Vec<Regex>,
    /// When true, deletions matching `all_push_sources_deny_patterns` are
    /// rejected. Native-only patterns already block deletions unconditionally.
    pub block_deletions: bool,
    additional_message: Option<String>,
}

fn compile_regexes(regexes: &[String], name: &str) -> Result<Vec<Regex>> {
    regexes
        .iter()
        .map(|re| Regex::new(re).with_context(|| format!("Invalid {name} regex '{re}'")))
        .collect()
}

impl DenyFiles {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(parse_hook_options(
            config,
            "deny_files",
            DenyFilesConfig::from_typed,
        )?)
    }

    pub fn with_config(config: DenyFilesConfig) -> Result<Self> {
        Ok(Self {
            all_push_sources_deny_patterns: compile_regexes(
                &config.deny_path_regexes,
                "deny_path_regexes",
            )?,
            native_push_only_deny_patterns: compile_regexes(
                &config.native_push_only_deny_path_regexes,
                "native_push_only_deny_path_regexes",
            )?,
            block_deletions: config.block_deletions,
            additional_message: config.additional_message,
        })
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
            self.additional_message.as_deref(),
            path,
            cross_repo_push_source,
            change,
        )
    }
}

fn rejection(path: &str, pattern: &Regex, additional_message: Option<&str>) -> HookExecution {
    let pattern = pattern.as_str();
    let default_message = format!(
        "Denied filename '{path}' matched deny pattern '{pattern}'. This path is protected and your change must not modify it. To fix this, revert your changes to '{path}' so that it no longer appears in your diff, then re-submit."
    );
    let long_description = match additional_message {
        Some(additional) => format!("{default_message}\n\n{additional}"),
        None => default_message,
    };
    HookExecution::rejected(HookRejectionInfo::new_long(
        "Denied filename matched name pattern",
        long_description,
    ))
}

/// Easily-testable business logic of the `DenyFiles` hook
fn deny_unacceptable_patterns<'a, 'b, 'c>(
    all_patterns: &'a [Regex],
    native_patterns: &'a [Regex],
    block_deletions: bool,
    additional_message: Option<&str>,
    path: &'b NonRootMPath,
    cross_repo_push_source: CrossRepoPushSource,
    change: Option<&'c BasicFileChange>,
) -> Result<HookExecution> {
    let path = path.to_string();
    if CrossRepoPushSource::NativeToThisRepo == cross_repo_push_source {
        for pattern in native_patterns {
            if pattern.is_match(&path).unwrap_or(false) {
                return Ok(rejection(&path, pattern, additional_message));
            }
        }
    }
    if change.is_none() && !block_deletions {
        // Deletions are accepted by default; opt in via `block_deletions`.
        return Ok(HookExecution::accepted());
    }

    for pattern in all_patterns {
        if pattern.is_match(&path).unwrap_or(false) {
            return Ok(rejection(&path, pattern, additional_message));
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

    fn setup_patterns() -> (Vec<Regex>, Vec<Regex>) {
        let all = vec![Regex::new("all").unwrap()];
        let native = vec![Regex::new("native").unwrap()];
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
            &mp,
            CrossRepoPushSource::PushRedirected,
            None,
        )
        .unwrap();
        assert!(r.is_accepted());
    }

    #[mononoke::test]
    fn test_additional_rejection_message() {
        let additional_message = "Contact the owners of this directory before changing it.";
        let config = HookConfig {
            strings: [(
                "additional_message".to_owned(),
                additional_message.to_owned(),
            )]
            .into_iter()
            .collect(),
            string_lists: [("deny_patterns".to_owned(), vec!["all".to_owned()])]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let hook = DenyFiles::new(&config).expect("test patterns should be valid");

        let result = deny_unacceptable_patterns(
            &hook.all_push_sources_deny_patterns,
            &hook.native_push_only_deny_patterns,
            hook.block_deletions,
            hook.additional_message.as_deref(),
            &mpath("all/1"),
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .expect("hook execution should succeed");

        assert!(result.is_rejected_with_reason(|info| {
            info.long_description
                .starts_with("Denied filename 'all/1' matched deny pattern 'all'. ")
                && info
                    .long_description
                    .ends_with(&format!("then re-submit.\n\n{additional_message}"))
        }));
    }

    #[mononoke::test]
    fn test_typed_and_json_configs_match() -> Result<()> {
        let parse = |config: &HookConfig| {
            parse_hook_options(config, "deny_files", DenyFilesConfig::from_typed)
        };
        let typed = HookConfig {
            string_lists: [
                (
                    "deny_patterns".to_owned(),
                    vec!["^all/".to_owned(), "^buck%-out/".to_owned()],
                ),
                (
                    "native_push_only_deny_patterns".to_owned(),
                    vec!["re:^native/".to_owned()],
                ),
            ]
            .into(),
            ints_64: [("block_deletions".to_owned(), 1)].into(),
            strings: [("additional_message".to_owned(), "Ask first.".to_owned())].into(),
            ..Default::default()
        };
        let json = HookConfig {
            options: Some(
                r#"{
                    "deny_path_regexes": ["^all/", "^buck\\-out/"],
                    "native_push_only_deny_path_regexes": ["^native/"],
                    "block_deletions": true,
                    "additional_message": "Ask first."
                }"#
                .to_owned(),
            ),
            ..Default::default()
        };
        let expected = DenyFilesConfig {
            deny_path_regexes: vec!["^all/".to_owned(), "^buck\\-out/".to_owned()],
            native_push_only_deny_path_regexes: vec!["^native/".to_owned()],
            block_deletions: true,
            additional_message: Some("Ask first.".to_owned()),
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
