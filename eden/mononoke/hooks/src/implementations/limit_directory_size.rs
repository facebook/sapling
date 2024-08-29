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
use crate::HookRejectionInfo;
use crate::HookStateProvider;
use crate::PushAuthoredBy;

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

    /// Path-based overrides.  The limits can be increased if paths match
    /// specific values.
    #[serde(default)]
    path_overrides: Vec<LimitDirectorySizeOverride>,

    /// Message to include in the hook rejection if the directory now exceeds
    /// the limit.
    ///
    /// The following variables used in the message will be expanded:
    ///    ${limit} => the limit used
    ///    ${old_size} => the old size of the directory
    ///    ${enlargement} => the number of added entries
    ///    ${new_size} => the new size of the directory
    ///    ${path} => the path of the directory
    too_large_directory_message: String,

    /// Message to include in the hook rejection if an oversize directory
    /// exceeds the next threshold
    ///
    /// The following variables used in the message will be expanded:
    ///    ${limit} => the normal limit of a directory
    ///    ${growth_limit} => the threshold this oversized directory now exceeds
    ///    ${old_size} => the old size of the directory
    ///    ${enlargement} => the number of added entries
    ///    ${new_size} => the new size of the directory
    ///    ${path} => the path of the directory
    increase_oversize_directory_message: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct LimitDirectorySizeOverride {
    /// This override will increase the size limit if any path matches.
    #[serde(with = "serde_regex")]
    path_regex: Regex,

    /// This override will increase the size limit to at least this value
    /// (other overrides may increase it further).
    #[serde(default)]
    directory_size_limit: Option<u64>,
}

/// Hook to block commits that exceed a size limit, either in terms of bytes
/// or number of files.
#[derive(Clone, Debug)]
pub struct LimitDirectorySizeHook {
    config: LimitDirectorySizeConfig,
}

impl LimitDirectorySizeHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: LimitDirectorySizeConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl ChangesetHook for LimitDirectorySizeHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        content_manager: &'fetcher dyn HookStateProvider,
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

        let source_changeset_id = changeset.get_changeset_id();
        let parent_changeset_id = match changeset.parents().exactly_one() {
            Ok(changeset_id) => changeset_id,
            _ => {
                // Ignore roots and merges
                return Ok(HookExecution::Accepted);
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
            content_manager.directory_sizes(ctx, source_changeset_id, changed_dirs.clone()),
            content_manager.directory_sizes(ctx, parent_changeset_id, changed_dirs.clone()),
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

            let limit = self
                .config
                .path_overrides
                .iter()
                .filter_map(|path_override| {
                    path_override
                        .directory_size_limit
                        .filter(|_| path_override.path_regex.is_match(&path))
                })
                .chain(self.config.directory_size_limit)
                .max();

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
                                    (parent_size + growth_limit - 1) / growth_limit * growth_limit;
                                if source_size > next_growth_limit {
                                    return Ok(HookExecution::Rejected(
                                        HookRejectionInfo::new_long(
                                            "Directory too large",
                                            self.config
                                                .increase_oversize_directory_message
                                                .replace("${old_size}", &parent_size.to_string())
                                                .replace("${limit}", &limit.to_string())
                                                .replace(
                                                    "${growth_limit}",
                                                    &next_growth_limit.to_string(),
                                                )
                                                .replace(
                                                    "${enlargement}",
                                                    &(source_size - parent_size).to_string(),
                                                )
                                                .replace("${new_size}", &source_size.to_string())
                                                .replace("${path}", &path),
                                        ),
                                    ));
                                }
                            }
                            // Directory was already oversized, so ignore it.
                            continue;
                        }
                    }
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "Directory too large",
                        self.config
                            .too_large_directory_message
                            .replace("${old_size}", &parent_size.to_string())
                            .replace("${limit}", &limit.to_string())
                            .replace("${enlargement}", &(source_size - parent_size).to_string())
                            .replace("${new_size}", &source_size.to_string())
                            .replace("${path}", &path),
                    )));
                }
            }
        }

        Ok(HookExecution::Accepted)
    }
}

#[cfg(test)]
mod test {
    use anyhow::Error;
    use blobstore::Loadable;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use repo_hook_file_content_provider::RepoHookStateProvider;
    use tests_utils::BasicTestRepo;

    use super::*;

    /// Create default test config that each test can customize.
    fn make_test_config() -> LimitDirectorySizeConfig {
        LimitDirectorySizeConfig {
            directory_size_limit: None,
            oversize_directory_threshold: None,
            oversize_directory_growth_limit: None,
            ignore_path_regexes: Vec::new(),
            path_overrides: Vec::new(),
            too_large_directory_message: String::from(
                "Directory too large: ${new_size} > ${limit}.",
            ),
            increase_oversize_directory_message: String::from(
                "Large directory grown too much: ${new_size} > ${growth_limit} > ${limit}.",
            ),
        }
    }

    fn assert_rejected(hook_execution: HookExecution, desc: &str) {
        match hook_execution {
            HookExecution::Rejected(info) => {
                assert_eq!(info.long_description, desc);
            }
            HookExecution::Accepted => {
                panic!("should be rejected");
            }
        };
    }

    #[mononoke::fbinit_test]
    async fn test_limit_directory_size(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
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

        let content_manager = RepoHookStateProvider::new(&repo);

        // Nothing enabled
        let config = make_test_config();
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("B").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("C").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);

        // Directory size limit enabled
        let mut config = make_test_config();
        config.directory_size_limit = Some(3);
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("B").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("C").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_rejected(hook_execution, "Directory too large: 5 > 3.");
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("D").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_rejected(hook_execution, "Directory too large: 6 > 3.");
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("E").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
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
                &bookmark,
                &load_commit("C").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_rejected(hook_execution, "Directory too large: 5 > 3.");
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("D").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("E").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);

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
                &bookmark,
                &load_commit("C").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_rejected(hook_execution, "Directory too large: 5 > 3.");
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("D").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("E").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_rejected(hook_execution, "Large directory grown too much: 8 > 6 > 3.");
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("F").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("G").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);

        // Ignore paths.
        let mut config = make_test_config();
        config.directory_size_limit = Some(3);
        config.ignore_path_regexes = vec![Regex::new("^dir")?];
        let hook = LimitDirectorySizeHook::with_config(config)?;
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("C").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);

        // Override limit.
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
                &bookmark,
                &load_commit("C").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);
        let hook_execution = hook
            .run(
                ctx,
                &bookmark,
                &load_commit("D").await?,
                &content_manager,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(hook_execution, HookExecution::Accepted);

        Ok(())
    }
}
