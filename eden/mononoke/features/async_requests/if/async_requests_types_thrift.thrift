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
include "thrift/annotation/rust.thrift"
include "thrift/annotation/thrift.thrift"

@thrift.AllowLegacyMissingUris
package;

// Id types for async service methods params and responses.
// Param and response types themselves are defined in the source_control.trift
@rust.NewType
typedef id.Id MegarepoAddTargetParamsId
@rust.NewType
typedef id.Id MegarepoAddBranchingTargetParamsId
@rust.NewType
typedef id.Id MegarepoChangeTargetConfigParamsId
@rust.NewType
typedef id.Id MegarepoRemergeSourceParamsId
@rust.NewType
typedef id.Id MegarepoSyncChangesetParamsId
@rust.NewType
typedef id.Id AsyncPingParamsId
@rust.NewType
typedef id.Id CommitSparseProfileSizeParamsId
@rust.NewType
typedef id.Id CommitSparseProfileDeltaParamsId

@rust.NewType
typedef id.Id MegarepoSyncChangesetResultId
@rust.NewType
typedef id.Id MegarepoAddTargetResultId
@rust.NewType
typedef id.Id MegarepoAddBranchingTargetResultId
@rust.NewType
typedef id.Id MegarepoRemergeSourceResultId
@rust.NewType
typedef id.Id MegarepoChangeTargetConfigResultId
@rust.NewType
typedef id.Id AsyncPingResultId
@rust.NewType
typedef id.Id CommitSparseProfileSizeResultId
@rust.NewType
typedef id.Id CommitSparseProfileDeltaResultId

@rust.NewType
typedef id.Id AsynchronousRequestResultId
union AsynchronousRequestResult {
  1: source_control.MegarepoAddTargetResult megarepo_add_target_result;
  2: source_control.MegarepoChangeTargetConfigResult megarepo_change_target_result;
  3: source_control.MegarepoRemergeSourceResult megarepo_remerge_source_result;
  4: source_control.MegarepoSyncChangesetResult megarepo_sync_changeset_result;
  5: source_control.MegarepoAddBranchingTargetResult megarepo_add_branching_target_result;
  6: source_control.AsyncPingResponse async_ping_result;
  7: source_control.CommitSparseProfileSizeResponse commit_sparse_profile_size_result;
  8: source_control.AsyncRequestError error;
  9: source_control.CommitSparseProfileDeltaResponse commit_sparse_profile_delta_result;
}

@rust.NewType
typedef id.Id AsynchronousRequestParamsId
union AsynchronousRequestParams {
  1: source_control.MegarepoAddTargetParams megarepo_add_target_params;
  2: source_control.MegarepoChangeTargetConfigParams megarepo_change_target_params;
  3: source_control.MegarepoRemergeSourceParams megarepo_remerge_source_params;
  4: source_control.MegarepoSyncChangesetParams megarepo_sync_changeset_params;
  5: source_control.MegarepoAddBranchingTargetParams megarepo_add_branching_target_params;
  6: source_control.AsyncPingParams async_ping_params;
  7: source_control.CommitSparseProfileSizeParamsV2 commit_sparse_profile_size_params;
  8: source_control.CommitSparseProfileDeltaParamsV2 commit_sparse_profile_delta_params;
}
