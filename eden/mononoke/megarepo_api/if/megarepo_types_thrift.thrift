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

// Stored error for asynchronous service methods
// TODO: better error structuring please
union StoredError {
    1: string request_error,
    2: string internal_error,
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
