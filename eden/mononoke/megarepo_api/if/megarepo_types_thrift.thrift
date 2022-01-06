/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
typedef mononoke_types_thrift.IdType MegarepoAddBranchingTargetParamsId (
  rust.newtype,
)
typedef mononoke_types_thrift.IdType MegarepoChangeTargetConfigParamsId (
  rust.newtype,
)
typedef mononoke_types_thrift.IdType MegarepoRemergeSourceParamsId (
  rust.newtype,
)
typedef mononoke_types_thrift.IdType MegarepoSyncChangesetParamsId (
  rust.newtype,
)

typedef mononoke_types_thrift.IdType MegarepoSyncChangesetResultId (
  rust.newtype,
)
typedef mononoke_types_thrift.IdType MegarepoAddTargetResultId (rust.newtype)
typedef mononoke_types_thrift.IdType MegarepoAddBranchingTargetResultId (
  rust.newtype,
)
typedef mononoke_types_thrift.IdType MegarepoRemergeSourceResultId (
  rust.newtype,
)
typedef mononoke_types_thrift.IdType MegarepoChangeTargetConfigResultId (
  rust.newtype,
)

typedef mononoke_types_thrift.IdType MegarepoAsynchronousRequestResultId (
  rust.newtype,
)
union MegarepoAsynchronousRequestResult {
  1: source_control.MegarepoAddTargetResult megarepo_add_target_result;
  2: source_control.MegarepoChangeTargetConfigResult megarepo_change_target_result;
  3: source_control.MegarepoRemergeSourceResult megarepo_remerge_source_result;
  4: source_control.MegarepoSyncChangesetResult megarepo_sync_changeset_result;
  5: source_control.MegarepoAddBranchingTargetResult megarepo_add_branching_target_result;
}

typedef mononoke_types_thrift.IdType MegarepoAsynchronousRequestParamsId (
  rust.newtype,
)
union MegarepoAsynchronousRequestParams {
  1: source_control.MegarepoAddTargetParams megarepo_add_target_params;
  2: source_control.MegarepoChangeTargetConfigParams megarepo_change_target_params;
  3: source_control.MegarepoRemergeSourceParams megarepo_remerge_source_params;
  4: source_control.MegarepoSyncChangesetParams megarepo_sync_changeset_params;
  5: source_control.MegarepoAddBranchingTargetParams megarepo_add_branching_target_params;
}
