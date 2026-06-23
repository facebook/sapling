/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use git_source_of_truth::GitSourceOfTruth;
use git_source_of_truth::GitSourceOfTruthConfig;
use git_source_of_truth::RepositoryName;
use git_source_of_truth::Staleness;
use scs_errors::ServiceError;
use source_control as thrift;
use stats::prelude::*;

use crate::source_control_impl::SourceControlServiceImpl;
use crate::specifiers::SpecifierExt;

define_stats! {
    prefix = "mononoke.scs.git_repo_state";
    not_started: timeseries(Rate, Sum),
    in_progress: timeseries(Rate, Sum),
    created: timeseries(Rate, Sum),
    unknown: timeseries(Rate, Sum),
    invalid: timeseries(Rate, Sum),
}

impl SourceControlServiceImpl {
    pub(crate) async fn git_repo_state(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        _params: thrift::GitRepoStateParams,
    ) -> Result<thrift::GitRepoStateResponse, ServiceError> {
        // Validate that `repo.name` is a configured repo before reading SoT.
        // Two reasons:
        //   1. Without this, a typo'd name silently returns UNKNOWN, identical
        //      to "row genuinely missing" — defeating the verification purpose.
        //   2. Without this, the endpoint becomes a recon signal: an attacker
        //      could probe SoT state for arbitrary names. Validating against
        //      configerator restricts callers to repos that exist as configs.
        //
        // Note: we route through `self.configs.get_or_load_repo_config` (same
        // pattern as `repo_info` at methods/repo.rs:71-74) rather than
        // `self.repo(...)`, because the latter requires the repo to be loaded
        // into MononokeRepos — which is exactly what NOT_STARTED / IN_PROGRESS
        // repos are NOT. See design final §B.1.
        let _repo_config = self
            .configs
            .get_or_load_repo_config(repo.name.as_str())
            .map_err(|_| scs_errors::repo_not_found(repo.description()))?;

        git_repo_state_impl(&ctx, &*self.git_source_of_truth_config, repo.name).await
    }
}

/// Free function for testability — see tests below.
/// Lets tests inject a TestGitSourceOfTruthConfig without constructing a full SourceControlServiceImpl.
async fn git_repo_state_impl(
    ctx: &CoreContext,
    config: &dyn GitSourceOfTruthConfig,
    repo_name: String,
) -> Result<thrift::GitRepoStateResponse, ServiceError> {
    // Staleness::MostRecent is required (not optional): MaybeStale would
    // re-introduce the caching staleness this design exists to avoid.
    let entry = config
        .get_by_repo_name(ctx, &RepositoryName(repo_name), Staleness::MostRecent)
        .await
        .inspect_err(|err| {
            tracing::warn!("git_repo_state: get_by_repo_name failed: {err:?}");
        })
        .map_err(scs_errors::internal_error)?;

    let (state, mutation_id) = match entry {
        None => (thrift::GitRepoState::UNKNOWN, None),
        Some(entry) => match entry.source_of_truth {
            GitSourceOfTruth::Reserved => match entry.mutation_id {
                None => (thrift::GitRepoState::NOT_STARTED, None),
                Some(mutation_id_val) => (
                    thrift::GitRepoState::IN_PROGRESS,
                    Some(mutation_id_val.to_string()),
                ),
            },
            GitSourceOfTruth::Mononoke => (thrift::GitRepoState::CREATED, None),
            // Metagit and Locked are valid SoT states elsewhere in Mononoke but
            // indicate that this repo is NOT under the auto-create flow's
            // ownership. From the caller's perspective they're an unrecoverable
            // verification failure -- collapse both into INVALID.
            GitSourceOfTruth::Metagit | GitSourceOfTruth::Locked => {
                (thrift::GitRepoState::INVALID, None)
            }
        },
    };

    match state {
        thrift::GitRepoState::NOT_STARTED => STATS::not_started.add_value(1),
        thrift::GitRepoState::IN_PROGRESS => STATS::in_progress.add_value(1),
        thrift::GitRepoState::CREATED => STATS::created.add_value(1),
        thrift::GitRepoState::UNKNOWN => STATS::unknown.add_value(1),
        thrift::GitRepoState::INVALID => STATS::invalid.add_value(1),
        _ => {
            // Unrecognized thrift variant (Thrift codegen produces a non-exhaustive
            // newtype, so the named arms above don't prove exhaustiveness).
            // Fold into INVALID since both mean "we don't know how to interpret
            // this SoT state". Log loud -- this is a service-internal bug
            // indicating Thrift codegen added a variant the Rust handler doesn't
            // know about.
            STATS::invalid.add_value(1);
            tracing::error!(
                "git_repo_state: unrecognized GitRepoState variant from codegen (state={:?})",
                state
            );
        }
    }

    Ok(thrift::GitRepoStateResponse {
        state,
        mutation_id,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use git_source_of_truth::GitSourceOfTruth;
    use git_source_of_truth::GitSourceOfTruthConfig;
    use git_source_of_truth::RepositoryName;
    use git_source_of_truth::TestGitSourceOfTruthConfig;
    use mononoke_macros::mononoke;
    use mononoke_types::RepositoryId;
    use source_control as thrift;

    use super::*;

    /// Bridge ServiceError (Debug only) to anyhow for test ergonomics.
    async fn call(
        ctx: &CoreContext,
        config: &TestGitSourceOfTruthConfig,
        name: &str,
    ) -> Result<thrift::GitRepoStateResponse> {
        git_repo_state_impl(ctx, config, name.to_string())
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))
    }

    // V5 regression test: handler must NOT call self.repo(...) -- it must
    // resolve from repo.name directly. Otherwise NOT_STARTED for a not-yet-
    // loaded repo returns repo_not_found instead of NOT_STARTED.
    #[mononoke::fbinit_test]
    async fn unloaded_repo_returns_typed_state_not_repo_not_found(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = TestGitSourceOfTruthConfig::new();
        // Seed a Reserved row with no mutation_id => NOT_STARTED.
        config
            .insert_or_update_repo(
                &ctx,
                RepositoryId::new(1),
                RepositoryName("aosp/platform/foo".to_string()),
                GitSourceOfTruth::Reserved,
            )
            .await?;
        let resp = call(&ctx, &config, "aosp/platform/foo").await?;
        assert_eq!(resp.state, thrift::GitRepoState::NOT_STARTED);
        Ok(())
    }
}
