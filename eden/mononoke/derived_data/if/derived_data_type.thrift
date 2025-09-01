/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

namespace cpp2 facebook.scm.service
namespace php DerivedData
namespace py scm.derived_data.thrift.derived_data_type
namespace py3 scm.derived_data.thrift
namespace java.swift com.facebook.scm.derived_data

/// An enum representing the DerivedDataType tied to the compiled time enforced rust enum, and
/// compatible with both SCS and the Derived Data Service interfaces
enum DerivedDataType {
  FSNODE = 1,
  UNODE = 2,
  FILENODE = 3,
  FASTLOG = 4,
  BLAME = 5,
  HG_CHANGESET = 6,
  CHANGESET_INFO = 7,
  // DELETED_MANIFEST = 8, // Deprecated -  please use equivalent to DELETED_MANIFEST_V2
  SKELETON_MANIFEST = 9,
  TREE_HANDLE = 10,
  DELETED_MANIFEST_V2 = 11,
  // BASENAME_SUFFIX_SKELETON_MANIFEST // Deprecated - please use BSSM_V3
  COMMIT_HANDLE = 13,
  GIT_DELTA_MANIFEST = 14,
  TEST_MANIFEST = 15,
  TEST_SHARDED_MANIFEST = 16,
  BSSM_V3 = 17,
  HG_AUGMENTED_MANIFEST = 18,
  GIT_DELTA_MANIFEST_V2 = 19,
  SKELETON_MANIFEST_V2 = 20,
  CCSM = 21,
  CONTENT_MANIFEST = 22,
  INFERRED_COPY_FROM = 23,
  GIT_DELTA_MANIFEST_V3 = 24,
}
