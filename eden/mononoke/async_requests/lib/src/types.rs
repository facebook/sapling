/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Debug;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
pub use async_requests_types_thrift::AsynchronousRequestParams as ThriftAsynchronousRequestParams;
pub use async_requests_types_thrift::AsynchronousRequestParamsId as ThriftAsynchronousRequestParamsId;
pub use async_requests_types_thrift::AsynchronousRequestResult as ThriftAsynchronousRequestResult;
pub use async_requests_types_thrift::AsynchronousRequestResultId as ThriftAsynchronousRequestResultId;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::impl_loadable_storable;
use context::CoreContext;
use fbthrift::compact_protocol;
use futures_watchdog::WatchdogExt;
pub use megarepo_config::SyncTargetConfig;
pub use megarepo_config::Target;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_types::BlobstoreKey;
use mononoke_types::RepositoryId;
use mononoke_types::hash::Blake2;
use mononoke_types::impl_typed_context;
use mononoke_types::impl_typed_hash_no_context;
pub use requests_table::RequestStatus;
pub use requests_table::RequestType;
pub use requests_table::RowId;
pub use source_control as thrift;

use crate::error::AsyncRequestsError;

const LEGACY_VALUE_TYPE_PARAMS: [&str; 1] = [
    // Support the old format during the transition
    "MegarepoAsynchronousRequestParams",
];

/// Grouping of types and behaviors for an asynchronous request
pub trait Request: Sized + Send + Sync {
    /// Name of the request
    const NAME: &'static str;
    /// Rust newtype for a polling token
    type Token: Token;

    /// Underlying thrift type for request params
    type ThriftParams: ThriftParams<R = Self>;

    /// Underlying thrift type for successful request response
    type ThriftResponse;

    /// Underlying thrift type for for request result (response or error)
    type ThriftResult: ThriftResult<R = Self>;

    /// A type representing potentially present response
    type PollResponse;

    /// Convert thrift result into a result of a poll response
    fn thrift_result_into_poll_response(tr: Self::ThriftResult) -> Self::PollResponse;

    /// Return an empty poll response. This indicates
    /// that the request hasn't been processed yet
    fn empty_poll_response() -> Self::PollResponse;
}

/// Thrift type representing async service method parameters
pub trait ThriftParams: Sized + Send + Sync + Into<AsynchronousRequestParams> + Debug {
    type R: Request<ThriftParams = Self>;

    /// Every *Params argument refers to some Target
    /// This method is needed to extract it from the
    /// implementer of this trait
    fn target(&self) -> String;
}
pub trait ThriftResult:
    Sized + Send + Sync + TryFrom<AsynchronousRequestResult, Error = AsyncRequestsError>
{
    type R: Request<ThriftResult = Self>;
}

/// Polling token for an async service method
pub trait Token: Clone + Sized + Send + Sync + Debug {
    type R: Request<Token = Self>;
    type ThriftToken;

    fn into_thrift(self) -> Self::ThriftToken;
    fn from_db_id(id: RowId) -> Result<Self, AsyncRequestsError>;
    fn to_db_id(&self) -> Result<RowId, AsyncRequestsError>;

    fn id(&self) -> RowId;
}

/// This macro implements an async service method type,
/// which can be stored/retrieved from the blobstore.
/// Such types are usually represented as value/handle pairs.
/// Since we need to implement (potentially foreign) traits
/// on these types, we also define corresponding Rust types
/// Some of the defined types (like context or thrift_type_newtype)
/// are not used from outside of the macro, but we still need
/// to pass identifiers for them from the outside, because
/// Rusts' macro hygiene does not allow identifier generation ¯\_(ツ)_/¯
macro_rules! impl_async_svc_stored_type {
    {
        /// Rust type for the Loadable handle
        handle_type => $handle_type: ident,
        /// Underlying thrift type for the handle
        handle_thrift_type => $handle_thrift_type: ident,
        /// A name for a Newtype-style trait, required by `impl_typed_hash_no_context`
        /// Rust type for the Storable value
        value_type => $value_type: ident,
        /// Underlying thrift type for the value
        value_thrift_type => $value_thrift_type: ident,
        /// A helper struct for hash computations
        context_type => $context_type: ident,
    } => {
        /// Rust handle type, wrapper around a Blake2 instance
        #[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
        pub struct $handle_type(Blake2);

        impl_typed_hash_no_context! {
            hash_type => $handle_type,
            thrift_type => $handle_thrift_type,
            blobstore_key => concat!("async.svc.", stringify!($value_type)),
        }

        // Typed context type is needed for hash computation
        impl_typed_context! {
            hash_type => $handle_type,
            context_type => $context_type,
            context_key => stringify!($value_type),
        }

        /// Main value type
        #[derive(Debug, Clone, PartialEq)]
        pub struct $value_type {
            id: $handle_type,
            thrift: $value_thrift_type,
        }

        impl $value_type {
            pub fn from_thrift(thrift: $value_thrift_type) -> Self {
                let data = compact_protocol::serialize(&thrift);
                let mut context = $context_type::new();
                context.update(&data);
                let id = context.finish();
                Self { id, thrift }
            }

            pub async fn load_from_key(ctx: &CoreContext, blobstore: &Arc<dyn Blobstore>, key: &str) -> Result<Self, AsyncRequestsError> {
                let bytes = blobstore.get(ctx, key).await?;
                Self::check_prefix(key)?;
                match bytes {
                    Some(bytes) => Ok(bytes.into_bytes().try_into()?),
                    None => Err(AsyncRequestsError::internal(anyhow!("Missing blob: {}", key))),
                }
            }

            pub fn check_prefix(key: &str) -> Result<(), AsyncRequestsError> {
                let prefix = concat!("async.svc.", stringify!($value_type), ".blake2.");
                if key.strip_prefix(prefix).is_some() {
                    return Ok(());
                }

                // if the standard prefix is not valid, this might be in one of an alternative prefixes we support
                for vt in LEGACY_VALUE_TYPE_PARAMS {
                    let prefix = format!("async.svc.{}.blake2.", vt);
                    if key.strip_prefix(&prefix).is_some() {
                        return Ok(());
                    }
                }

                return Err(AsyncRequestsError::internal(anyhow!("{} is not a blobstore key for {}", key, stringify!($value_type))));
            }

            pub fn handle(&self) -> &$handle_type {
                &self.id
            }

            pub fn thrift(&self) -> &$value_thrift_type {
                &self.thrift
            }

        }

        // Conversions between thrift types and their Rust counterparts

        impl TryFrom<$handle_thrift_type> for $handle_type {
            type Error = Error;

            fn try_from(t: $handle_thrift_type) -> Result<Self, Self::Error> {
                Self::from_thrift(t)
            }
        }

        impl From<$handle_type> for $handle_thrift_type {
            fn from(other: $handle_type) -> Self {
                Self(mononoke_types_serialization::id::Id::Blake2(other.0.into_thrift()))
            }
        }

        impl TryFrom<$value_thrift_type> for $value_type {
            type Error = Error;

            fn try_from(t: $value_thrift_type) -> Result<Self, Self::Error> {
                Ok(Self::from_thrift(t))
            }
        }

        impl From<$value_type> for $value_thrift_type {
            fn from(other: $value_type) -> Self {
                other.thrift
            }
        }

        impl_loadable_storable! {
            handle_type => $handle_type,
            handle_thrift_type => $handle_thrift_type,
            value_type => $value_type,
            value_thrift_type => $value_thrift_type,
        }
    }
}

/// Implement the Params type for an async method, and any conversions.
macro_rules! impl_async_svc_method_types_params {
    {
        request_struct => $request_struct: ident,
        params_value_thrift_type => $params_value_thrift_type: ident,
        params_union_variant => $params_union_variant: ident,

        fn target(&$self_ident: ident: ThriftParams) -> String $target_in_params: tt
    } => {
        impl ThriftParams for thrift::$params_value_thrift_type {
            type R = $request_struct;

            fn target(&$self_ident) -> String {
                $target_in_params
            }
        }

        impl From<thrift::$params_value_thrift_type> for AsynchronousRequestParams{
            fn from(params: thrift::$params_value_thrift_type) -> AsynchronousRequestParams {
                AsynchronousRequestParams::from_thrift(
                    ThriftAsynchronousRequestParams::$params_union_variant(params)
                )
            }
        }
    }
}

/// Implement the Token type for an async method, and any conversions.
macro_rules! impl_async_svc_method_types_token {
    {
        request_struct => $request_struct: ident,
        token_type => $token_type: ident,
        token_thrift_type => $token_thrift_type: ident,
    } => {
        #[derive(Clone, Debug)]
        pub struct $token_type(pub thrift::$token_thrift_type);

        impl Token for $token_type {
            type ThriftToken = thrift::$token_thrift_type;
            type R = $request_struct;

            fn from_db_id(id: RowId) -> Result<Self, AsyncRequestsError> {
                // Thrift token is a string alias
                // but's guard ourselves here against
                // it changing unexpectedly.
                let thrift_token = thrift::$token_thrift_type {
                    id: id.0 as i64,
                    ..Default::default()
                };
                Ok(Self(thrift_token))
            }

            fn to_db_id(&self) -> Result<RowId, AsyncRequestsError> {
                let row_id = self.0.id as u64;
                let row_id = RowId(row_id);

                Ok(row_id)
            }

            fn id(&self) -> RowId {
                RowId(self.0.id as u64)
            }

            fn into_thrift(self) -> thrift::$token_thrift_type {
                self.0
            }
        }
    }
}

/// Implement the Result type for an async method, and any conversions.
macro_rules! impl_async_svc_method_types_result {
    {
        request_struct => $request_struct: ident,
        result_value_thrift_type => $result_value_thrift_type: ident,
        result_union_variant => $result_union_variant: ident,
    } => {
        impl ThriftResult for thrift::$result_value_thrift_type {
            type R = $request_struct;
        }

        impl From<thrift::$result_value_thrift_type> for AsynchronousRequestResult {
            fn from(r: thrift::$result_value_thrift_type) -> AsynchronousRequestResult {
                let thrift = ThriftAsynchronousRequestResult::$result_union_variant(r);
                AsynchronousRequestResult::from_thrift(thrift)
            }
        }

        impl TryFrom<AsynchronousRequestResult> for thrift::$result_value_thrift_type {
            type Error = AsyncRequestsError;

            fn try_from(r: AsynchronousRequestResult) -> Result<thrift::$result_value_thrift_type, Self::Error> {
                match r.thrift {
                    ThriftAsynchronousRequestResult::$result_union_variant(payload) => Ok(payload),
                    ThriftAsynchronousRequestResult::error(e) => {
                        Err(e.into())
                    }
                    ThriftAsynchronousRequestResult::UnknownField(x) => {
                        // TODO: maybe use structured error?
                        Err(AsyncRequestsError::internal(
                            anyhow!(
                                "failed to parse {} thrift. UnknownField: {}",
                                stringify!(thrift::$result_value_thrift_type),
                                x,
                            )
                        ))
                    },
                    x => {
                        Err(AsyncRequestsError::internal(
                            anyhow!(
                                "failed to parse {} thrift. The result union contains the wrong result variant: {:?}",
                                stringify!(thrift::$result_value_thrift_type),
                                x,
                            )
                        ))
                    }
                }
            }
        }
    }
}

/// A macro to call impl_async_svc_stored_type for params/result
/// types, as well as define a bunch of relationships between
/// these types, and their Request-related frients.
/// An underlying idea is to define as much behavior and relationships
/// as possible in the type system, so that we
/// (a) minimize a chance of using incorrect pair of types somewhere
/// (b) can write generic enqueuing/polling functions
///
/// The arguments are as follows:
/// * method_name:              the Thrift method name
/// * request_struct:           the name of an internal struct to represent the request (declared by the macro)
/// * params_value_thrift_type: the Thrift structs that contains all the params
/// * params_union_variant:     the name of a variant of the AsynchronousRequestParams union, maatching the Thrift params struct
/// * response_type:            the name of the Thrift struct that holds the response to the method
/// * result_union_variant:     the name of a variant of the AsynchronousRequestResult union, matching the Thrift response struct
/// * poll_response_type:       the Thrift struct returned by the poll method associated with this method
/// * token_type:               the name of an internal struct to represent the token (declared by the macro)
/// * token_thrift_type:        the Thrift struct holding the token returned by the method and accepted by the poll method
macro_rules! impl_async_svc_method_types {
    {
        method_name => $method_name: expr,
        request_struct => $request_struct: ident,

        params_value_thrift_type => $params_value_thrift_type: ident,
        params_union_variant => $params_union_variant: ident,

        response_type => $response_type: ident,
        result_union_variant => $result_union_variant: ident,

        poll_response_type => $poll_response_type: ident,
        token_type => $token_type: ident,
        token_thrift_type => $token_thrift_type: ident,

        fn target(&$self_ident: ident: ThriftParams) -> String $target_in_params: tt

    } => {
        impl_async_svc_method_types_params!(
            request_struct => $request_struct,
            params_value_thrift_type => $params_value_thrift_type,
            params_union_variant => $params_union_variant,

            fn target(&$self_ident: ThriftParams) -> String {
                $target_in_params
            }
        );
        impl_async_svc_method_types_token!(
            request_struct => $request_struct,
            token_type => $token_type,
            token_thrift_type => $token_thrift_type,
        );

        impl_async_svc_method_types_result!(
            request_struct => $request_struct,
            result_value_thrift_type => $response_type,
            result_union_variant => $result_union_variant,
        );

        pub struct $request_struct;

        impl Request for $request_struct {
            const NAME: &'static str = $method_name;

            type Token = $token_type;
            type ThriftParams = thrift::$params_value_thrift_type;
            type ThriftResult = thrift::$response_type;
            type ThriftResponse = thrift::$response_type;
            type PollResponse = thrift::$poll_response_type;

            fn thrift_result_into_poll_response(
                thrift_result: Self::ThriftResult,
            ) -> Self::PollResponse {
                thrift::$poll_response_type::response(thrift_result)
            }

            fn empty_poll_response() -> Self::PollResponse {
                thrift::$poll_response_type::poll_pending ( thrift::PollPending{..Default::default() } )
            }
        }

        impl From<Result<thrift::$response_type, AsyncRequestsError>> for AsynchronousRequestResult {
            fn from(r: Result<thrift::$response_type, AsyncRequestsError>) -> AsynchronousRequestResult {
                let thrift = match r {
                    Ok(payload) => ThriftAsynchronousRequestResult::$result_union_variant(payload),
                    Err(e) => {
                        ThriftAsynchronousRequestResult::error(e.into())
                    }
                };

                AsynchronousRequestResult::from_thrift(thrift)
            }
        }

        impl TryFrom<AsynchronousRequestResult> for thrift::$poll_response_type {
            type Error = AsyncRequestsError;

            fn try_from(r: AsynchronousRequestResult) -> Result<thrift::$poll_response_type, Self::Error> {
                match r.thrift {
                    ThriftAsynchronousRequestResult::$result_union_variant(payload) => {
                        Ok(thrift::$poll_response_type::response(payload))
                    }
                    ThriftAsynchronousRequestResult::error(e) => {
                        Err(e.into())
                    }
                    ThriftAsynchronousRequestResult::UnknownField(x) => {
                        // TODO: maybe use structured error?
                        Err(AsyncRequestsError::internal(
                            anyhow!(
                                "failed to parse {} thrift. UnknownField: {}",
                                stringify!(thrift::$response_type),
                                x,
                            )
                        ))
                    }
                    x => {
                        Err(AsyncRequestsError::internal(
                            anyhow!(
                                "failed to parse {} thrift. The result union contains the wrong result variant: {:?}",
                                stringify!(thrift::$response_type),
                                x,
                            )
                        ))
                    }
                }
            }
        }
    }
}

/// Legacy version of impl_async_svc_method_types_legacy to maintain backward compatibility.
macro_rules! impl_async_svc_method_types_legacy {
    {
        method_name => $method_name: expr,
        request_struct => $request_struct: ident,

        params_value_thrift_type => $params_value_thrift_type: ident,
        params_union_variant => $params_union_variant: ident,

        result_value_thrift_type => $result_value_thrift_type: ident,
        result_union_variant => $result_union_variant: ident,

        response_type => $response_type: ident,
        poll_response_type => $poll_response_type: ident,
        token_type => $token_type: ident,
        token_thrift_type => $token_thrift_type: ident,

        fn target(&$self_ident: ident: ThriftParams) -> String $target_in_params: tt

    } => {
        impl_async_svc_method_types_params!(
            request_struct => $request_struct,
            params_value_thrift_type => $params_value_thrift_type,
            params_union_variant => $params_union_variant,

            fn target(&$self_ident: ThriftParams) -> String {
                $target_in_params
            }
        );
        impl_async_svc_method_types_token!(
            request_struct => $request_struct,
            token_type => $token_type,
            token_thrift_type => $token_thrift_type,
        );
        impl_async_svc_method_types_result!(
            request_struct => $request_struct,
            result_value_thrift_type => $result_value_thrift_type,
            result_union_variant => $result_union_variant,
        );

        pub struct $request_struct;

        impl Request for $request_struct {
            const NAME: &'static str = $method_name;

            type Token = $token_type;
            type ThriftParams = thrift::$params_value_thrift_type;
            type ThriftResult = thrift::$result_value_thrift_type;
            type ThriftResponse = thrift::$response_type;
            type PollResponse = thrift::$poll_response_type;

            fn thrift_result_into_poll_response(
                thrift_result: Self::ThriftResult,
            ) -> Self::PollResponse {
                thrift::$poll_response_type { result: Some(thrift_result), ..Default::default() }
            }

            fn empty_poll_response() -> Self::PollResponse {
                thrift::$poll_response_type { result: None, ..Default::default() }
            }
        }

        impl From<Result<thrift::$response_type, AsyncRequestsError>> for AsynchronousRequestResult {
            fn from(r: Result<thrift::$response_type, AsyncRequestsError>) -> AsynchronousRequestResult {
                let thrift = match r {
                    Ok(payload) => ThriftAsynchronousRequestResult::$result_union_variant(thrift::$result_value_thrift_type::success(payload)),
                    Err(e) => ThriftAsynchronousRequestResult::$result_union_variant(thrift::$result_value_thrift_type::error(e.into()))
                };

                AsynchronousRequestResult::from_thrift(thrift)
            }
        }
    }
}

// Params and result types for megarepo_add_sync_target

impl_async_svc_method_types_legacy! {
    method_name => "megarepo_add_sync_target",
    request_struct => MegarepoAddSyncTarget,

    params_value_thrift_type => MegarepoAddTargetParams,
    params_union_variant => megarepo_add_target_params,

    result_value_thrift_type => MegarepoAddTargetResult,
    result_union_variant => megarepo_add_target_result,

    response_type => MegarepoAddTargetResponse,
    poll_response_type => MegarepoAddTargetPollResponse,
    token_type => MegarepoAddTargetToken,
    token_thrift_type => MegarepoAddTargetToken,

    fn target(&self: ThriftParams) -> String {
        render_target(&self.config_with_new_target.target)
    }
}

// Params and result types for megarepo_add_branching_sync_target

impl_async_svc_method_types_legacy! {
    method_name => "megarepo_add_branching_sync_target",
    request_struct => MegarepoAddBranchingSyncTarget,

    params_value_thrift_type => MegarepoAddBranchingTargetParams,
    params_union_variant => megarepo_add_branching_target_params,

    result_value_thrift_type => MegarepoAddBranchingTargetResult,
    result_union_variant => megarepo_add_branching_target_result,

    response_type => MegarepoAddBranchingTargetResponse,
    poll_response_type => MegarepoAddBranchingTargetPollResponse,
    token_type => MegarepoAddBranchingTargetToken,
    token_thrift_type => MegarepoAddBranchingTargetToken,

    fn target(&self: ThriftParams) -> String {
        render_target(&self.target)
    }
}

// Params and result types for megarepo_change_target_config

impl_async_svc_method_types_legacy! {
    method_name => "megarepo_change_target_config",
    request_struct => MegarepoChangeTargetConfig,

    params_value_thrift_type => MegarepoChangeTargetConfigParams,
    params_union_variant => megarepo_change_target_params,

    result_value_thrift_type => MegarepoChangeTargetConfigResult,
    result_union_variant => megarepo_change_target_result,

    response_type => MegarepoChangeTargetConfigResponse,
    poll_response_type => MegarepoChangeTargetConfigPollResponse,
    token_type => MegarepoChangeTargetConfigToken,
    token_thrift_type => MegarepoChangeConfigToken,

    fn target(&self: ThriftParams) -> String {
        render_target(&self.target)
    }
}

// Params and result types for megarepo_sync_changeset

impl_async_svc_method_types_legacy! {
    method_name => "megarepo_sync_changeset",
    request_struct => MegarepoSyncChangeset,

    params_value_thrift_type => MegarepoSyncChangesetParams,
    params_union_variant => megarepo_sync_changeset_params,

    result_value_thrift_type => MegarepoSyncChangesetResult,
    result_union_variant => megarepo_sync_changeset_result,

    response_type => MegarepoSyncChangesetResponse,
    poll_response_type => MegarepoSyncChangesetPollResponse,
    token_type => MegarepoSyncChangesetToken,
    token_thrift_type => MegarepoSyncChangesetToken,

    fn target(&self: ThriftParams) -> String {
        render_target(&self.target)
    }
}

// Params and result types for megarepo_remerge_source

impl_async_svc_method_types_legacy! {
    method_name => "megarepo_remerge_source",
    request_struct => MegarepoRemergeSource,

    params_value_thrift_type => MegarepoRemergeSourceParams,
    params_union_variant => megarepo_remerge_source_params,

    result_value_thrift_type => MegarepoRemergeSourceResult,
    result_union_variant => megarepo_remerge_source_result,

    response_type => MegarepoRemergeSourceResponse,
    poll_response_type => MegarepoRemergeSourcePollResponse,
    token_type => MegarepoRemergeSourceToken,
    token_thrift_type => MegarepoRemergeSourceToken,

    fn target(&self: ThriftParams) -> String {
        render_target(&self.target)
    }
}

// Params and result types for async_ping

impl_async_svc_method_types! {
    method_name => "async_ping",
    request_struct => AsyncPing,

    params_value_thrift_type => AsyncPingParams,
    params_union_variant => async_ping_params,

    response_type => AsyncPingResponse,
    result_union_variant => async_ping_result,

    poll_response_type => AsyncPingPollResponse,
    token_type => AsyncPingToken,
    token_thrift_type => AsyncPingToken,

    fn target(&self: ThriftParams) -> String {
        "".to_string()
    }
}

// Params and result types for commit_sparse_profile_size_async

impl_async_svc_method_types! {
    method_name => "commit_sparse_profile_size_async",
    request_struct => CommitSparseProfileSize,

    params_value_thrift_type => CommitSparseProfileSizeParamsV2,
    params_union_variant => commit_sparse_profile_size_params,

    response_type => CommitSparseProfileSizeResponse,
    result_union_variant => commit_sparse_profile_size_result,

    poll_response_type => CommitSparseProfileSizePollResponse,
    token_type => CommitSparseProfileSizeToken,
    token_thrift_type => CommitSparseProfileSizeToken,

    fn target(&self: ThriftParams) -> String {
        format!(
            "repo: {}, id: {}",
            self.commit.repo,
            self.commit.id
        )
    }
}

// Params and result types for commit_sparse_profile_delta_async

impl_async_svc_method_types! {
    method_name => "commit_sparse_profile_delta_async",
    request_struct => CommitSparseProfileDelta,

    params_value_thrift_type => CommitSparseProfileDeltaParamsV2,
    params_union_variant => commit_sparse_profile_delta_params,

    response_type => CommitSparseProfileDeltaResponse,
    result_union_variant => commit_sparse_profile_delta_result,

    poll_response_type => CommitSparseProfileDeltaPollResponse,
    token_type => CommitSparseProfileDeltaToken,
    token_thrift_type => CommitSparseProfileDeltaToken,

    fn target(&self: ThriftParams) -> String {
        format!(
            "repo: {}, commit: {}, other: {}",
            self.commit.repo,
            self.commit.id,
            self.other_id,
        )
    }
}

impl_async_svc_stored_type! {
    handle_type => AsynchronousRequestParamsId,
    handle_thrift_type => ThriftAsynchronousRequestParamsId,
    value_type => AsynchronousRequestParams,
    value_thrift_type => ThriftAsynchronousRequestParams,
    context_type => AsynchronousRequestParamsIdContext,
}

impl_async_svc_stored_type! {
    handle_type => AsynchronousRequestResultId,
    handle_thrift_type => ThriftAsynchronousRequestResultId,
    value_type => AsynchronousRequestResult,
    value_thrift_type => ThriftAsynchronousRequestResult,
    context_type => AsynchronousRequestResultIdContext,
}

fn render_target(target: &thrift::MegarepoTarget) -> String {
    format!(
        "{}: {}, bookmark: {}",
        target
            .repo
            .as_ref()
            .map_or_else(|| "repo_id".to_string(), |_| "repo_name".to_string(),),
        target.repo.as_ref().map_or_else(
            || target.repo_id.unwrap_or(0).to_string(),
            |repo| repo.name.clone()
        ),
        target.bookmark
    )
}

impl AsynchronousRequestParams {
    pub fn target(&self) -> Result<String, AsyncRequestsError> {
        match &self.thrift {
            ThriftAsynchronousRequestParams::megarepo_add_target_params(params) => {
                Ok(params.target())
            }
            ThriftAsynchronousRequestParams::megarepo_add_branching_target_params(params) => {
                Ok(params.target())
            }
            ThriftAsynchronousRequestParams::megarepo_change_target_params(params) => {
                Ok(params.target())
            }
            ThriftAsynchronousRequestParams::megarepo_remerge_source_params(params) => {
                Ok(params.target())
            }
            ThriftAsynchronousRequestParams::megarepo_sync_changeset_params(params) => {
                Ok(params.target())
            }
            ThriftAsynchronousRequestParams::async_ping_params(params) => Ok(params.target()),
            ThriftAsynchronousRequestParams::commit_sparse_profile_size_params(params) => {
                Ok(params.target())
            }
            ThriftAsynchronousRequestParams::commit_sparse_profile_delta_params(params) => {
                Ok(params.target())
            }
            ThriftAsynchronousRequestParams::UnknownField(union_tag) => {
                Err(AsyncRequestsError::internal(anyhow!(
                    "this type of request (AsynchronousRequestParams tag {}) not supported by this worker!",
                    union_tag
                )))
            }
        }
    }
}

/// Convert an item into a thrift type we use for storing configuration
pub trait IntoConfigFormat<T, R> {
    fn into_config_format(self, mononoke: &Mononoke<R>) -> Result<T, AsyncRequestsError>;
}

impl<R: MononokeRepo> IntoConfigFormat<Target, R> for thrift::MegarepoTarget {
    fn into_config_format(self, mononoke: &Mononoke<R>) -> Result<Target, AsyncRequestsError> {
        let repo_id = match (self.repo, self.repo_id) {
            (Some(repo), _) => mononoke
                .repo_id_from_name(repo.name.clone())
                .ok_or_else(|| anyhow!("Invalid repo_name {}", repo.name))?
                .id() as i64,
            (_, Some(repo_id)) => repo_id,
            (None, None) => Err(anyhow!("both repo_id and repo_name are None!"))?,
        };

        Ok(Target {
            repo_id,
            bookmark: self.bookmark,
        })
    }
}

impl<R: MononokeRepo> IntoConfigFormat<SyncTargetConfig, R> for thrift::MegarepoSyncTargetConfig {
    fn into_config_format(
        self,
        mononoke: &Mononoke<R>,
    ) -> Result<SyncTargetConfig, AsyncRequestsError> {
        Ok(SyncTargetConfig {
            target: self.target.into_config_format(mononoke)?,
            sources: self.sources,
            version: self.version,
        })
    }
}

/// Convert an item into a thrift type we use in APIs
pub trait IntoApiFormat<T, R> {
    fn into_api_format(self, mononoke: &Mononoke<R>) -> Result<T, AsyncRequestsError>;
}

#[async_trait]
impl<R: MononokeRepo> IntoApiFormat<thrift::MegarepoTarget, R> for Target {
    fn into_api_format(
        self,
        mononoke: &Mononoke<R>,
    ) -> Result<thrift::MegarepoTarget, AsyncRequestsError> {
        let repo = mononoke
            .repo_name_from_id(RepositoryId::new(self.repo_id as i32))
            .map(|name| thrift::RepoSpecifier {
                name,
                ..Default::default()
            });
        Ok(thrift::MegarepoTarget {
            repo_id: Some(self.repo_id),
            bookmark: self.bookmark,
            repo,
            ..Default::default()
        })
    }
}

#[async_trait]
impl<R: MononokeRepo> IntoApiFormat<thrift::MegarepoSyncTargetConfig, R> for SyncTargetConfig {
    fn into_api_format(
        self,
        mononoke: &Mononoke<R>,
    ) -> Result<thrift::MegarepoSyncTargetConfig, AsyncRequestsError> {
        Ok(thrift::MegarepoSyncTargetConfig {
            target: self.target.into_api_format(mononoke)?,
            sources: self.sources,
            version: self.version,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod test {
    use blobstore::Loadable;
    use blobstore::PutBehaviour;
    use blobstore::Storable;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use memblob::Memblob;
    use mononoke_macros::mononoke;

    use super::*;

    macro_rules! test_blobstore_key {
        {
            $type: ident,
            $prefix: expr
        } => {
            let id = $type::from_byte_array([1; 32]);
            assert_eq!(id.blobstore_key(), format!(concat!($prefix, ".blake2.{}"), id));
        }
    }

    macro_rules! serialize_deserialize {
        {
            $type: ident
        } => {
            let id = $type::from_byte_array([1; 32]);
            let serialized = serde_json::to_string(&id).unwrap();
            let deserialized = serde_json::from_str(&serialized).unwrap();
            assert_eq!(id, deserialized);
        }
    }

    #[mononoke::test]
    fn blobstore_key() {
        // These IDs are persistent, and this test is really to make sure that they don't change
        // accidentally. Same as in typed_hash.rs
        test_blobstore_key!(
            AsynchronousRequestParamsId,
            "async.svc.AsynchronousRequestParams"
        );
        test_blobstore_key!(
            AsynchronousRequestResultId,
            "async.svc.AsynchronousRequestResult"
        );
    }

    #[mononoke::test]
    fn test_serialize_deserialize() {
        serialize_deserialize!(AsynchronousRequestParamsId);
        serialize_deserialize!(AsynchronousRequestResultId);
    }

    macro_rules! test_store_load {
        { $type: ident, $ctx: ident, $blobstore: ident } => {
            let obj = $type::from_thrift(Default::default());

            let id = obj
                .clone()
                .store(&$ctx, &$blobstore)
                .await
                .expect(&format!("Failed to store {}", stringify!($type)));

            let obj2 = id
                .load(&$ctx, &$blobstore)
                .await
                .expect(&format!("Failed to load {}", stringify!($type)));

            assert_eq!(obj, obj2);
        }
    }

    #[mononoke::fbinit_test]
    async fn test_megaerpo_add_target_params_type(fb: FacebookInit) {
        let blobstore = Memblob::new(PutBehaviour::IfAbsent);
        let ctx = CoreContext::test_mock(fb);
        test_store_load!(AsynchronousRequestParams, ctx, blobstore);
        test_store_load!(AsynchronousRequestResult, ctx, blobstore);
    }
}
