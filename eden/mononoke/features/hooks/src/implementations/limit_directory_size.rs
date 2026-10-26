/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use context::CoreContext;
use futures::future;
use itertools::Itertools;
use mononoke_types::BonsaiChangeset;
use mononoke_types::MPath;
use regex::Regex;
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::Pushvars;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `limit_directory_size` hook.
    pub(crate) struct LimitDirectorySizeMessages for "limit_directory_size" {
        /// A directory grew to more entries than its limit.
        too_large_directory(
            /// Path of the directory.
            path,
            /// Number of entries before the commit.
            old_size,
            /// Number of entries after the commit.
            new_size,
            /// Number of entries the commit added.
            enlargement,
            /// The limit on the number of entries, after path overrides.
            limit,
        ) = "Directory ${path} has ${new_size} entries but at most ${limit} are allowed.";
        /// A directory that was already well over its limit grew past the
        /// next multiple of `oversize_directory_growth_limit`.
        increase_oversize_directory(
            /// Path of the directory.
            path,
            /// Number of entries before the commit.
            old_size,
            /// Number of entries after the commit.
            new_size,
            /// Number of entries the commit added.
            enlargement,
            /// The normal limit on the number of entries, after path overrides.
            limit,
            /// The growth threshold the directory now exceeds.
            growth_limit,
        ) = "Directory ${path} already had ${old_size} entries, more than the limit of ${limit}, and now has ${new_size}, more than ${growth_limit}.";
    }
}

/// Limit the size of directories to prevent very large directories from being
/// created.  In this context, the "size" of a directory is the number of
/// child entries.
///
/// Due to limitations in the bonsai data model, we consider the sizes of
/// directories in the source commit *before* pushrebase.  That means it's
/// still possible to create a very large directory if you pushrebase a commit
/// that increases the size onto a commit that increases it to just before the
/// limit.  In this case, it will likely be the *next* commit that adds a file
/// to the directory that will trigger the hook, although that is not
/// guaranteed, so this hook should be considered "best effort".
///
/// The hook considers two values: the size of each modified directory before
/// (B) and after (A) the change.  The hook only applies if the directory size
/// grows (A > B), so directories can always stay the same size or be made
/// smaller.
///
/// In the simplest case, the limit (L) means that if A > L then the hook will
/// disallow the commit.
///
/// To allow existing very large directories to be given some grace period,
/// this restriction is relaxed if B > (L + T).
///
/// However, to prevent these directories from getting even larger, we will
/// still disallow if they exceed the next multiple of the growth limit, i.e.
/// if N * G < B <= (N + 1) * G and A > (N + 1) * G.
#[derive(Deserialize, Clone, Debug)]
pub struct LimitDirectorySizeConfig {
    /// The maximum size for a directory.  Changes will not be allowed
    /// if it results in a directory being larger than this size.
    #[serde(default)]
    directory_size_limit: Option<u64>,

    /// If a directory is already much larger than the maximum size, then the
    /// change may still be allowed if the existing size of the directory
    /// exceeds the limit by this value.
    #[serde(default)]
    oversize_directory_threshold: Option<u64>,

    /// Maximum growth for directories already over the limit.  Changes
    /// will not be allowed to cross the next threshold of N * limit).
    #[serde(default)]
    oversize_directory_growth_limit: Option<u64>,

    /// Ignore paths.  These paths will be ignored for the purposes of
    /// calculating the commit size or number of files.
    #[serde(default, with = "serde_regex")]
    ignore_path_regexes: Vec<Regex>,

    /// Path-based overrides.  When a path matches, the override value
    /// replaces the base limit.  Multiple overrides must not match the
    /// same path (an error is returned if they do).
    #[serde(default)]
    path_overrides: Vec<LimitDirectorySizeOverride>,

    /// Replaces the `too_large_directory` message. Use the hook config's
    /// `messages` instead.
    #[serde(default)]
    too_large_directory_message: Option<String>,

    /// Replaces the `increase_oversize_directory` message. Use the hook
    /// config's `messages` instead.
    #[serde(default)]
    increase_oversize_directory_message: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct LimitDirectorySizeOverride {
    /// This override applies when the path matches.
    #[serde(with = "serde_regex")]
    path_regex: Regex,

    /// Override the size limit to this value when the path matches.
    #[serde(default)]
    directory_size_limit: Option<u64>,
}

/// Hook to block commits that exceed a size limit, either in terms of bytes
/// or number of files.
#[derive(Clone, Debug)]
pub struct LimitDirectorySizeHook {
    config: LimitDirectorySizeConfig,
    messages: LimitDirectorySizeMessages,
}

impl LimitDirectorySizeHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Ok(Self::with_options(config.parse_options()?, config))
    }

    fn with_options(options: LimitDirectorySizeConfig, config: &HookConfig) -> Self {
        let messages = LimitDirectorySizeMessages::builder()
            .legacy(
                "too_large_directory",
                options.too_large_directory_message.as_deref(),
            )
            .legacy(
                "increase_oversize_directory",
                options.increase_oversize_directory_message.as_deref(),
            )
            .build(config);
        Self {
            config: options,
            messages,
        }
    }

    #[cfg(test)]
    fn with_config(options: LimitDirectorySizeConfig) -> Result<Self> {
        Ok(Self::with_options(options, &HookConfig::default()))
    }
}

#[async_trait]
impl ChangesetHook for LimitDirectorySizeHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        _maybe_pushvars: Option<&'cs Pushvars>,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::accepted());
        }
        if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
            // For push-redirected commits, we rely on running source-repo hooks
            return Ok(HookExecution::accepted());
        }

        let source_changeset_id = changeset.get_changeset_id();
        let parent_changeset_id = match changeset.parents().exactly_one() {
            Ok(changeset_id) => changeset_id,
            _ => {
                // Ignore roots and merges
                return Ok(HookExecution::accepted());
            }
        };

        let mut changed_dirs = HashSet::new();

        for (path, file_change) in changeset.file_changes() {
            if file_change.is_changed() {
                for parent_path in MPath::from(path.clone()).into_ancestors() {
                    if !changed_dirs.insert(parent_path) {
                        break;
                    }
                }
            }
        }

        let changed_dirs: Vec<_> = changed_dirs.into_iter().collect();

        let (source_directory_sizes, parent_directory_sizes) = future::try_join(
            hook_repo.directory_sizes(ctx, source_changeset_id, changed_dirs.clone()),
            hook_repo.directory_sizes(ctx, parent_changeset_id, changed_dirs.clone()),
        )
        .await?;

        for (path, source_size) in source_directory_sizes {
            let parent_size = parent_directory_sizes.get(&path).copied().unwrap_or(0);

            if source_size <= parent_size {
                // Only consider growth.
                continue;
            }

            let path = path.to_string();

            if self
                .config
                .ignore_path_regexes
                .iter()
                .any(|regex| regex.is_match(&path))
            {
                continue;
            }

            let matching_overrides: Vec<(&Regex, u64)> = self
                .config
                .path_overrides
                .iter()
                .filter_map(|path_override| {
                    path_override
                        .directory_size_limit
                        .filter(|_| path_override.path_regex.is_match(&path))
                        .map(|limit| (&path_override.path_regex, limit))
                })
                .collect();

            let limit = match matching_overrides.len() {
                0 => self.config.directory_size_limit,
                1 => Some(matching_overrides[0].1),
                _ => {
                    let details: Vec<String> = matching_overrides
                        .iter()
                        .map(|(regex, limit)| format!("'{regex}' (limit={limit})"))
                        .collect();
                    anyhow::bail!(
                        "limit_directory_size hook: directory '{}' matches multiple \
                         path_overrides: {}. Each directory should match at most one override.",
                        path,
                        details.join(", "),
                    );
                }
            };

            if let Some(limit) = limit {
                if source_size > limit {
                    // Directory is over the limit.  Was this already a large
                    // directory?
                    if let Some(threshold) = self.config.oversize_directory_threshold {
                        if parent_size > limit + threshold {
                            // This was already a large directory.  Did we hit
                            // its growth limit?
                            if let Some(growth_limit) = self.config.oversize_directory_growth_limit
                            {
                                // The threshold is the next multiple of the growth limit after the size.
                                let next_growth_limit =
                                    parent_size.div_ceil(growth_limit) * growth_limit;
                                if source_size > next_growth_limit {
                                    return Ok(HookExecution::rejected(
                                        self.messages
                                            .increase_oversize_directory(
                                                &path,
                                                parent_size,
                                                source_size,
                                                source_size - parent_size,
                                                limit,
                                                next_growth_limit,
                                            )
                                            .into_rejection("Directory too large"),
                                    ));
                                }
                            }
                            // Directory was already oversized, so ignore it.
                            continue;
                        }
                    }
                    return Ok(HookExecution::rejected(
                        self.messages
                            .too_large_directory(
                                &path,
                                parent_size,
                                source_size,
                                source_size - parent_size,
                                limit,
                            )
                            .into_rejection("Directory too large"),
                    ));
                }
            }
        }

        Ok(HookExecution::accepted())
    }
}

#[cfg(test)]
mod test {
    use anyhow::Error;
    use blobstore::Loadable;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use hook_manager::HookRepo;
    use hook_manager_testlib::HookTestRepo;
    use mononoke_macros::mononoke;

    use super::*;
    use crate::HookResult;

    /// Create default test config that each test can customize.
    fn make_test_config() -> LimitDirectorySizeConfig {
        LimitDirectorySizeConfig {
            directory_size_limit: None,
            oversize_directory_threshold: None,
            oversize_directory_growth_limit: None,
            ignore_path_regexes: Vec::new(),
            path_overrides: Vec::new(),
            too_large_directory_message: Some(String::from(
                "Directory too large: ${new_size} > ${limit}.",
            )),
            increase_oversize_directory_message: Some(String::from(
                "Large directory grown too much: ${new_size} > ${growth_limit} > ${limit}.",
            )),
        }
    }

    fn assert_rejected(hook_execution: HookExecution, desc: &str) {
        match hook_execution.result {
            HookResult::Rejected(info) => {
                assert_eq!(info.long_description, desc);
            }
            HookResult::Accepted => {
                panic!("should be rejected");
            }
        };
    }

    #[mononoke::fbinit_test]
    async fn test_limit_directory_size(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let (commits, _dag) = tests_utils::drawdag::extend_from_dag_with_actions(
            ctx,
            repo,
            r#"
                A-B-C-D-E-F-G
                # modify: A "dir1/file1" "A"
                # modify: B "dir1/file2" "B"
                # modify: B "dir1/file3" "B"
                # modify: C "dir1/file4" "C"
                # modify: C "dir1/file5" "C"
                # modify: D "dir1/file6" "D"
                # modify: E "dir1/file7" "E"
                # modify: E "dir1/file8" "E"
                # modify: F "dir1/file8" "F"
                # delete: G "dir1/file8"
                # default_files: false
            "#,
        )
        .await?;
        borrowed!(commits);

        let load_commit =
            move |name| async move { commits[name].load(ctx, &repo.repo_blobstore).await };

        let bookmark = BookmarkKey::new("bookmark")?;

        let hook_repo = HookRepo::build_from(&repo);

        // Nothing enabled
        let config = make_test_config();
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("B").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("C").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());

        // Directory size limit enabled
        let mut config = make_test_config();
        config.directory_size_limit = Some(3);
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("B").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("C").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_rejected(hook_execution, "Directory too large: 5 > 3.");
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("D").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_rejected(hook_execution, "Directory too large: 6 > 3.");
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("E").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_rejected(hook_execution, "Directory too large: 8 > 3.");

        // Directory size limit and oversized directories enabled.
        let mut config = make_test_config();
        config.directory_size_limit = Some(3);
        config.oversize_directory_threshold = Some(1);
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("C").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_rejected(hook_execution, "Directory too large: 5 > 3.");
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("D").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("E").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());

        // Directory size limit and oversize directories with growth limit
        // enabled.
        let mut config = make_test_config();
        config.directory_size_limit = Some(3);
        config.oversize_directory_threshold = Some(1);
        config.oversize_directory_growth_limit = Some(3);
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("C").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_rejected(hook_execution, "Directory too large: 5 > 3.");
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("D").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("E").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_rejected(hook_execution, "Large directory grown too much: 8 > 6 > 3.");
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("F").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("G").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());

        // Ignore paths.
        let mut config = make_test_config();
        config.directory_size_limit = Some(3);
        config.ignore_path_regexes = vec![Regex::new("^dir")?];
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("C").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());

        // Override raises limit.
        let mut config = make_test_config();
        config.directory_size_limit = Some(3);
        config.path_overrides = vec![LimitDirectorySizeOverride {
            path_regex: Regex::new("^dir1$")?,
            directory_size_limit: Some(10),
        }];
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("C").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("D").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());

        // Override lowers limit.
        // Base=10, override dir1 to 3 -> dir1 with 5 entries rejected.
        let mut config = make_test_config();
        config.directory_size_limit = Some(10);
        config.path_overrides = vec![LimitDirectorySizeOverride {
            path_regex: Regex::new("^dir1$")?,
            directory_size_limit: Some(3),
        }];
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("C").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_rejected(hook_execution, "Directory too large: 5 > 3.");

        // Override doesn't affect non-matching paths.
        // Base=10, override "^other" to 3 -> dir1 with 5 entries accepted.
        let mut config = make_test_config();
        config.directory_size_limit = Some(10);
        config.path_overrides = vec![LimitDirectorySizeOverride {
            path_regex: Regex::new("^other")?,
            directory_size_limit: Some(3),
        }];
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("C").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::accepted());

        // Multiple overlapping overrides error.
        let mut config = make_test_config();
        config.directory_size_limit = Some(10);
        config.path_overrides = vec![
            LimitDirectorySizeOverride {
                path_regex: Regex::new("^dir1$")?,
                directory_size_limit: Some(8),
            },
            LimitDirectorySizeOverride {
                path_regex: Regex::new("^dir1$")?,
                directory_size_limit: Some(4),
            },
        ];
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let result = hook
            .run(
                ctx,
                &hook_repo,
                &bookmark,
                &load_commit("C").await?,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
                None,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("limit_directory_size hook")
                && err.contains("dir1")
                && err.contains("matches multiple path_overrides"),
            "unexpected error: {err}",
        );

        Ok(())
    }
}
