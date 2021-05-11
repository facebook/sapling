/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error, Result};
use blobstore::{impl_loadable_storable, Loadable, Storable};
use fbthrift::compact_protocol;
use megarepo_config::Target;
use megarepo_error::MegarepoError;
use megarepo_types_thrift::{
    MegarepoAddTargetParamsId as ThriftMegarepoAddTargetParamsId,
    MegarepoChangeTargetConfigParamsId as ThriftMegarepoChangeTargetConfigParamsId,
    MegarepoRemergeSourceParamsId as ThriftMegarepoRemergeSourceParamsId,
    MegarepoSyncChangesetParamsId as ThriftMegarepoSyncChangesetParamsId,
};
use megarepo_types_thrift::{
    MegarepoAddTargetResult as ThriftMegarepoAddTargetResult,
    MegarepoChangeTargetConfigResult as ThriftMegarepoChangeTargetConfigResult,
    MegarepoRemergeSourceResult as ThriftMegarepoRemergeSourceResult,
    MegarepoSyncChangesetResult as ThriftMegarepoSyncChangesetResult,
};
use megarepo_types_thrift::{
    MegarepoAddTargetResultId as ThriftMegarepoAddTargetResultId,
    MegarepoChangeTargetConfigResultId as ThriftMegarepoChangeTargetConfigResultId,
    MegarepoRemergeSourceResultId as ThriftMegarepoRemergeSourceResultId,
    MegarepoSyncChangesetResultId as ThriftMegarepoSyncChangesetResultId,
};
use mononoke_types::{hash::Blake2, impl_typed_context, impl_typed_hash_no_context};
use requests_table::RowId;
use source_control::{
    MegarepoAddTargetParams as ThriftMegarepoAddTargetParams,
    MegarepoChangeTargetConfigParams as ThriftMegarepoChangeTargetConfigParams,
    MegarepoRemergeSourceParams as ThriftMegarepoRemergeSourceParams,
    MegarepoSyncChangesetParams as ThriftMegarepoSyncChangesetParams,
};
use source_control::{
    MegarepoAddTargetPollResponse as ThriftMegarepoAddTargetPollResponse,
    MegarepoChangeTargetConfigPollResponse as ThriftMegarepoChangeTargetConfigPollResponse,
    MegarepoRemergeSourcePollResponse as ThriftMegarepoRemergeSourcePollResponse,
    MegarepoSyncChangesetPollResponse as ThriftMegarepoSyncChangesetPollResponse,
};
use source_control::{
    MegarepoAddTargetResponse as ThriftMegarepoAddTargetResponse,
    MegarepoChangeTargetConfigResponse as ThriftMegarepoChangeTargetConfigResponse,
    MegarepoRemergeSourceResponse as ThriftMegarepoRemergeSourceResponse,
    MegarepoSyncChangesetResponse as ThriftMegarepoSyncChangesetResponse,
};
use source_control::{
    MegarepoAddTargetToken as ThriftMegarepoAddTargetToken,
    MegarepoChangeConfigToken as ThriftMegarepoChangeConfigToken,
    MegarepoRemergeSourceToken as ThriftMegarepoRemergeSourceToken,
    MegarepoSyncChangesetToken as ThriftMegarepoSyncChangesetToken,
};
use std::convert::TryFrom;
use std::str::FromStr;

/// Grouping of types and behaviors for an asynchronous request
pub trait Request: Sized + Send + Sync {
    /// Name of the request
    const NAME: &'static str;
    /// Id type for request stored result
    type StoredResultId: FromStr<Err = Error>
        + Loadable<Value = Self::StoredResult>
        + BlobstoreKeyWrapper;
    /// Id type for request params
    type ParamsId: FromStr<Err = Error> + Loadable<Value = Self::Params> + BlobstoreKeyWrapper;
    /// Rust newtype for a polling token
    type Token: Token;
    /// Rust type for request params
    type Params: Storable<Key = Self::ParamsId> + TryFrom<Self::ThriftParams, Error = Error>;

    /// Underlying thrift type for request params
    type ThriftParams: ThriftParams<R = Self>;

    /// Rust type for request result (response or error),
    /// stored in a blobstore
    type StoredResult: Storable<Key = Self::StoredResultId>;
    /// A type representing potentially present response
    type PollResponse;

    /// Convert stored result into a result of a poll response
    /// Stored result is a serialization of either a successful
    /// respone, or an error. Poll response cannot convey an error,
    /// so we use result of a poll response to do so.
    /// Note that this function should return either a
    /// non-empty poll-response, or an error
    fn stored_result_into_poll_response(
        sr: Self::StoredResult,
    ) -> Result<Self::PollResponse, MegarepoError>;

    /// Return an empty poll response. This indicates
    /// that the request hasn't been processed yet
    fn empty_poll_response() -> Self::PollResponse;
}

/// A type, which can be parsed from a blobstore key,
/// and from which a blobstore key can be produced
/// (this is implemented by various handle types, where
/// blobstore key consists of two things: a hash
/// and a string, describing what the key refers to)
pub trait BlobstoreKeyWrapper: FromStr<Err = Error> {
    fn blobstore_key(&self) -> String;
    fn parse_blobstore_key(key: &str) -> Result<Self, Error>;
}

/// Thrift type representing async service method parameters
pub trait ThriftParams: Sized + Send + Sync {
    type R: Request<ThriftParams = Self>;

    /// Every *Params argument referes to some Target
    /// This method is needed to extract it from the
    /// implementor of this trait
    fn target(&self) -> &Target;
}

/// Polling token for an async service method
pub trait Token: Sized + Send + Sync {
    type R: Request<Token = Self>;
    type ThriftToken;

    fn into_thrift(self) -> Self::ThriftToken;
    fn from_db_id(id: RowId) -> Self;
    fn to_db_id(&self) -> Result<RowId, MegarepoError>;
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
        }

        impl BlobstoreKeyWrapper for $handle_type {
            fn blobstore_key(&self) -> String {
                format!("async.svc.{}.blake2.{}", stringify!($value_type), self.0.to_hex())
            }

            fn parse_blobstore_key(key: &str) -> Result<Self, Error> {
                // concat! instead of format! to not allocate every time
                let prefix = concat!("async.svc.", stringify!($value_type), ".blake2.");
                match key.strip_prefix(prefix) {
                    None => return Err(anyhow!("{} is not a blobstore key for {}", key, stringify!($value_type))),
                    Some(suffix) => Self::from_str(suffix)
                }
            }
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

            pub fn handle(&self) -> &$handle_type {
                &self.id
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

        params_handle_type => $params_handle_type: ident,
        params_handle_thrift_type => $params_handle_thrift_type: ident,
        params_value_type => $params_value_type: ident,
        params_value_thrift_type => $params_value_thrift_type: ident,
        params_context_type => $params_context_type: ident,

        result_handle_type => $result_handle_type: ident,
        result_handle_thrift_type => $result_handle_thrift_type: ident,
        result_value_type => $result_value_type: ident,
        result_value_thrift_type => $result_value_thrift_type: ident,
        result_context_type => $result_context_type: ident,

        response_type => $response_type: ident,
        poll_response_type => $poll_response_type: ident,
        token_type => $token_type: ident,
        token_thrift_type => $token_thrift_type: ident,

        fn target(&$self_ident: ident: ThriftParams) -> &Target $target_in_params: tt

    } => {
        impl_async_svc_stored_type! {
            handle_type => $params_handle_type,
            handle_thrift_type => $params_handle_thrift_type,
            value_type => $params_value_type,
            value_thrift_type => $params_value_thrift_type,
            context_type => $params_context_type,
        }

        impl_async_svc_stored_type! {
            handle_type => $result_handle_type,
            handle_thrift_type => $result_handle_thrift_type,
            value_type => $result_value_type,
            value_thrift_type => $result_value_thrift_type,
            context_type => $result_context_type,
        }

        pub struct $token_type(pub $token_thrift_type);

        impl ThriftParams for $params_value_thrift_type {
            type R = $request_struct;

            fn target(&$self_ident) -> &Target {
                $target_in_params
            }
        }

        impl Token for $token_type {
            type ThriftToken = $token_thrift_type;
            type R = $request_struct;

            fn from_db_id(id: RowId) -> Self {
                // Thrift token is a string alias
                // but's guard ourselves here against
                // it changing unexpectedly.
                let thrift_token: $token_thrift_type = format!("{}", id.0);
                Self(thrift_token)
            }

            fn to_db_id(&self) -> Result<RowId, MegarepoError> {
                self.0
                    .parse::<u64>()
                    .map_err(MegarepoError::request)
                    .map(RowId)
            }

            fn into_thrift(self) -> $token_thrift_type {
                self.0
            }
        }

        impl From<Result<$response_type, MegarepoError>> for $result_value_type {
            fn from(r: Result<$response_type, MegarepoError>) -> $result_value_type {
                let thrift = match r {
                    Ok(payload) => $result_value_thrift_type::success(payload),
                    Err(e) => $result_value_thrift_type::error(e.into())
                };

                $result_value_type::from_thrift(thrift)
            }
        }

        impl From<$result_value_type> for Result<$response_type, MegarepoError> {
            fn from(r: $result_value_type) -> Result<$response_type, MegarepoError> {
                match r.thrift {
                    $result_value_thrift_type::success(payload) => Ok(payload),
                    $result_value_thrift_type::error(e) => Err(e.into()),
                    $result_value_thrift_type::UnknownField(x) => {
                        // TODO: maybe use structured error?
                        Err(MegarepoError::internal(
                            anyhow!(
                                "failed to parse {} thrift. UnknownField: {}",
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

            type StoredResultId = $result_handle_type;
            type ParamsId = $params_handle_type;
            type Token = $token_type;
            type ThriftParams = $params_value_thrift_type;
            type Params = $params_value_type;
            type StoredResult = $result_value_type;
            type PollResponse = $poll_response_type;

            fn stored_result_into_poll_response(
                stored_result: Self::StoredResult,
            ) -> Result<Self::PollResponse, MegarepoError> {
                let r: Result<$response_type, MegarepoError> = stored_result.into();
                r.map(|r| $poll_response_type { response: Some(r) })
            }

            fn empty_poll_response() -> Self::PollResponse {
                $poll_response_type { response: None }
            }
        }

    }
}

// Params and result types for megarepo_add_sync_target

impl_async_svc_method_types! {
    method_name => "megarepo_add_sync_target",
    request_struct => MegarepoAddSyncTarget,

    params_handle_type => MegarepoAddTargetParamsId,
    params_handle_thrift_type => ThriftMegarepoAddTargetParamsId,
    params_value_type => MegarepoAddTargetParams,
    params_value_thrift_type => ThriftMegarepoAddTargetParams,
    params_context_type => MegarepoAddTargetParamsIdContext,

    result_handle_type => MegarepoAddTargetResultId,
    result_handle_thrift_type => ThriftMegarepoAddTargetResultId,
    result_value_type => MegarepoAddTargetResult,
    result_value_thrift_type => ThriftMegarepoAddTargetResult,
    result_context_type => MegarepoAddTargetResultIdContext,

    response_type => ThriftMegarepoAddTargetResponse,
    poll_response_type => ThriftMegarepoAddTargetPollResponse,
    token_type => MegarepoAddTargetToken,
    token_thrift_type => ThriftMegarepoAddTargetToken,

    fn target(&self: ThriftParams) -> &Target {
        &self.config_with_new_target.target
    }
}

// Params and result types for megarepo_change_target_config

impl_async_svc_method_types! {
    method_name => "megarepo_change_target_config",
    request_struct => MegarepoChangeTargetConfig,

    params_handle_type => MegarepoChangeTargetConfigParamsId,
    params_handle_thrift_type => ThriftMegarepoChangeTargetConfigParamsId,
    params_value_type => MegarepoChangeTargetConfigParams,
    params_value_thrift_type => ThriftMegarepoChangeTargetConfigParams,
    params_context_type => MegarepoChangeTargetConfigParamsIdContext,

    result_handle_type => MegarepoChangeTargetConfigResultId,
    result_handle_thrift_type => ThriftMegarepoChangeTargetConfigResultId,
    result_value_type => MegarepoChangeTargetConfigResult,
    result_value_thrift_type => ThriftMegarepoChangeTargetConfigResult,
    result_context_type => MegarepoChangeTargetConfigResultIdContext,

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

    params_handle_type => MegarepoSyncChangesetParamsId,
    params_handle_thrift_type => ThriftMegarepoSyncChangesetParamsId,
    params_value_type => MegarepoSyncChangesetParams,
    params_value_thrift_type => ThriftMegarepoSyncChangesetParams,
    params_context_type => MegarepoSyncChangesetParamsIdContext,

    result_handle_type => MegarepoSyncChangesetResultId,
    result_handle_thrift_type => ThriftMegarepoSyncChangesetResultId,
    result_value_type => MegarepoSyncChangesetResult,
    result_value_thrift_type => ThriftMegarepoSyncChangesetResult,
    result_context_type => MegarepoSyncChangesetResultIdContext,

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

    params_handle_type => MegarepoRemergeSourceParamsId,
    params_handle_thrift_type => ThriftMegarepoRemergeSourceParamsId,
    params_value_type => MegarepoRemergeSourceParams,
    params_value_thrift_type => ThriftMegarepoRemergeSourceParams,
    params_context_type => MegarepoRemergeSourceParamsIdContext,

    result_handle_type => MegarepoRemergeSourceResultId,
    result_handle_thrift_type => ThriftMegarepoRemergeSourceResultId,
    result_value_type => MegarepoRemergeSourceResult,
    result_value_thrift_type => ThriftMegarepoRemergeSourceResult,
    result_context_type => MegarepoRemergeSourceResultIdContext,

    response_type => ThriftMegarepoRemergeSourceResponse,
    poll_response_type => ThriftMegarepoRemergeSourcePollResponse,
    token_type => MegarepoRemergeSourceToken,
    token_thrift_type => ThriftMegarepoRemergeSourceToken,

    fn target(&self: ThriftParams) -> &Target {
        &self.target
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use blobstore::{Loadable, PutBehaviour, Storable};
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
            MegarepoAddTargetParamsId,
            "async.svc.MegarepoAddTargetParams"
        );
        test_blobstore_key!(
            MegarepoAddTargetResultId,
            "async.svc.MegarepoAddTargetResult"
        );
        test_blobstore_key!(
            MegarepoChangeTargetConfigParamsId,
            "async.svc.MegarepoChangeTargetConfigParams"
        );
        test_blobstore_key!(
            MegarepoChangeTargetConfigResultId,
            "async.svc.MegarepoChangeTargetConfigResult"
        );
        test_blobstore_key!(
            MegarepoRemergeSourceParamsId,
            "async.svc.MegarepoRemergeSourceParams"
        );
        test_blobstore_key!(
            MegarepoRemergeSourceResultId,
            "async.svc.MegarepoRemergeSourceResult"
        );
        test_blobstore_key!(
            MegarepoSyncChangesetParamsId,
            "async.svc.MegarepoSyncChangesetParams"
        );
        test_blobstore_key!(
            MegarepoSyncChangesetResultId,
            "async.svc.MegarepoSyncChangesetResult"
        );
    }

    #[test]
    fn test_serialize_deserialize() {
        serialize_deserialize!(MegarepoAddTargetParamsId);
        serialize_deserialize!(MegarepoAddTargetResultId);
        serialize_deserialize!(MegarepoChangeTargetConfigParamsId);
        serialize_deserialize!(MegarepoChangeTargetConfigResultId);
        serialize_deserialize!(MegarepoRemergeSourceParamsId);
        serialize_deserialize!(MegarepoRemergeSourceResultId);
        serialize_deserialize!(MegarepoSyncChangesetParamsId);
        serialize_deserialize!(MegarepoSyncChangesetResultId);
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
        test_store_load!(MegarepoAddTargetParams, ctx, blobstore);
        test_store_load!(MegarepoAddTargetResult, ctx, blobstore);
        test_store_load!(MegarepoChangeTargetConfigParams, ctx, blobstore);
        test_store_load!(MegarepoChangeTargetConfigResult, ctx, blobstore);
        test_store_load!(MegarepoRemergeSourceParams, ctx, blobstore);
        test_store_load!(MegarepoRemergeSourceResult, ctx, blobstore);
        test_store_load!(MegarepoSyncChangesetParams, ctx, blobstore);
        test_store_load!(MegarepoSyncChangesetResult, ctx, blobstore);
    }
}
