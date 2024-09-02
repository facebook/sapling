/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "fb303/thrift/fb303_core.thrift"
include "eden/mononoke/git/git_types/if/git_types_thrift.thrift"
include "eden/mononoke/filenodes/if/filenodes.thrift"
include "eden/mononoke/mercurial/types/if/mercurial_thrift.thrift"
include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/changeset_info.thrift"
include "eden/mononoke/mononoke_types/serialization/test_manifest.thrift"

struct DerivedDataType {
  1: string type_name;
} (rust.exhaustive)

union DerivationType {
  1: DeriveSingle derive_single;
  2: DeriveUnderived derive_underived;
  3: Rederivation rederive;
} (rust.ord)

struct DeriveSingle {} (rust.exhaustive)

struct DeriveUnderived {} (rust.exhaustive)

struct Rederivation {} (rust.exhaustive)

/// Represents status of derivation request
enum RequestStatus {
  /// Derivation succeeded
  SUCCESS = 0,
  /// Derivation was request but still in progress
  IN_PROGRESS = 1,
  /// Derivation either wasn't requested or finished
  DOES_NOT_EXIST = 2,
}

struct DeriveRequest {
  1: string repo_name;
  2: DerivedDataType derived_data_type;
  3: binary changeset_id;
  4: string config_name;
  5: DerivationType derivation_type;
  6: optional i64 bubble_id;
} (rust.exhaustive)

struct DeriveResponse {
  1: optional DerivedData data;
  2: RequestStatus status;
} (rust.exhaustive)

struct PollRequest {
  1: DeriveRequest original_request;
} (rust.exhaustive)

union DerivedData {
  1: DerivedDataFsnode fsnode;
  2: DerivedDataUnode unode;
  3: DerivedDataFilenode filenode;
  4: DerivedDataFastlog fastlog;
  5: DerivedDataBlame blame;
  6: DerivedDataHgChangeset hg_changeset;
  7: DerivedDataChangesetInfo changeset_info;
  // 8: deleted deleted_manifest
  9: DerivedDataSkeletonManifest skeleton_manifest;
  10: DerivedDataTreeHandle tree_handle;
  11: DerivedDataDeletedManifestV2 deleted_manifest_v2;
  // 12: deleted basename_suffix_skeleton_manifest
  13: DerivedDataCommitHandle commit_handle;
  // 14: deleted git_delta_manifest
  15: DerivedDataTestManifest test_manifest;
  16: DerivedDataTestShardedManifest test_sharded_manifest;
  17: DerivedDataBssmV3 bssm_v3;
  18: DerivedDataHgAugmentedManifest hg_augmented_manifest;
  19: DerivedDataGitDeltaManifestV2 git_delta_manifest_v2;
  20: DerivedDataSkeletonManifestV2 skeleton_manifest_v2;
  21: DerivedDataCcsm ccsm;
}

union DerivedDataFsnode {
  1: id.FsnodeId root_fsnode_id;
}

union DerivedDataUnode {
  1: id.ManifestUnodeId root_unode_manifest_id;
}

union DerivedDataFilenode {
  1: DerivedDataFilenodePresent filenode_present;
  2: DisabledFilenodes filenode_disabled;
}

struct DerivedDataFilenodePresent {
  1: optional filenodes.FilenodeInfo root_filenode;
} (rust.exhaustive)

union DerivedDataFastlog {
  1: id.ChangesetId root_fastlog_id;
}

union DerivedDataBlame {
  // 1: DerivedDataRootBlameV1 was deleted
  2: DerivedDataRootBlameV2 root_blame_v2;
}

struct DerivedDataRootBlameV2 {
  1: id.ChangesetId changeset_id;
  2: DerivedDataUnode unode;
} (rust.exhaustive)

union DerivedDataHgChangeset {
  1: mercurial_thrift.HgNodeHash mapped_hgchangeset_id;
}

union DerivedDataChangesetInfo {
  1: changeset_info.ChangesetInfo changeset_info;
}

union DerivedDataDeletedManifestV2 {
  1: id.DeletedManifestV2Id root_deleted_manifest_v2_id;
}

union DerivedDataBssmV3 {
  1: id.BssmV3DirectoryId root_bssm_v3_directory_id;
}

union DerivedDataSkeletonManifestV2 {
  1: id.SkeletonManifestV2Id root_skeleton_manifest_v2_id;
}

union DerivedDataCcsm {
  1: id.CaseConflictSkeletonManifestId root_ccsm_id;
}

union DerivedDataTestManifest {
  1: test_manifest.TestManifestDirectory root_test_manifest_directory;
}

union DerivedDataTestShardedManifest {
  1: test_manifest.TestShardedManifestDirectory root_test_sharded_manifest_directory;
}

union DerivedDataSkeletonManifest {
  1: id.SkeletonManifestId root_skeleton_manifest_id;
}

union DerivedDataTreeHandle {
  1: git_types_thrift.TreeHandle tree_handle;
}

union DerivedDataCommitHandle {
  1: git_types_thrift.MappedGitCommitId mapped_commit_id;
}

union DerivedDataHgAugmentedManifest {
  1: mercurial_thrift.HgNodeHash root_hg_augmented_manifest_id;
}
union DerivedDataGitDeltaManifestV2 {
  1: git_types_thrift.GitDeltaManifestV2Id root_git_delta_manifest_v2_id;
}

struct DerivedDataTypeNotEnabled {
  1: string reason;
} (rust.exhaustive)

struct CommitNotFound {
  1: string changeset_id;
  2: string repo_name;
} (rust.exhaustive)

struct RepoNotFound {
  1: string reason;
} (rust.exhaustive)

struct UnknownDerivedDataConfig {
  1: string reason;
} (rust.exhaustive)

struct UnknownDerivationType {
  1: string reason;
} (rust.exhaustive)

struct DisabledDerivation {
  1: string type_name;
  2: i32 repo_id;
  3: string repo_name;
} (rust.exhaustive)

struct TypeDisabledForEphemeralBubbles {
  1: string type_name;
  2: i32 repo_id;
} (rust.exhaustive)

struct DisabledFilenodes {} (rust.exhaustive)

union RequestErrorReason {
  1: DerivedDataTypeNotEnabled derived_data_type_not_enabled;
  2: CommitNotFound commit_not_found;
  3: RepoNotFound repo_not_found;
  4: UnknownDerivedDataConfig unknown_derived_data_config;
  5: UnknownDerivationType unknown_derivation_type;
  6: DisabledDerivation disabled_derivation;
  7: TypeDisabledForEphemeralBubbles type_disabled_for_ephemeral_bubbles;
}

safe permanent client exception RequestError {
  1: RequestErrorReason reason;
} (rust.exhaustive)

safe permanent server exception InternalError {
  1: string reason;
} (rust.exhaustive)

service DerivedDataService extends fb303_core.BaseService {
  /// Request derivation for given commit. Service will find all underived commits
  /// and dependency for other derived data types
  DeriveResponse derive(1: DeriveRequest request) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
  );

  /// If derivation took longer than initial request timeout clients should poll
  /// using this method. This method will not trigger derivation.
  DeriveResponse poll(1: PollRequest request) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
  );
} (rust.request_context)
