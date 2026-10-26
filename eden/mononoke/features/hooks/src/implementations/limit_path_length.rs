/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use context::CoreContext;
use mercurial_types::simple_fsencode;
use mononoke_types::BasicFileChange;
use mononoke_types::NonRootMPath;
use serde::Deserialize;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `limit_path_length` hook.
    pub(crate) struct LimitPathLengthMessages for "limit_path_length" {
        /// A changed file's path is at least `length_limit` bytes long.
        path_too_long(
            /// Path of the file.
            path,
            /// Length of the path, in bytes.
            length,
            /// The limit, in bytes.
            limit,
        ) = "Path length for '${path}' (${length}) exceeds length limit (>= ${limit})";
        /// A component of the file's path, as Mercurial would store it, is
        /// too long for Mercurial.
        component_too_long(
            /// The encoded path component, as a list of bytes.
            component,
            /// Length of the component, in bytes.
            length,
            /// The limit, in bytes.
            limit,
        ) = "Path component length for ${component} (${length}) exceeds length limit (>= ${limit})";
    }
}

// The filesystem max is 255.
const MAX_PATH_COMPONENT_LIMIT: usize = 255;

#[derive(Clone, Debug, Deserialize)]
pub struct LimitPathLengthConfig {
    length_limit: usize,
}

/// Hook to block commits changing file paths above a certain length.
/// This is needed because Mercurial has these limits and we shouldn't allow
/// commits that can't be synced to mercurial.
#[derive(Clone, Debug)]
pub struct LimitPathLengthHook {
    config: LimitPathLengthConfig,
    messages: LimitPathLengthMessages,
}

impl LimitPathLengthHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Ok(Self {
            messages: LimitPathLengthMessages::new(config),
            ..Self::with_config(config.parse_options()?)?
        })
    }

    pub fn with_config(config: LimitPathLengthConfig) -> Result<Self> {
        Ok(Self {
            config,
            messages: LimitPathLengthMessages::default(),
        })
    }
}

#[async_trait]
impl FileHook for LimitPathLengthHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        if push_authored_by.service() {
            return Ok(HookExecution::accepted());
        }
        if change.is_none() {
            // You can always delete paths
            return Ok(HookExecution::accepted());
        }

        // Encode file in mercurial encoding to make sure mercurial can accept those files
        // as well
        if let Some(rejection) = check_path(path, &self.messages)? {
            return Ok(rejection);
        }

        let len = path.len();

        let execution = if len >= self.config.length_limit {
            HookExecution::rejected(
                self.messages
                    .path_too_long(path, len, self.config.length_limit)
                    .into_rejection("Path too long"),
            )
        } else {
            HookExecution::accepted()
        };

        Ok(execution)
    }
}

fn check_path(
    path: &NonRootMPath,
    messages: &LimitPathLengthMessages,
) -> Result<Option<HookExecution>, Error> {
    let mut elements = path
        .as_ref()
        .iter()
        .map(|e| e.as_ref())
        .collect::<Vec<&[u8]>>();

    let basename = elements
        .pop()
        .ok_or_else(|| anyhow!("invalid path - no basename!"))?;

    let mut basename = Vec::from(basename);
    basename.extend(b".i");
    elements.push(&basename);

    let encoded_index_path = simple_fsencode(&elements);

    for component in encoded_index_path.iter() {
        if component.len() > MAX_PATH_COMPONENT_LIMIT {
            return Ok(Some(HookExecution::rejected(
                messages
                    .component_too_long(
                        format!("{component:?}"),
                        component.len(),
                        MAX_PATH_COMPONENT_LIMIT,
                    )
                    .into_rejection("Path component too long"),
            )));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_path_bad() {
        let path = NonRootMPath::new("flib/intern/__generated__/GraphQLMeerkatStep/flib/intern/entschema/generated/entity/profile_plus/EntPlatformToolViewerContextCallsiteMigrationRuleAction.php/GQLG_Intern__PlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionResponsePayload__EntPlatformToolViewerContextCallsiteMigrationRuleAction__genPerformGraphQLPlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionMutationType.php").unwrap();
        assert!(
            check_path(&path, &LimitPathLengthMessages::default())
                .unwrap()
                .is_some()
        );
    }

    #[mononoke::test]
    fn test_path_ok() {
        let path = NonRootMPath::new("flib/intern/__generated__/GraphQLFetchersMeerkatStep/ic/GQLG_File__EntIcxPositionSearchHitWorkdayPositionViewStateJunction__GraphQLFacebookInternalTypeSetFetcherWrapper.php").unwrap();
        assert!(
            check_path(&path, &LimitPathLengthMessages::default())
                .unwrap()
                .is_none()
        );
    }
}
