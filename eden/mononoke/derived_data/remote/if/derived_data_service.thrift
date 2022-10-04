/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "fb303/thrift/fb303_core.thrift"
include "eden/mononoke/derived_data/changeset_info/if/changeset_info_thrift.thrift"
include "eden/mononoke/git/git_types/if/git_types_thrift.thrift"
include "eden/mononoke/filenodes/if/filenodes.thrift"
include "eden/mononoke/mercurial/types/if/mercurial_thrift.thrift"
include "eden/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"

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

struct DeriveRequest {
  1: string repo_name;
  2: DerivedDataType derived_data_type;
  3: binary changeset_id;
  4: string config_name;
  5: DerivationType derivation_type;
} (rust.exhaustive)

struct DeriveResponse {
  1: optional DerivedData data;
} (rust.exhaustive)

union DerivedData {
  1: DerivedDataFsnode fsnode;
  2: DerivedDataUnode unode;
  3: DerivedDataFilenode filenode;
  4: DerivedDataFastlog fastlog;
  5: DerivedDataBlame blame;
  6: DerivedDataHgChangeset hg_changeset;
  7: DerivedDataChangesetInfo changeset_info;
  8: DerivedDataDeletedManifest deleted_manifest;
  9: DerivedDataSkeletonManifest skeleton_manifest;
  10: DerivedDataTreeHandle tree_handle;
  11: DerivedDataDeletedManifestV2 deleted_manifest_v2;
  12: DerivedDataBasenameSuffixSkeletonManifest basename_suffix_skeleton_manifest;
}

union DerivedDataFsnode {
  1: mononoke_types_thrift.FsnodeId root_fsnode_id;
}

union DerivedDataUnode {
  1: mononoke_types_thrift.ManifestUnodeId root_unode_manifest_id;
}

union DerivedDataFilenode {
  1: DerivedDataFilenodePresent filenode_present;
  2: DisabledFilenodes filenode_disabled;
}

struct DerivedDataFilenodePresent {
  1: optional filenodes.FilenodeInfo root_filenode;
} (rust.exhaustive)

union DerivedDataFastlog {
  1: mononoke_types_thrift.ChangesetId root_fastlog_id;
}

union DerivedDataBlame {
  1: DerivedDataRootBlameV1 root_blame_v1;
  2: DerivedDataRootBlameV2 root_blame_v2;
}

struct DerivedDataRootBlameV1 {
  1: mononoke_types_thrift.ChangesetId blame_root_id;
} (rust.exhaustive)

struct DerivedDataRootBlameV2 {
  1: mononoke_types_thrift.ChangesetId changeset_id;
  2: DerivedDataUnode unode;
} (rust.exhaustive)

union DerivedDataHgChangeset {
  1: mercurial_thrift.HgNodeHash mapped_hgchangeset_id;
}

union DerivedDataChangesetInfo {
  1: changeset_info_thrift.ChangesetInfo changeset_info;
}

union DerivedDataDeletedManifest {
  1: mononoke_types_thrift.DeletedManifestId root_deleted_manifest_id;
}

union DerivedDataDeletedManifestV2 {
  1: mononoke_types_thrift.DeletedManifestV2Id root_deleted_manifest_v2_id;
}

union DerivedDataBasenameSuffixSkeletonManifest {
  1: mononoke_types_thrift.BssmDirectory root_basename_suffix_skeleton_manifest;
}

union DerivedDataSkeletonManifest {
  1: mononoke_types_thrift.SkeletonManifestId root_skeleton_manifest_id;
}

union DerivedDataTreeHandle {
  1: git_types_thrift.TreeHandle tree_handle;
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

struct DisabledFilenodes {} (rust.exhaustive)

union RequestErrorReason {
  1: DerivedDataTypeNotEnabled derived_data_type_not_enabled;
  2: CommitNotFound commit_not_found;
  3: RepoNotFound repo_not_found;
  4: UnknownDerivedDataConfig unknown_derived_data_config;
  5: UnknownDerivationType unknown_derivation_type;
  6: DisabledDerivation disabled_derivation;
}

safe permanent client exception RequestError {
  1: RequestErrorReason reason;
} (rust.exhaustive)

safe permanent server exception InternalError {
  1: string reason;
} (rust.exhaustive)

service DerivedDataService extends fb303_core.BaseService {
  # At first stage of the project this method requires that
  # parents of the commit have to be derived already
  DeriveResponse derive(1: DeriveRequest request) throws (
    1: RequestError request_error,
    2: InternalError internal_error,
  );
} (rust.request_context)
