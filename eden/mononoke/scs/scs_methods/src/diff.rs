/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use diff_service_client::DiffServiceClient;
use environment::RemoteDiffOptions;
use futures::StreamExt;
use mononoke_api::FileContext;
use mononoke_api::HeaderlessUnifiedDiff;
use mononoke_api::Repo;
use mononoke_api::headerless_unified_diff;
use scs_errors::ServiceError;

/// Router for diff operations that can use either local mononoke_api
/// or remote diff_service based on command line args and JustKnobs configuration.
pub struct DiffRouter<'a> {
    pub(crate) diff_service_client: &'a Option<DiffServiceClient>,
    pub(crate) diff_options: &'a RemoteDiffOptions,
}

impl<'a> DiffRouter<'a> {
    /// Check if remote diff should be used for this repo
    fn should_use_remote_diff(&self, repo_name: &str) -> bool {
        // If remote diffs are enabled we check the JK to make sure the feature is active
        let jk_enabled =
            justknobs::eval("scm/mononoke:remote_diff", None, Some(repo_name)).unwrap_or(false);
        self.diff_options.diff_remotely && jk_enabled
    }

    /// Generate headerless unified diff between two files.
    /// Routes to either local mononoke_api or remote diff_service based on command line args and JustKnobs.
    pub async fn headerless_unified_diff(
        &self,
        ctx: &CoreContext,
        repo_name: &str,
        other_file: &FileContext<Repo>,
        base_file: &FileContext<Repo>,
        context_lines: usize,
    ) -> Result<HeaderlessUnifiedDiff, ServiceError> {
        if self.should_use_remote_diff(repo_name) {
            // Use remote diff service
            self.call_remote_diff_service(ctx, repo_name, other_file, base_file, context_lines)
                .await
        } else {
            // Use local diff (existing mononoke_api implementation)
            headerless_unified_diff(other_file, base_file, context_lines)
                .await
                .map_err(ServiceError::from)
        }
    }

    /// Call remote diff service for headerless unified diff
    async fn call_remote_diff_service(
        &self,
        ctx: &CoreContext,
        repo_name: &str,
        other_file: &FileContext<Repo>,
        base_file: &FileContext<Repo>,
        context_lines: usize,
    ) -> Result<HeaderlessUnifiedDiff, ServiceError> {
        let diff_service_client = self
            .diff_service_client
            .as_ref()
            .ok_or_else(|| scs_errors::internal_error("diff_service_client not configured"))?;

        let other_content_id = other_file.id().await?;
        let base_content_id = base_file.id().await?;

        let base_input = diff_service_client::DiffInput::content(base_content_id);
        let other_input = diff_service_client::DiffInput::content(other_content_id);

        let options = Some(diff_service_if::DiffUnifiedHeaderlessOptions {
            context_lines: context_lines as i32,
            inspect_binary_data: false,
            ..Default::default()
        });

        let repo_client = diff_service_client::RepoDiffServiceClient::new(
            repo_name.to_string(),
            diff_service_client.clone(),
        );

        let (response, mut stream) = repo_client
            .diff_unified_headerless(ctx, base_input, other_input, options)
            .await
            .map_err(|e| scs_errors::internal_error(format!("diff service error: {}", e)))?;

        let mut raw_diff = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                scs_errors::internal_error(format!("diff service stream error: {}", e))
            })?;
            raw_diff.extend_from_slice(&chunk.content);
        }

        Ok(HeaderlessUnifiedDiff {
            raw_diff,
            is_binary: response.is_binary,
        })
    }
}
