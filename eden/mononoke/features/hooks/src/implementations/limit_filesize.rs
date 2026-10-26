/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::BasicFileChange;
use mononoke_types::NonRootMPath;
use regex::Regex;
use serde::Deserialize;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;
use crate::hook_options::parse_hook_options;

hook_messages! {
    /// Messages of the `limit_filesize` hook.
    pub(crate) struct LimitFilesizeMessages for "limit_filesize" {
        /// A changed file is larger than the limit for its path.
        too_large(
            /// Path of the file.
            path,
            /// Size of the file, in bytes.
            size,
            /// The limit, in bytes.
            limit,
            /// Size of the file divided by the limit, to two decimal places.
            ratio,
            /// The `path_regex` of the limit.
            regex,
        ) = "File size limit is ${limit} bytes. You tried to push file ${path} that is over the limit \
            (${size} bytes, ${ratio}x the limit). This limit is enforced for files matching the \
            following regex: \"${regex}\".\n\
            \n\
            WHY THIS IS BLOCKED: Large files have ongoing infrastructure costs — they impact \
            caching systems, Mononoke, biggrep indexing, and permanent backups used by \
            30,000+ engineers.\n\
            \n\
            ALTERNATIVES TO CONSIDER:\n\
            - Manifold: Store large binaries in blob storage\n\
            - Dotslash: Distribute large tools without checking them in\n\
            - Buckify: Package binaries as Buck-managed dependencies\n\
            - LFS: Use Git LFS for large files that must be versioned\n\
            - Split files: Break large files into smaller pieces\n\
            \n\
            IF ALTERNATIVES DO NOT WORK:\n\
            1. Add @allow-large-files to your commit message \
            (using `sl amend -e`).\n\
            2. Request bypass approval at https://fburl.com/support/sourcecontrol.\n\
            \n\
            See https://fburl.com/landing_big_diffs for more details.";
    }
}

/// Configuration for the `limit_filesize` hook.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct LimitFilesizeConfig {
    /// Size limits in order of precedence: a file is subject to the first limit whose
    /// `path_regex` matches its path, and to no limit if none matches. Must not be empty.
    pub limits: Vec<FilesizeLimit>,
}

/// The size limit for files whose paths match a regex.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct FilesizeLimit {
    /// Regex matched against the path of the file.
    pub path_regex: String,
    /// Largest accepted file size in bytes, or `null` for no limit. The key is required, so that
    /// a misspelt key is an error rather than a missing limit.
    #[serde(deserialize_with = "Option::deserialize")]
    pub max_bytes: Option<u64>,
}

impl LimitFilesizeConfig {
    /// Builds the config from the `filesize_limits_regexes` string list and the
    /// `filesize_limits_values` int list, pairing them by position. A negative value means no
    /// limit.
    fn from_typed(config: &HookConfig) -> Result<Self> {
        let (Some(regexes), Some(values)) = (
            config.string_lists.get("filesize_limits_regexes"),
            config.int_lists.get("filesize_limits_values"),
        ) else {
            bail!(
                "Failed to initialize limit_filesize hook. Either 'filesize_limits_regexes' or 'filesize_limits_values' option is missing."
            );
        };
        if regexes.is_empty() || values.is_empty() {
            bail!(
                "Failed to initialize limit_filesize hook. Either 'filesize_limits_regexes' or 'filesize_limits_values' list is empty."
            );
        }
        let limits = regexes
            .iter()
            .zip(values)
            .map(|(path_regex, value)| FilesizeLimit {
                path_regex: path_regex.clone(),
                max_bytes: u64::try_from(*value).ok(),
            })
            .collect();
        Ok(Self { limits })
    }
}

pub struct LimitFilesize {
    path_regexes_with_limits: Vec<(Regex, Option<u64>)>,
    messages: LimitFilesizeMessages,
}

impl LimitFilesize {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Ok(Self {
            messages: LimitFilesizeMessages::new(config),
            ..Self::with_config(parse_hook_options(
                config,
                "limit_filesize",
                LimitFilesizeConfig::from_typed,
            )?)?
        })
    }

    pub fn with_config(config: LimitFilesizeConfig) -> Result<Self> {
        if config.limits.is_empty() {
            bail!("Failed to initialize limit_filesize hook. The list of limits is empty.");
        }
        let path_regexes_with_limits = config
            .limits
            .into_iter()
            .map(|limit| {
                let regex = Regex::new(&limit.path_regex)
                    .with_context(|| format!("Invalid path_regex '{}'", limit.path_regex))?;
                Ok((regex, limit.max_bytes))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            path_regexes_with_limits,
            messages: LimitFilesizeMessages::default(),
        })
    }
}

#[async_trait]
impl FileHook for LimitFilesize {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::accepted());
        }
        if cross_repo_push_source == CrossRepoPushSource::PushRedirected {
            // For push-redirected commits, we rely on running source-repo hooks
            return Ok(HookExecution::accepted());
        }

        let path = path.to_string();
        let change = match change {
            Some(c) => c,
            None => return Ok(HookExecution::accepted()),
        };

        if change.git_lfs().is_lfs_pointer() {
            // LFS pointers are not stored in the repo, so for now they don't count towards the limit
            // We might want to revise this policy in the future.
            return Ok(HookExecution::accepted());
        }

        let len = hook_repo
            .get_file_metadata(ctx, change.content_id())
            .await?
            .total_size;
        for (regex, maybe_limit) in &self.path_regexes_with_limits {
            if !regex.is_match(&path) {
                continue;
            }
            match maybe_limit {
                None => return Ok(HookExecution::accepted()),
                Some(limit) if len <= *limit => return Ok(HookExecution::accepted()),
                Some(limit) => {
                    let ratio = len as f64 / *limit as f64;
                    return Ok(HookExecution::rejected(
                        self.messages
                            .too_large(&path, len, limit, format!("{ratio:.2}"), regex)
                            .into_rejection("File too large"),
                    ));
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

    fn parse(config: &HookConfig) -> Result<LimitFilesizeConfig> {
        parse_hook_options(config, "limit_filesize", LimitFilesizeConfig::from_typed)
    }

    fn json_config(json: &str) -> HookConfig {
        HookConfig {
            options: Some(json.to_owned()),
            ..Default::default()
        }
    }

    #[mononoke::test]
    fn test_typed_and_json_configs_match() -> Result<()> {
        let typed = HookConfig {
            string_lists: [(
                "filesize_limits_regexes".to_owned(),
                vec!["^exempt/".to_owned(), ".*".to_owned()],
            )]
            .into(),
            int_lists: [("filesize_limits_values".to_owned(), vec![-1, 100])].into(),
            ..Default::default()
        };
        let json = json_config(
            r#"{"limits": [
                {"path_regex": "^exempt/", "max_bytes": null},
                {"path_regex": ".*", "max_bytes": 100}
            ]}"#,
        );
        let expected = LimitFilesizeConfig {
            limits: vec![
                FilesizeLimit {
                    path_regex: "^exempt/".to_owned(),
                    max_bytes: None,
                },
                FilesizeLimit {
                    path_regex: ".*".to_owned(),
                    max_bytes: Some(100),
                },
            ],
        };
        assert_eq!(parse(&typed)?, expected);
        assert_eq!(parse(&json)?, expected);
        Ok(())
    }

    #[mononoke::test]
    fn test_missing_max_bytes_is_an_error() {
        assert!(parse(&json_config(r#"{"limits": [{"path_regex": ".*"}]}"#)).is_err());
    }

    #[mononoke::test]
    fn test_no_limits_is_an_error() -> Result<()> {
        assert!(parse(&HookConfig::default()).is_err());
        assert!(parse(&json_config("{}")).is_err());
        assert!(LimitFilesize::with_config(parse(&json_config(r#"{"limits": []}"#))?).is_err());
        Ok(())
    }
}
