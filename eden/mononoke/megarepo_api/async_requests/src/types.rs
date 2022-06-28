/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use blobstore::impl_loadable_storable;
use blobstore::Blobstore;
use context::CoreContext;
use fbthrift::compact_protocol;
pub use megarepo_config::Target;
use megarepo_error::MegarepoError;
pub use megarepo_types_thrift::MegarepoAsynchronousRequestParams as ThriftMegarepoAsynchronousRequestParams;
pub use megarepo_types_thrift::MegarepoAsynchronousRequestParamsId as ThriftMegarepoAsynchronousRequestParamsId;
pub use megarepo_types_thrift::MegarepoAsynchronousRequestResult as ThriftMegarepoAsynchronousRequestResult;
pub use megarepo_types_thrift::MegarepoAsynchronousRequestResultId as ThriftMegarepoAsynchronousRequestResultId;
use mononoke_types::hash::Blake2;
use mononoke_types::impl_typed_context;
use mononoke_types::impl_typed_hash_no_context;
use mononoke_types::BlobstoreKey;
pub use requests_table::RequestStatus;
pub use requests_table::RequestType;
pub use requests_table::RowId;
use source_control::MegarepoAddBranchingTargetParams as ThriftMegarepoAddBranchingTargetParams;
use source_control::MegarepoAddBranchingTargetPollResponse as ThriftMegarepoAddBranchingTargetPollResponse;
use source_control::MegarepoAddBranchingTargetResponse as ThriftMegarepoAddBranchingTargetResponse;
use source_control::MegarepoAddBranchingTargetResult as ThriftMegarepoAddBranchingTargetResult;
use source_control::MegarepoAddBranchingTargetToken as ThriftMegarepoAddBranchingTargetToken;
use source_control::MegarepoAddTargetParams as ThriftMegarepoAddTargetParams;
use source_control::MegarepoAddTargetPollResponse as ThriftMegarepoAddTargetPollResponse;
use source_control::MegarepoAddTargetResponse as ThriftMegarepoAddTargetResponse;
use source_control::MegarepoAddTargetResult as ThriftMegarepoAddTargetResult;
use source_control::MegarepoAddTargetToken as ThriftMegarepoAddTargetToken;
use source_control::MegarepoChangeConfigToken as ThriftMegarepoChangeConfigToken;
use source_control::MegarepoChangeTargetConfigParams as ThriftMegarepoChangeTargetConfigParams;
use source_control::MegarepoChangeTargetConfigPollResponse as ThriftMegarepoChangeTargetConfigPollResponse;
use source_control::MegarepoChangeTargetConfigResponse as ThriftMegarepoChangeTargetConfigResponse;
use source_control::MegarepoChangeTargetConfigResult as ThriftMegarepoChangeTargetConfigResult;
use source_control::MegarepoRemergeSourceParams as ThriftMegarepoRemergeSourceParams;
use source_control::MegarepoRemergeSourcePollResponse as ThriftMegarepoRemergeSourcePollResponse;
use source_control::MegarepoRemergeSourceResponse as ThriftMegarepoRemergeSourceResponse;
use source_control::MegarepoRemergeSourceResult as ThriftMegarepoRemergeSourceResult;
use source_control::MegarepoRemergeSourceToken as ThriftMegarepoRemergeSourceToken;
use source_control::MegarepoSyncChangesetParams as ThriftMegarepoSyncChangesetParams;
use source_control::MegarepoSyncChangesetPollResponse as ThriftMegarepoSyncChangesetPollResponse;
use source_control::MegarepoSyncChangesetResponse as ThriftMegarepoSyncChangesetResponse;
use source_control::MegarepoSyncChangesetResult as ThriftMegarepoSyncChangesetResult;
use source_control::MegarepoSyncChangesetToken as ThriftMegarepoSyncChangesetToken;
use std::str::FromStr;
use std::sync::Arc;

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
pub trait ThriftParams: Sized + Send + Sync + Into<MegarepoAsynchronousRequestParams> {
    type R: Request<ThriftParams = Self>;

    /// Every *Params argument referes to some Target
    /// This method is needed to extract it from the
    /// implementor of this trait
    fn target(&self) -> &Target;
}
pub trait ThriftResult:
    Sized + Send + Sync + TryFrom<MegarepoAsynchronousRequestResult, Error = MegarepoError>
{
    type R: Request<ThriftResult = Self>;
}

/// Polling token for an async service method
pub trait Token: Clone + Sized + Send + Sync {
    type R: Request<Token = Self>;
    type ThriftToken;

    fn into_thrift(self) -> Self::ThriftToken;
    fn from_db_id_and_target(id: RowId, target: Target) -> Self;
    fn to_db_id_and_target(&self) -> Result<(RowId, Target), MegarepoError>;

    /// Every Token referes to some Target
    /// This method is needed to extract it from the
    /// implementor of this trait
    fn target(&self) -> &Target;
}

/// This macro implements an async service method type,
/// which can be stored/retrieved from the blobstore.
/// Such types are usually represented as value/handle pairs.
/// Since we need to implement (potentially foreign) traits
/// on these types, we also define corrensponding Rust types
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

            pub async fn load_from_key(ctx: &CoreContext, blobstore: &Arc<dyn Blobstore>, key: &str) -> Result<Self, MegarepoError> {
                let bytes = blobstore.get(ctx, key).await?;

                let prefix = concat!("async.svc.", stringify!($value_type), ".blake2.");
                if key.strip_prefix(prefix).is_none() {
                    return Err(MegarepoError::internal(anyhow!("{} is not a blobstore key for {}", key, stringify!($value_type))));
                }

                match bytes {
                    Some(bytes) => Ok(bytes.into_bytes().try_into()?),
                    None => Err(MegarepoError::internal(anyhow!("Missing blob: {}", key))),
                }
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
                Self(mononoke_types_thrift::IdType::Blake2(other.0.into_thrift()))
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

/// A macro to call impl_async_svc_stored_type for params/result
/// types, as well as define a bunch of relationships between
/// these types, and their Request-related frients.
/// An underlying idea is to define as much behavior and relationships
/// as possible in the type system, so that we
/// (a) minimize a chance of using incorrent pair of types somewhere
/// (b) can write generic enqueing/polling functions
macro_rules! impl_async_svc_method_types {
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

        fn target(&$self_ident: ident: ThriftParams) -> &Target $target_in_params: tt

    } => {
        impl ThriftParams for $params_value_thrift_type {
            type R = $request_struct;

            fn target(&$self_ident) -> &Target {
                $target_in_params
            }
        }

        #[derive(Clone)]
        pub struct $token_type(pub $token_thrift_type);

        impl Token for $token_type {
            type ThriftToken = $token_thrift_type;
            type R = $request_struct;

            fn from_db_id_and_target(id: RowId, target: Target) -> Self {
                // Thrift token is a string alias
                // but's guard ourselves here against
                // it changing unexpectedly.
                let thrift_token = $token_thrift_type {
                    target,
                    id: id.0 as i64,
                    ..Default::default()
                };
                Self(thrift_token)
            }

            fn to_db_id_and_target(&self) -> Result<(RowId, Target), MegarepoError> {
                let row_id = self.0.id as u64;
                let row_id = RowId(row_id);
                let target = self.0.target.clone();

                Ok((row_id, target))
            }

            fn into_thrift(self) -> $token_thrift_type {
                self.0
            }

            fn target(&self) -> &Target {
                &self.0.target
            }
        }

        impl From<Result<$response_type, MegarepoError>> for MegarepoAsynchronousRequestResult {
            fn from(r: Result<$response_type, MegarepoError>) -> MegarepoAsynchronousRequestResult {
                let thrift = match r {
                    Ok(payload) => ThriftMegarepoAsynchronousRequestResult::$result_union_variant($result_value_thrift_type::success(payload)),
                    Err(e) => ThriftMegarepoAsynchronousRequestResult::$result_union_variant($result_value_thrift_type::error(e.into()))
                };

                MegarepoAsynchronousRequestResult::from_thrift(thrift)
            }
        }

        impl From<$result_value_thrift_type> for MegarepoAsynchronousRequestResult {
            fn from(r: $result_value_thrift_type) -> MegarepoAsynchronousRequestResult {
                let thrift = ThriftMegarepoAsynchronousRequestResult::$result_union_variant(r);
                MegarepoAsynchronousRequestResult::from_thrift(thrift)
            }
        }

        impl From<$params_value_thrift_type> for MegarepoAsynchronousRequestParams{
            fn from(params: $params_value_thrift_type) -> MegarepoAsynchronousRequestParams {
                MegarepoAsynchronousRequestParams::from_thrift(
                    ThriftMegarepoAsynchronousRequestParams::$params_union_variant(params)
                )
            }
        }

        impl ThriftResult for $result_value_thrift_type {
            type R = $request_struct;
        }

        impl TryFrom<MegarepoAsynchronousRequestResult> for $result_value_thrift_type {
            type Error = MegarepoError;

            fn try_from(r: MegarepoAsynchronousRequestResult) -> Result<$result_value_thrift_type, Self::Error> {
                match r.thrift {
                    ThriftMegarepoAsynchronousRequestResult::$result_union_variant(payload) => Ok(payload),
                    ThriftMegarepoAsynchronousRequestResult::UnknownField(x) => {
                        // TODO: maybe use structured error?
                        Err(MegarepoError::internal(
                            anyhow!(
                                "failed to parse {} thrift. UnknownField: {}",
                                stringify!($result_value_thrift_type),
                                x,
                            )
                        ))
                    },
                    x => {
                        Err(MegarepoError::internal(
                            anyhow!(
                                "failed to parse {} thrift. The result union contains the wrong result variant: {:?}",
                                stringify!($result_value_thrift_type),
                                x,
                            )
                        ))
                    }
                }
            }
        }

        pub struct $request_struct;

        impl Request for $request_struct {
            const NAME: &'static str = $method_name;

            type Token = $token_type;
            type ThriftParams = $params_value_thrift_type;
            type ThriftResult = $result_value_thrift_type;
            type ThriftResponse = $response_type;

            type PollResponse = $poll_response_type;

            fn thrift_result_into_poll_response(
                thrift_result: Self::ThriftResult,
            ) -> Self::PollResponse {
                $poll_response_type { result: Some(thrift_result), ..Default::default() }
            }

            fn empty_poll_response() -> Self::PollResponse {
                $poll_response_type { result: None, ..Default::default() }
            }
        }

    }
}

// Params and result types for megarepo_add_sync_target

impl_async_svc_method_types! {
    method_name => "megarepo_add_sync_target",
    request_struct => MegarepoAddSyncTarget,

    params_value_thrift_type => ThriftMegarepoAddTargetParams,
    params_union_variant => megarepo_add_target_params,

    result_value_thrift_type => ThriftMegarepoAddTargetResult,
    result_union_variant => megarepo_add_target_result,

    response_type => ThriftMegarepoAddTargetResponse,
    poll_response_type => ThriftMegarepoAddTargetPollResponse,
    token_type => MegarepoAddTargetToken,
    token_thrift_type => ThriftMegarepoAddTargetToken,

    fn target(&self: ThriftParams) -> &Target {
        &self.config_with_new_target.target
    }
}

// Params and result types for megarepo_add_branching_sync_target

impl_async_svc_method_types! {
    method_name => "megarepo_add_branching_sync_target",
    request_struct => MegarepoAddBranchingSyncTarget,

    params_value_thrift_type => ThriftMegarepoAddBranchingTargetParams,
    params_union_variant => megarepo_add_branching_target_params,

    result_value_thrift_type => ThriftMegarepoAddBranchingTargetResult,
    result_union_variant => megarepo_add_branching_target_result,

    response_type => ThriftMegarepoAddBranchingTargetResponse,
    poll_response_type => ThriftMegarepoAddBranchingTargetPollResponse,
    token_type => MegarepoAddBranchingTargetToken,
    token_thrift_type => ThriftMegarepoAddBranchingTargetToken,

    fn target(&self: ThriftParams) -> &Target {
        &self.target
    }
}

// Params and result types for megarepo_change_target_config

impl_async_svc_method_types! {
    method_name => "megarepo_change_target_config",
    request_struct => MegarepoChangeTargetConfig,

    params_value_thrift_type => ThriftMegarepoChangeTargetConfigParams,
    params_union_variant => megarepo_change_target_params,

    result_value_thrift_type => ThriftMegarepoChangeTargetConfigResult,
    result_union_variant => megarepo_change_target_result,

    response_type => ThriftMegarepoChangeTargetConfigResponse,
    poll_response_type => ThriftMegarepoChangeTargetConfigPollResponse,
    token_type => MegarepoChangeTargetConfigToken,
    token_thrift_type => ThriftMegarepoChangeConfigToken,

    fn target(&self: ThriftParams) -> &Target {
        &self.target
    }
}

// Params and result types for megarepo_sync_changeset

impl_async_svc_method_types! {
    method_name => "megarepo_sync_changeset",
    request_struct => MegarepoSyncChangeset,

    params_value_thrift_type => ThriftMegarepoSyncChangesetParams,
    params_union_variant => megarepo_sync_changeset_params,

    result_value_thrift_type => ThriftMegarepoSyncChangesetResult,
    result_union_variant => megarepo_sync_changeset_result,

    response_type => ThriftMegarepoSyncChangesetResponse,
    poll_response_type => ThriftMegarepoSyncChangesetPollResponse,
    token_type => MegarepoSyncChangesetToken,
    token_thrift_type => ThriftMegarepoSyncChangesetToken,

    fn target(&self: ThriftParams) -> &Target {
        &self.target
    }
}

// Params and result types for megarepo_remerge_source

impl_async_svc_method_types! {
    method_name => "megarepo_remerge_source",
    request_struct => MegarepoRemergeSource,

    params_value_thrift_type => ThriftMegarepoRemergeSourceParams,
    params_union_variant => megarepo_remerge_source_params,

    result_value_thrift_type => ThriftMegarepoRemergeSourceResult,
    result_union_variant => megarepo_remerge_source_result,

    response_type => ThriftMegarepoRemergeSourceResponse,
    poll_response_type => ThriftMegarepoRemergeSourcePollResponse,
    token_type => MegarepoRemergeSourceToken,
    token_thrift_type => ThriftMegarepoRemergeSourceToken,

    fn target(&self: ThriftParams) -> &Target {
        &self.target
    }
}

impl_async_svc_stored_type! {
    handle_type => MegarepoAsynchronousRequestParamsId,
    handle_thrift_type => ThriftMegarepoAsynchronousRequestParamsId,
    value_type => MegarepoAsynchronousRequestParams,
    value_thrift_type => ThriftMegarepoAsynchronousRequestParams,
    context_type => MegarepoAsynchronousRequestParamsIdContext,
}

impl_async_svc_stored_type! {
    handle_type => MegarepoAsynchronousRequestResultId,
    handle_thrift_type => ThriftMegarepoAsynchronousRequestResultId,
    value_type => MegarepoAsynchronousRequestResult,
    value_thrift_type => ThriftMegarepoAsynchronousRequestResult,
    context_type => MegarepoAsynchronousRequestResultIdContext,
}

impl MegarepoAsynchronousRequestParams {
    pub fn target(&self) -> Result<&Target, MegarepoError> {
        match &self.thrift {
            ThriftMegarepoAsynchronousRequestParams::megarepo_add_target_params(params) => {
                Ok(params.target())
            }
            ThriftMegarepoAsynchronousRequestParams::megarepo_add_branching_target_params(
                params,
            ) => Ok(params.target()),
            ThriftMegarepoAsynchronousRequestParams::megarepo_change_target_params(params) => {
                Ok(params.target())
            }
            ThriftMegarepoAsynchronousRequestParams::megarepo_remerge_source_params(params) => {
                Ok(params.target())
            }
            ThriftMegarepoAsynchronousRequestParams::megarepo_sync_changeset_params(params) => {
                Ok(params.target())
            }
            ThriftMegarepoAsynchronousRequestParams::UnknownField(union_tag) => {
                Err(MegarepoError::internal(anyhow!(
                    "this type of reuqest (MegarepoAsynchronousRequestParams tag {}) not supported by this worker!",
                    union_tag
                )))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use blobstore::Loadable;
    use blobstore::PutBehaviour;
    use blobstore::Storable;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use memblob::Memblob;

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

    #[test]
    fn blobstore_key() {
        // These IDs are persistent, and this test is really to make sure that they don't change
        // accidentally. Same as in typed_hash.rs
        test_blobstore_key!(
            MegarepoAsynchronousRequestParamsId,
            "async.svc.MegarepoAsynchronousRequestParams"
        );
        test_blobstore_key!(
            MegarepoAsynchronousRequestResultId,
            "async.svc.MegarepoAsynchronousRequestResult"
        );
        test_blobstore_key!(
            MegarepoAsynchronousRequestParamsId,
            "async.svc.MegarepoAsynchronousRequestParams"
        );
        test_blobstore_key!(
            MegarepoAsynchronousRequestResultId,
            "async.svc.MegarepoAsynchronousRequestResult"
        );
    }

    #[test]
    fn test_serialize_deserialize() {
        serialize_deserialize!(MegarepoAsynchronousRequestParamsId);
        serialize_deserialize!(MegarepoAsynchronousRequestResultId);
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

    #[fbinit::test]
    async fn test_megaerpo_add_target_params_type(fb: FacebookInit) {
        let blobstore = Memblob::new(PutBehaviour::IfAbsent);
        let ctx = CoreContext::test_mock(fb);
        test_store_load!(MegarepoAsynchronousRequestParams, ctx, blobstore);
        test_store_load!(MegarepoAsynchronousRequestResult, ctx, blobstore);
    }
}
