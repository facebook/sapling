/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use context::CoreContext;
use diff_service_client::DiffInput;
use diff_service_client::DiffServiceClient;
use diff_service_client::RepoDiffServiceClient;
use diff_service_if_clients::errors::DiffBlocksError;
use diff_service_if_clients::errors::DiffHunksError;
use diff_service_if_clients::errors::DiffUnifiedError;
use diff_service_if_clients::errors::DiffUnifiedHeaderlessError;
use diff_service_if_clients::errors::MetadataDiffError;
use environment::RemoteDiffOptions;
use futures::StreamExt;
use futures_retry::retry;
use metaconfig_types::RemoteDiffConfig;
use mononoke_api::ChangesetPathContentContext;
use mononoke_api::ChangesetPathDiffContext;
use mononoke_api::FileContext;
use mononoke_api::HeaderlessUnifiedDiff;
use mononoke_api::MononokeError;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api::UnifiedDiff;
use mononoke_api::UnifiedDiffMode;
use mononoke_api::headerless_unified_diff;
use mononoke_types::NonRootMPath;
use scs_errors::ServiceError;

// Retry configuration for transient diff service errors
const DIFF_SERVICE_RETRY_BASE_DELAY: Duration = Duration::from_secs(1);
const DIFF_SERVICE_MAX_RETRY_ATTEMPTS: usize = 5;
const DIFF_SERVICE_BACKOFF_MULTIPLIER: f64 = 1.5;

/// Trait for extracting RequestError from diff service error types.
/// All diff service operations throw the same error types (RequestError and InternalError),
/// so this trait allows us to uniformly check for transient errors across all operations.
trait DiffServiceError {
    fn request_error(&self) -> Option<&diff_service_if::RequestError>;
}

impl DiffServiceError for DiffUnifiedHeaderlessError {
    fn request_error(&self) -> Option<&diff_service_if::RequestError> {
        match self {
            Self::ex(req_err) => Some(req_err),
            _ => None,
        }
    }
}

impl DiffServiceError for DiffUnifiedError {
    fn request_error(&self) -> Option<&diff_service_if::RequestError> {
        match self {
            Self::ex(req_err) => Some(req_err),
            _ => None,
        }
    }
}

impl DiffServiceError for DiffHunksError {
    fn request_error(&self) -> Option<&diff_service_if::RequestError> {
        match self {
            Self::ex(req_err) => Some(req_err),
            _ => None,
        }
    }
}

impl DiffServiceError for DiffBlocksError {
    fn request_error(&self) -> Option<&diff_service_if::RequestError> {
        match self {
            Self::ex(req_err) => Some(req_err),
            _ => None,
        }
    }
}

impl DiffServiceError for MetadataDiffError {
    fn request_error(&self) -> Option<&diff_service_if::RequestError> {
        match self {
            Self::ex(req_err) => Some(req_err),
            _ => None,
        }
    }
}

/// Check if an error from the diff service is transient and should be retried.
/// This checks for errors marked as transient by the diff service (e.g., repo not found
/// during shard reallocation, repo initialization, etc.).
///
/// This function works with any diff service error type (DiffUnifiedError,
/// DiffUnifiedHeaderlessError, DiffHunksError, DiffBlocksError, MetadataDiffError)
/// since they all throw the same RequestError exception type.
fn is_transient_diff_error<E: DiffServiceError>(e: &E) -> bool {
    if let Some(request_error) = e.request_error() {
        matches!(
            &request_error.reason,
            diff_service_if::RequestErrorReason::transient_error(_)
        )
    } else {
        false
    }
}

/// Router for diff operations that can use either local mononoke_api
/// or remote diff_service based on command line args and JustKnobs configuration.
pub struct DiffRouter<'a> {
    pub(crate) fb: fbinit::FacebookInit,
    pub(crate) diff_options: &'a RemoteDiffOptions,
    pub(crate) remote_diff_config: Option<&'a RemoteDiffConfig>,
}

impl<'a> DiffRouter<'a> {
    /// Create a DiffServiceClient based on the remote_diff_config.
    /// Falls back to Service Manager discovery if no config is provided.
    fn create_diff_service_client(
        &self,
        repo_name: &str,
    ) -> Result<DiffServiceClient, ServiceError> {
        let result = match self.remote_diff_config {
            Some(RemoteDiffConfig::HostPort(host_port)) => {
                DiffServiceClient::from_host_port(self.fb, host_port.clone()).map_err(|e| {
                    format!(
                        "Failed to create diff service client from host:port '{}': {}",
                        host_port, e
                    )
                })
            }
            Some(RemoteDiffConfig::SmcTier(smc_tier)) => {
                DiffServiceClient::from_tier_name(self.fb, smc_tier.clone()).map_err(|e| {
                    format!(
                        "Failed to create diff service client from SMC tier '{}': {}",
                        smc_tier, e
                    )
                })
            }
            Some(RemoteDiffConfig::ShardManagerTier(sm_tier)) => {
                DiffServiceClient::from_sm_tier_name(
                    self.fb,
                    sm_tier.clone(),
                    repo_name.to_string(),
                )
                .map_err(|e| {
                    format!(
                        "Failed to create diff service client from ShardManager tier '{}': {}",
                        sm_tier, e
                    )
                })
            }
            None => {
                // Fallback to default Service Manager discovery
                DiffServiceClient::new_with_sm(self.fb, repo_name.to_string()).map_err(|e| {
                    format!(
                        "Failed to create diff service client for repo '{}': {}",
                        repo_name, e
                    )
                })
            }
        };
        result.map_err(|e| scs_errors::internal_error(e).into())
    }

    /// Check if remote diff should be used for this repo
    fn should_use_remote_diff(&self, repo_name: &str) -> bool {
        // Gate 1: CLI flag must be enabled
        if !self.diff_options.diff_remotely {
            return false;
        }

        // Gate 2: Check JK - this is the kill switch in production
        match justknobs::eval("scm/mononoke:remote_diff", None, Some(repo_name)) {
            Ok(true) => {
                // JK explicitly enabled - allow remote diff
                true
            }
            Ok(false) => {
                // JK explicitly disabled - this is the kill switch, always block
                false
            }
            Err(_) => {
                // JK not configured (e.g., in integration tests)
                // Fall back to checking if remote_diff_config is present,
                // which indicates explicit test configuration
                self.remote_diff_config.is_some()
            }
        }
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
            self.remote_headerless_diff(ctx, repo_name, other_file, base_file, context_lines)
                .await
        } else {
            headerless_unified_diff(other_file, base_file, context_lines, false)
                .await
                .map_err(ServiceError::from)
        }
    }

    pub async fn unified_diff(
        &self,
        ctx: &CoreContext,
        repo_name: &str,
        path_context: &ChangesetPathDiffContext<Repo>,
        mode: UnifiedDiffMode,
        context_lines: usize,
    ) -> Result<UnifiedDiff, ServiceError> {
        if self.should_use_remote_diff(repo_name) {
            self.remote_unified_diff(ctx, repo_name, path_context, mode, context_lines)
                .await
        } else {
            Ok(path_context
                .unified_diff(ctx, context_lines, mode, false)
                .await?)
        }
    }

    pub async fn metadata_diff(
        &self,
        ctx: &CoreContext,
        repo_name: &str,
        path_context: &ChangesetPathDiffContext<Repo>,
    ) -> Result<mononoke_api::MetadataDiff, ServiceError> {
        if self.should_use_remote_diff(repo_name) {
            self.remote_metadata_diff(ctx, repo_name, path_context)
                .await
        } else {
            Ok(path_context.metadata_diff(ctx, false).await?)
        }
    }

    async fn remote_headerless_diff(
        &self,
        ctx: &CoreContext,
        repo_name: &str,
        other_file: &FileContext<Repo>,
        base_file: &FileContext<Repo>,
        context_lines: usize,
    ) -> Result<HeaderlessUnifiedDiff, ServiceError> {
        let other_content_id = other_file.id().await?;
        let base_content_id = base_file.id().await?;

        let base_input = Some(DiffInput::content(base_content_id));
        let other_input = Some(DiffInput::content(other_content_id));

        let options = Some(diff_service_if::DiffUnifiedHeaderlessOptions {
            context_lines: context_lines as i32,
            inspect_binary_data: false,
            ..Default::default()
        });

        let client = self.create_diff_service_client(repo_name)?;

        let repo_client = RepoDiffServiceClient::new(repo_name.to_string(), client);

        // Retry the diff service call with exponential backoff for transient errors
        let (result, _attempts) = retry(
            |attempt| {
                // Clone the values so they can be moved into the async block
                let repo_client = repo_client.clone();
                let base_input = base_input.clone();
                let other_input = other_input.clone();
                let options = options.clone();

                async move {
                    if attempt > 1 {
                        tracing::info!(
                            repo_name = %repo_name,
                            attempt = attempt,
                            "Retrying diff service call"
                        );
                    }

                    repo_client
                        .diff_unified_headerless(ctx, base_input, other_input, options)
                        .await
                }
            },
            DIFF_SERVICE_RETRY_BASE_DELAY,
        )
        .exponential_backoff(DIFF_SERVICE_BACKOFF_MULTIPLIER)
        .max_attempts(DIFF_SERVICE_MAX_RETRY_ATTEMPTS)
        .retry_if(|_attempt, e| is_transient_diff_error(e))
        .await
        .map_err(|e| scs_errors::internal_error(format!("diff service error: {:#?}", e)))?;

        let (response, mut stream) = result;

        let mut raw_diff = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                scs_errors::internal_error(format!("diff service stream error: {:#?}", e))
            })?;
            raw_diff.extend_from_slice(&chunk.content);
        }

        Ok(HeaderlessUnifiedDiff {
            raw_diff,
            is_binary: response.is_binary,
        })
    }

    fn input_from_changeset<R: MononokeRepo>(
        content: &ChangesetPathContentContext<R>,
        replacement_path: Option<String>,
    ) -> Result<DiffInput, MononokeError> {
        let path = NonRootMPath::try_from(content.path().clone())?.to_string();
        let changeset_id = content.changeset().id();
        Ok(diff_service_client::DiffInput::ChangesetPath {
            changeset_id,
            path,
            replacement_path,
        })
    }

    async fn remote_unified_diff(
        &self,
        ctx: &CoreContext,
        repo_name: &str,
        path_context: &ChangesetPathDiffContext<Repo>,
        mode: UnifiedDiffMode,
        context_lines: usize,
    ) -> Result<UnifiedDiff, ServiceError> {
        let replacement_path = path_context.subtree_copy_dest_path().map(|p| p.to_string());

        // The Base file is the "old" file, with Other is the "new" one
        // the replacement path goes in the "old" file, so that it can show
        // the new path after a move.
        let base_input = path_context
            .get_old_content()
            .map(|c| Self::input_from_changeset(c, replacement_path))
            .transpose()?;

        let other_input = path_context
            .get_new_content()
            .map(|c| Self::input_from_changeset(c, None))
            .transpose()?;

        let copy_info = path_context.copy_info();

        let file_type = match path_context
            .get_old_content()
            .or(path_context.get_new_content())
        {
            Some(content) => content.file_type().await?,
            None => None,
        };

        let options = diff_service_if::DiffUnifiedOptions {
            context_lines: context_lines as i32,
            diff_mode: match mode {
                UnifiedDiffMode::Inline => diff_service_if::DiffMode::INLINE,
                UnifiedDiffMode::OmitContent => diff_service_if::DiffMode::OMIT_CONTENT,
            },
            file_type: file_type.map_or(diff_service_if::DiffFileType::REGULAR, |file_type| {
                match file_type {
                    mononoke_api::FileType::Regular => diff_service_if::DiffFileType::REGULAR,
                    mononoke_api::FileType::Executable => diff_service_if::DiffFileType::EXECUTABLE,
                    mononoke_api::FileType::Symlink => diff_service_if::DiffFileType::SYMLINK,
                    mononoke_api::FileType::GitSubmodule => {
                        diff_service_if::DiffFileType::GIT_SUBMODULE
                    }
                }
            }),
            copy_info: match copy_info {
                mononoke_api::CopyInfo::None => diff_service_if::DiffCopyInfo::NONE,
                mononoke_api::CopyInfo::Move => diff_service_if::DiffCopyInfo::MOVE,
                mononoke_api::CopyInfo::Copy => diff_service_if::DiffCopyInfo::COPY,
            },
            inspect_lfs_pointers: false,
            ..Default::default()
        };

        let client = self.create_diff_service_client(repo_name)?;

        let repo_client = RepoDiffServiceClient::new(repo_name.to_string(), client);

        let (result, _attempts) = retry(
            |attempt| {
                let repo_client = repo_client.clone();
                let base_input = base_input.clone();
                let other_input = other_input.clone();
                let options = options.clone();

                async move {
                    if attempt > 1 {
                        tracing::info!(
                            repo_name = %repo_name,
                            attempt = attempt,
                            "Retrying diff service call"
                        );
                    }

                    repo_client
                        .diff_unified(ctx, base_input, other_input, options)
                        .await
                }
            },
            DIFF_SERVICE_RETRY_BASE_DELAY,
        )
        .exponential_backoff(DIFF_SERVICE_BACKOFF_MULTIPLIER)
        .max_attempts(DIFF_SERVICE_MAX_RETRY_ATTEMPTS)
        .retry_if(|_attempt, e| is_transient_diff_error(e))
        .await
        .map_err(|e| scs_errors::internal_error(format!("diff service error: {:#?}", e)))?;

        let (response, mut stream) = result;
        let mut raw_diff = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                scs_errors::internal_error(format!("diff service stream error: {:#?}", e))
            })?;
            raw_diff.extend_from_slice(&chunk.content);
        }

        Ok(UnifiedDiff {
            raw_diff,
            is_binary: response.is_binary,
        })
    }

    async fn remote_metadata_diff(
        &self,
        ctx: &CoreContext,
        repo_name: &str,
        path_context: &ChangesetPathDiffContext<Repo>,
    ) -> Result<mononoke_api::MetadataDiff, ServiceError> {
        let replacement_path = path_context.subtree_copy_dest_path().map(|p| p.to_string());

        // The Base file is the "old" file, with Other is the "new" one
        // the replacement path goes in the "old" file, so that it can show
        // the new path after a move.
        let base_input = path_context
            .get_old_content()
            .map(|c| Self::input_from_changeset(c, replacement_path))
            .transpose()?;

        let other_input = path_context
            .get_new_content()
            .map(|c| Self::input_from_changeset(c, None))
            .transpose()?;

        let client = self.create_diff_service_client(repo_name)?;

        let repo_client = RepoDiffServiceClient::new(repo_name.to_string(), client);

        let (response, _attempts) = retry(
            |attempt| {
                let repo_client = repo_client.clone();
                let base_input = base_input.clone();
                let other_input = other_input.clone();

                async move {
                    if attempt > 1 {
                        tracing::info!(
                            repo_name = %repo_name,
                            attempt = attempt,
                            "Retrying diff service call"
                        );
                    }

                    repo_client
                        .metadata_diff(ctx, base_input, other_input, false)
                        .await
                }
            },
            DIFF_SERVICE_RETRY_BASE_DELAY,
        )
        .exponential_backoff(DIFF_SERVICE_BACKOFF_MULTIPLIER)
        .max_attempts(DIFF_SERVICE_MAX_RETRY_ATTEMPTS)
        .retry_if(|_attempt, e| is_transient_diff_error(e))
        .await
        .map_err(|e| scs_errors::internal_error(format!("diff service error: {:#?}", e)))?;

        // Convert the diff_service_if enums to mononoke_api enums
        let convert_file_type = |ft: Option<diff_service_if::DiffFileType>| -> Result<
            Option<mononoke_api::FileType>,
            ServiceError,
        > {
            ft.map(
                |file_type| -> Result<mononoke_api::FileType, ServiceError> {
                    match file_type {
                        diff_service_if::DiffFileType::REGULAR => {
                            Ok(mononoke_api::FileType::Regular)
                        }
                        diff_service_if::DiffFileType::EXECUTABLE => {
                            Ok(mononoke_api::FileType::Executable)
                        }
                        diff_service_if::DiffFileType::SYMLINK => {
                            Ok(mononoke_api::FileType::Symlink)
                        }
                        diff_service_if::DiffFileType::GIT_SUBMODULE => {
                            Ok(mononoke_api::FileType::GitSubmodule)
                        }
                        unknown => Err(scs_errors::internal_error(format!(
                            "Unknown file type from diff service: {:?}",
                            unknown
                        ))
                        .into()),
                    }
                },
            )
            .transpose()
        };

        let convert_content_type = |ct: Option<diff_service_if::DiffContentType>| -> Result<
            Option<mononoke_api::FileContentType>,
            ServiceError,
        > {
            ct.map(
                |content_type| -> Result<mononoke_api::FileContentType, ServiceError> {
                    match content_type {
                        diff_service_if::DiffContentType::TEXT => {
                            Ok(mononoke_api::FileContentType::Text)
                        }
                        diff_service_if::DiffContentType::NON_UTF8 => {
                            Ok(mononoke_api::FileContentType::NonUtf8)
                        }
                        diff_service_if::DiffContentType::BINARY => {
                            Ok(mononoke_api::FileContentType::Binary)
                        }
                        unknown => Err(scs_errors::internal_error(format!(
                            "Unknown content type from diff service: {:?}",
                            unknown
                        ))
                        .into()),
                    }
                },
            )
            .transpose()
        };

        let convert_generated_status = |gs: Option<diff_service_if::DiffGeneratedStatus>| -> Result<
            Option<mononoke_api::FileGeneratedStatus>,
            ServiceError,
        > {
            gs.map(
                |generated_status| -> Result<mononoke_api::FileGeneratedStatus, ServiceError> {
                    match generated_status {
                        diff_service_if::DiffGeneratedStatus::FULLY => {
                            Ok(mononoke_api::FileGeneratedStatus::FullyGenerated)
                        }
                        diff_service_if::DiffGeneratedStatus::PARTIALLY => {
                            Ok(mononoke_api::FileGeneratedStatus::PartiallyGenerated)
                        }
                        diff_service_if::DiffGeneratedStatus::NON_GENERATED => {
                            Ok(mononoke_api::FileGeneratedStatus::NotGenerated)
                        }
                        unknown => Err(scs_errors::internal_error(format!(
                            "Unknown generated status from diff service: {:?}",
                            unknown
                        ))
                        .into()),
                    }
                },
            )
            .transpose()
        };

        // Convert the response back to mononoke_api::MetadataDiff
        Ok(mononoke_api::MetadataDiff {
            old_file_info: mononoke_api::MetadataDiffFileInfo {
                file_type: convert_file_type(response.base_file_info.file_type)?,
                file_content_type: convert_content_type(response.base_file_info.content_type)?,
                file_generated_status: convert_generated_status(
                    response.base_file_info.generated_status,
                )?,
            },
            new_file_info: mononoke_api::MetadataDiffFileInfo {
                file_type: convert_file_type(response.other_file_info.file_type)?,
                file_content_type: convert_content_type(response.other_file_info.content_type)?,
                file_generated_status: convert_generated_status(
                    response.other_file_info.generated_status,
                )?,
            },
            lines_count: response
                .lines_count
                .map(|lc| mononoke_api::MetadataDiffLinesCount {
                    added_lines_count: lc.added_lines as usize,
                    deleted_lines_count: lc.deleted_lines as usize,
                    significant_added_lines_count: lc.significant_added_lines as usize,
                    significant_deleted_lines_count: lc.significant_deleted_lines as usize,
                    first_added_line_number: lc.first_added_line_number.map(|n| n as usize),
                }),
        })
    }
}

#[cfg(test)]
mod tests {
    use environment::RemoteDiffOptions;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::with_just_knobs;
    use maplit::hashmap;
    use metaconfig_types::RemoteDiffConfig;
    use mononoke_macros::mononoke;

    use super::*;

    fn create_diff_router<'a>(
        fb: fbinit::FacebookInit,
        diff_options: &'a RemoteDiffOptions,
        remote_diff_config: Option<&'a RemoteDiffConfig>,
    ) -> DiffRouter<'a> {
        DiffRouter {
            fb,
            diff_options,
            remote_diff_config,
        }
    }

    #[mononoke::fbinit_test]
    fn test_should_use_remote_diff_cli_flag_disabled(fb: fbinit::FacebookInit) {
        let diff_options = RemoteDiffOptions {
            diff_remotely: false,
        };
        let router = create_diff_router(fb, &diff_options, None);

        // Even with JK enabled, should return false when CLI flag is off
        let result = with_just_knobs(
            JustKnobsInMemory::new(hashmap![
                "scm/mononoke:remote_diff".to_string() => KnobVal::Bool(true)
            ]),
            || router.should_use_remote_diff("test_repo"),
        );
        assert!(!result, "Should return false when CLI flag is disabled");
    }

    #[mononoke::fbinit_test]
    fn test_should_use_remote_diff_jk_disabled(fb: fbinit::FacebookInit) {
        // When JK is disabled, should always return false even with CLI flag enabled
        let diff_options = RemoteDiffOptions {
            diff_remotely: true,
        };
        let router = create_diff_router(fb, &diff_options, None);

        let result = with_just_knobs(
            JustKnobsInMemory::new(hashmap![
                "scm/mononoke:remote_diff".to_string() => KnobVal::Bool(false)
            ]),
            || router.should_use_remote_diff("test_repo"),
        );
        assert!(!result, "Should return false when JK is disabled");
    }

    #[mononoke::fbinit_test]
    fn test_should_use_remote_diff_jk_is_kill_switch_with_config(fb: fbinit::FacebookInit) {
        // Even with remote_diff_config present, JK should still act as kill switch
        // This was the bug - config presence used to bypass JK check
        let diff_options = RemoteDiffOptions {
            diff_remotely: true,
        };
        let config = RemoteDiffConfig::HostPort("localhost:8080".to_string());
        let router = create_diff_router(fb, &diff_options, Some(&config));

        // With JK disabled, should return false even though config is present
        let result = with_just_knobs(
            JustKnobsInMemory::new(hashmap![
                "scm/mononoke:remote_diff".to_string() => KnobVal::Bool(false)
            ]),
            || router.should_use_remote_diff("test_repo"),
        );
        assert!(
            !result,
            "JK should act as kill switch even when remote_diff_config is present"
        );
    }

    #[mononoke::fbinit_test]
    fn test_should_use_remote_diff_enabled(fb: fbinit::FacebookInit) {
        // When both CLI flag and JK are enabled, should return true
        let diff_options = RemoteDiffOptions {
            diff_remotely: true,
        };
        let router = create_diff_router(fb, &diff_options, None);

        let result = with_just_knobs(
            JustKnobsInMemory::new(hashmap![
                "scm/mononoke:remote_diff".to_string() => KnobVal::Bool(true)
            ]),
            || router.should_use_remote_diff("test_repo"),
        );
        assert!(
            result,
            "Should return true when both CLI flag and JK are enabled"
        );
    }

    #[mononoke::fbinit_test]
    fn test_should_use_remote_diff_enabled_with_config(fb: fbinit::FacebookInit) {
        // When both CLI flag and JK are enabled with config, should return true
        let diff_options = RemoteDiffOptions {
            diff_remotely: true,
        };
        let config = RemoteDiffConfig::SmcTier("diff_service.smc".to_string());
        let router = create_diff_router(fb, &diff_options, Some(&config));

        let result = with_just_knobs(
            JustKnobsInMemory::new(hashmap![
                "scm/mononoke:remote_diff".to_string() => KnobVal::Bool(true)
            ]),
            || router.should_use_remote_diff("test_repo"),
        );
        assert!(
            result,
            "Should return true when CLI flag and JK are enabled with config"
        );
    }

    #[mononoke::fbinit_test]
    fn test_should_use_remote_diff_jk_not_configured_no_config(fb: fbinit::FacebookInit) {
        // When JK is not configured (returns error) and no config, should return false
        let diff_options = RemoteDiffOptions {
            diff_remotely: true,
        };
        let router = create_diff_router(fb, &diff_options, None);

        // Don't use with_just_knobs - JK will fail to evaluate
        let result = router.should_use_remote_diff("test_repo");
        assert!(
            !result,
            "Should return false when JK is not configured and no config is present"
        );
    }

    #[mononoke::fbinit_test]
    fn test_should_use_remote_diff_jk_not_configured_with_config(fb: fbinit::FacebookInit) {
        // When JK is not configured (returns error) but config is present,
        // should return true (test/dev mode fallback)
        let diff_options = RemoteDiffOptions {
            diff_remotely: true,
        };
        let config = RemoteDiffConfig::HostPort("localhost:8080".to_string());
        let router = create_diff_router(fb, &diff_options, Some(&config));

        // Don't use with_just_knobs - JK will fail to evaluate
        let result = router.should_use_remote_diff("test_repo");
        assert!(
            result,
            "Should return true when JK is not configured but config is present (test mode)"
        );
    }
}
