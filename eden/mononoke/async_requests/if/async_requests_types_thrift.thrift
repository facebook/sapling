/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Thrift types for async service methods
//! These aren't part of `source_control.trift`, because they aren't part of
//! the service interface. They aren't part of of `megarepo_configs.thrift`,
//! because they aren't configs. Rather, they are service implementation detail.

include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/scs/if/source_control.thrift"

// Id types for async service methods params and responses.
// Param and response types themselves are defined in the source_control.trift
typedef id.Id MegarepoAddTargetParamsId (rust.newtype)
typedef id.Id MegarepoAddBranchingTargetParamsId (rust.newtype)
typedef id.Id MegarepoChangeTargetConfigParamsId (rust.newtype)
typedef id.Id MegarepoRemergeSourceParamsId (rust.newtype)
typedef id.Id MegarepoSyncChangesetParamsId (rust.newtype)

typedef id.Id MegarepoSyncChangesetResultId (rust.newtype)
typedef id.Id MegarepoAddTargetResultId (rust.newtype)
typedef id.Id MegarepoAddBranchingTargetResultId (rust.newtype)
typedef id.Id MegarepoRemergeSourceResultId (rust.newtype)
typedef id.Id MegarepoChangeTargetConfigResultId (rust.newtype)

typedef id.Id AsynchronousRequestResultId (rust.newtype)
union AsynchronousRequestResult {
  1: source_control.MegarepoAddTargetResult megarepo_add_target_result;
  2: source_control.MegarepoChangeTargetConfigResult megarepo_change_target_result;
  3: source_control.MegarepoRemergeSourceResult megarepo_remerge_source_result;
  4: source_control.MegarepoSyncChangesetResult megarepo_sync_changeset_result;
  5: source_control.MegarepoAddBranchingTargetResult megarepo_add_branching_target_result;
}

typedef id.Id AsynchronousRequestParamsId (rust.newtype)
union AsynchronousRequestParams {
  1: source_control.MegarepoAddTargetParams megarepo_add_target_params;
  2: source_control.MegarepoChangeTargetConfigParams megarepo_change_target_params;
  3: source_control.MegarepoRemergeSourceParams megarepo_remerge_source_params;
  4: source_control.MegarepoSyncChangesetParams megarepo_sync_changeset_params;
  5: source_control.MegarepoAddBranchingTargetParams megarepo_add_branching_target_params;
}
