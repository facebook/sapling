/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */


//! Thrift types for megarepo async service methods
//! These aren't part of `source_control.trift`, because they aren't part of
//! the service interface. They aren't part of of `megarepo_configs.thrift`,
//! because they aren't configs. Rather, they are service implementation detail.

include "eden/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"
include "eden/mononoke/scs/if/source_control.thrift"


// Id types for async service methods params and responses.
// Param and response types themselves are defined in the source_control.trift
typedef mononoke_types_thrift.IdType MegarepoAddTargetParamsId (rust.newtype)
typedef mononoke_types_thrift.IdType MegarepoChangeTargetConfigParamsId (rust.newtype)
typedef mononoke_types_thrift.IdType MegarepoRemergeSourceParamsId (rust.newtype)
typedef mononoke_types_thrift.IdType MegarepoSyncChangesetParamsId (rust.newtype)

typedef mononoke_types_thrift.IdType MegarepoSyncChangesetResultId (rust.newtype)
typedef mononoke_types_thrift.IdType MegarepoAddTargetResultId (rust.newtype)
typedef mononoke_types_thrift.IdType MegarepoRemergeSourceResultId (rust.newtype)
typedef mononoke_types_thrift.IdType MegarepoChangeTargetConfigResultId (rust.newtype)

struct RequestErrorStruct {
    1: source_control.RequestErrorKind kind,
    2: string reason,
}

struct InternalErrorStruct {
    1: string reason,
    2: optional string backtrace,
    3: list<string> source_chain,
}

// Stored error for asynchronous service methods
union StoredError {
    1: RequestErrorStruct request_error,
    2: InternalErrorStruct internal_error,
}

// Stored result for `source_control.megarepo_add_sync_target` calls
// These stored results are used to preserve the result of an asynchronous
// computation, so that they can be polled afterwards
union MegarepoAddTargetResult {
    1: source_control.MegarepoAddTargetResponse success,
    2: StoredError error,
}

// Stored result for `source_control.megarepo_change_target_config` calls
// These stored results are used to preserve the result of an asynchronous
// computation, so that they can be polled afterwards
union MegarepoChangeTargetConfigResult {
    1: source_control.MegarepoChangeTargetConfigResponse success,
    2: StoredError error,
}

// Stored result for `source_control.megarepo_remerge_source` calls
// These stored results are used to preserve the result of an asynchronous
// computation, so that they can be polled afterwards
union MegarepoRemergeSourceResult {
    1: source_control.MegarepoRemergeSourceResponse success,
    2: StoredError error,
}

// Stored result for `source_control.megarepo_sync_changeset` calls
// These stored results are used to preserve the result of an asynchronous
// computation, so that they can be polled afterwards
union MegarepoSyncChangesetResult {
    1: source_control.MegarepoSyncChangesetResponse success,
    2: StoredError error,
}

typedef mononoke_types_thrift.IdType MegarepoAsynchronousRequestResultId (rust.newtype)
union MegarepoAsynchronousRequestResult {
    1: MegarepoAddTargetResult megarepo_add_target_result,
    2: MegarepoChangeTargetConfigResult megarepo_change_target_result,
    3: MegarepoRemergeSourceResult megarepo_remerge_source_result,
    4: MegarepoSyncChangesetResult megarepo_sync_changeset_result,
}

typedef mononoke_types_thrift.IdType MegarepoAsynchronousRequestParamsId (rust.newtype)
union MegarepoAsynchronousRequestParams {
    1: source_control.MegarepoAddTargetParams megarepo_add_target_params,
    2: source_control.MegarepoChangeTargetConfigParams megarepo_change_target_params,
    3: source_control.MegarepoRemergeSourceParams megarepo_remerge_source_params,
    4: source_control.MegarepoSyncChangesetParams megarepo_sync_changeset_params,
}
