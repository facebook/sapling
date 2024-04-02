/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use configo_thrift::AutoCanaryRequirements;
use configo_thrift::ChangeCannotBeReverted;
pub use configo_thrift::ConfigObject;
use configo_thrift::ErrorType;
use configo_thrift::GetRequest;
use configo_thrift::Mutation;
use configo_thrift::MutationInfo;
use configo_thrift::MutationResponse;
use configo_thrift::MutationState;
use configo_thrift::MutationStateInfo;
use configo_thrift::PrepareRequest;
use configo_thrift_clients::ConfigoService;
use fbinit::FacebookInit;
use megarepo_error::bail_internal;
use megarepo_error::MegarepoError;
use slog::debug;
use slog::info;
use slog::warn;
use slog::Logger;

/// The amount of time to wait until the Mutation is in Prepared state
/// When timed out, the mutation has no effect
const MAX_WAIT_FOR_MUTATION_PREPARE: Duration = Duration::from_millis(20_000);
/// The amount of time to wait until the Mutation is in Landed state
/// When timed out, the mutation *may still land*
const MAX_WAIT_FOR_MUTATION_COMMIT: Duration = Duration::from_millis(20_000);

#[derive(Debug, Clone, Copy)]
struct MutationId(i64);

/// Wrapper around a Configo thrift client
/// with convenience methods for compare-and-swap writes
#[cfg(fbcode_build)]
pub struct MononokeConfigoClient {
    thrift_client: Arc<dyn ConfigoService + Send + Sync>,
}

#[cfg(fbcode_build)]
impl MononokeConfigoClient {
    /// Create a new instance of the client
    pub fn new(fb: FacebookInit) -> Result<Self> {
        let thrift_client =
            configo_thrift_srclients::make_ConfigoService_srclient!(fb, tiername = "configo.api")?;

        Ok(Self { thrift_client })
    }

    /// Create a new mutation instance, changing configs at `cconf_paths`.
    /// The actual changes to the config objects are created by a `value_updater`
    /// Returns:
    /// - `Ok(Some(id))` if mutation is created
    /// - `Ok(None)` if `value_updated` produced no changes
    /// - `Err(_)` if an error occurs
    async fn create_mutation(
        &self,
        logger: &Logger,
        cconf_paths: Vec<String>,
        value_updater: impl FnOnce(BTreeMap<String, ConfigObject>) -> Result<Vec<ConfigObject>>,
        mutation_title: String,
        mutation_summary: Option<String>,
    ) -> Result<Option<MutationId>, MegarepoError> {
        debug!(logger, "Issuing configo::GetRequest");
        let get_request = GetRequest {
            objects: cconf_paths,
            ..Default::default()
        };

        let resp = self
            .thrift_client
            .get(&get_request)
            .await
            .map_err(MegarepoError::internal)?;
        debug!(
            logger,
            "configo::GetRequest returned response with version {:?}", resp.version
        );

        debug!(logger, "Calling value updater for received config values");
        // Errors from updated are always treated as request errors
        let new_objects = value_updater(resp.objects).map_err(MegarepoError::request)?;
        debug!(logger, "Value updater returned");

        let mutation_info = MutationInfo {
            author: Some(String::from("svcscm")),
            authorEmail: Some(String::from("svcscm@fb.com")),
            title: mutation_title,
            summary: mutation_summary,
            ..Default::default()
        };

        let prepare_req = PrepareRequest {
            objects: new_objects,
            info: Some(mutation_info),
            deps: Default::default(),
            runThriftCValidators: true,
            // Version is needed for compare-and-swap semantics: this way two concurrent
            // modifications of the same config cannot succeed
            version: Some(resp.version),
            ..Default::default()
        };

        debug!(logger, "Calling configo::prepare");
        let mutation_id = match self
            .thrift_client
            .prepare(&prepare_req)
            .await
            .map_err(MegarepoError::internal)?
        {
            MutationResponse {
                error: true,
                errorType: ErrorType::EMPTY_COMMIT_ERROR,
                ..
            } => {
                return Ok(None);
            }
            MutationResponse {
                error: true,
                errorType: error_type,
                errorMessage: error_message,
                ..
            } => {
                bail_internal!(
                    "Configo exception while preparing  (type={}): {}",
                    error_type,
                    error_message
                );
            }
            MutationResponse {
                error: false,
                mutation,
                ..
            } => mutation.map_or(0, |Mutation { id, .. }| id),
        };

        Ok(Some(MutationId(mutation_id)))
    }

    /// Wait until a mutation is in a given state
    /// Returns:
    /// - `Ok(true)` if the state is reached
    /// - `Ok(false)` if `EMPTY_COMMIT_ERROR` is encountered
    /// - `Err(_)` if mutation is `FAILED` or if timeout occurs
    async fn wait_for_state(
        &self,
        logger: &Logger,
        mutation_id: MutationId,
        state: MutationState,
        max_wait_time: Duration,
    ) -> Result<bool> {
        let start = std::time::Instant::now();
        let iteration_delay = Duration::from_millis(250u64);
        let mut last_known_state: Option<MutationStateInfo> = None;
        loop {
            if start.elapsed() > max_wait_time {
                warn!(
                    logger,
                    "Last known state of mutation {} is {:?}", mutation_id.0, last_known_state
                );
                bail!(
                    "Timed out while waiting for mutation to be in state {}",
                    state
                );
            }
            let resp = self.thrift_client.status(&mutation_id.0).await?;
            if resp.error {
                bail!(resp.errorMessage)
            }
            let mutation = resp
                .mutation
                .with_context(|| format!("Mutation {} missing", mutation_id.0))?;

            if mutation.stateInfo.state == state {
                break;
            }

            last_known_state = Some(mutation.stateInfo.clone());
            match mutation.stateInfo {
                MutationStateInfo {
                    errorType: Some(ErrorType::EMPTY_COMMIT_ERROR),
                    ..
                } => {
                    return Ok(false);
                }
                MutationStateInfo { isError: true, .. }
                | MutationStateInfo {
                    state: MutationState::FAILED,
                    ..
                } => {
                    // Weird matching condition in this branch is because
                    // mutation commit can fail without putting a mutation
                    // into a "FAILED" state - for instance, when email is missing
                    // At the same time, I am not sure whether isError is always
                    // set either, so let's just match both of these.
                    bail!(
                        "Mutation {} FAILED: {} ({:?})",
                        mutation_id.0,
                        mutation.stateInfo.errorMessage,
                        mutation.stateInfo.errorType
                    );
                }
                _ => {}
            }
            tokio::time::sleep(iteration_delay).await;
        }

        Ok(true)
    }

    /// Update configs at `cconf_paths` by using `value_updater` callback
    /// This fn will wait until the mutation is prepared, and then until it is
    /// landed. Both waits have timeouts, and as such it is important to understand
    /// what happens when those expire:
    /// 1. If the preparation timeout expires, the mutation can end up in
    ///    either PREPARING or PREPARED states, but not be landed
    /// 2. If the landing timeout expires, the mutation may be either landed or
    ///    not landed (i.e. it may land after the expiration happens). The expected
    ///    client behavior in a case of landing expiration is to retry. If landing
    ///    the mutation actually succeeds after the expiration, but before the retry,
    ///    second call to `update_config` will recognize a no-op mutation and succeed.
    /// Interesting params:
    /// - `cconf_paths` - a list of `.cconf` configerator paths being updated. These
    ///                   should all start with `source/`. If you need to create a new
    ///                   object (rather than modify existing), you should still list
    ///                   its path here
    /// - `value_updater` - a modifying fn, which receives existing objects at (`cconf_paths`),
    ///                     and returns modified versions of those
    /// - `mutation_title`/`mutation_summary`- title and summary of the modification commit
    /// Returns:
    /// - `Ok(true)` if this call produced the right change
    /// - `Ok(false)` if the right change was already present in a repo
    /// - `Err(_)` on error
    pub async fn update_config(
        &self,
        logger: &Logger,
        cconf_paths: Vec<String>,
        value_updater: impl FnOnce(BTreeMap<String, ConfigObject>) -> Result<Vec<ConfigObject>>,
        mutation_title: String,
        mutation_summary: Option<String>,
    ) -> Result<bool, MegarepoError> {
        info!(logger, "Creating a mutation for paths {:?}", cconf_paths);
        let mutation_id = match self
            .create_mutation(
                logger,
                cconf_paths,
                value_updater,
                mutation_title,
                mutation_summary,
            )
            .await?
        {
            Some(mutation_id) => mutation_id,
            None => {
                warn!(logger, "Mutation wasn't created: EMPTY_COMMIT_ERROR");
                return Ok(false);
            }
        };

        info!(logger, "Waiting for mutation {} to prepare", mutation_id.0);
        if !self
            .wait_for_state(
                logger,
                mutation_id,
                MutationState::PREPARED,
                MAX_WAIT_FOR_MUTATION_PREPARE,
            )
            .await?
        {
            warn!(
                logger,
                "Waiting for mutation preparation returned on EMPTY_COMMIT_ERROR"
            );
            return Ok(false);
        };

        info!(
            logger,
            "Submitting mutation commit request for {}", mutation_id.0
        );
        let land_request = configo_thrift::LandRequest {
            mutationId: mutation_id.0,
            autoCanaryRequirements: Some(AutoCanaryRequirements {
                changeCannotBeReverted: Some(ChangeCannotBeReverted {
                    details: "This is used for changing the megarepo configuration which must happen with human supervision, for instance by locking the appropriate repos during the change".to_string(),

                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.thrift_client
            .land(&land_request)
            .await
            .map_err(MegarepoError::internal)?;
        if !self
            .wait_for_state(
                logger,
                mutation_id,
                MutationState::LANDED,
                MAX_WAIT_FOR_MUTATION_COMMIT,
            )
            .await?
        {
            warn!(
                logger,
                "Waiting for mutation to land returned on EMPTY_COMMIT_ERROR"
            );
            return Ok(false);
        }

        info!(logger, "Mutation {} is landed", mutation_id.0);
        Ok(true)
    }
}
