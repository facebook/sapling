/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <glog/logging.h>
#include <variant>
#include "eden/fs/config/EdenConfig.h"

namespace facebook {
namespace eden {

/**
 * Indicates the key space is safe to clear at any moment. The key space's disk
 * usage should be kept under the size specified by `cacheLimit`.
 */
struct Ephemeral {
  ConfigSetting<uint64_t> EdenConfig::*cacheLimit;
};

/**
 * Indicates the key space contains persistent data and should never be cleared.
 */
struct Persistent {};

/**
 * The key space is no longer used. It should be cleared on startup.
 */
struct Deprecated {};

using Persistence = std::variant<Ephemeral, Persistent, Deprecated>;

/**
 * Which key space (and thus column family for the RocksDbLocalStore) should be
 * used to store a specific key.  The `name` value must be stable across builds
 * as it is used to identify the table names in RocksDbLocalStore and
 * SqliteLocalStore.
 */
struct KeySpaceRecord {
  uint8_t index;
  folly::StringPiece name;
  Persistence persistence;

  constexpr bool isEphemeral() const noexcept {
    return std::holds_alternative<Ephemeral>(persistence);
  }

  constexpr bool isDeprecated() const noexcept {
    return std::holds_alternative<Deprecated>(persistence);
  }
};

class KeySpace {
 public:
  /* implicit */ constexpr KeySpace(const KeySpaceRecord& record)
      : record_{&record} {}

  /* implicit */ KeySpace(const KeySpaceRecord* record) : record_{record} {
    CHECK_NOTNULL(record);
  }

  constexpr const KeySpaceRecord* operator->() const {
    return record_;
  }

  static constexpr KeySpaceRecord BlobFamily{
      0,
      "blob",
      Ephemeral{&EdenConfig::localStoreBlobSizeLimit}};
  static constexpr KeySpaceRecord BlobMetaDataFamily{
      1,
      "blobmeta",
      Ephemeral{&EdenConfig::localStoreBlobMetaSizeLimit}};
  static constexpr KeySpaceRecord TreeFamily{
      2,
      "tree",
      Ephemeral{&EdenConfig::localStoreTreeSizeLimit}};
  // Proxy hashes are required to fetch objects from hg from a hash.
  // Deleting them breaks re-importing after an inode is unloaded.
  static constexpr KeySpaceRecord HgProxyHashFamily{3,
                                                    "hgproxyhash",
                                                    Persistent{}};
  static constexpr KeySpaceRecord HgCommitToTreeFamily{
      4,
      "hgcommit2tree",
      Ephemeral{&EdenConfig::localStoreHgCommit2TreeSizeLimit}};
  static constexpr KeySpaceRecord BlobSizeFamily{5, "blobsize", Deprecated{}};

  static constexpr const KeySpaceRecord* kAll[] = {&BlobFamily,
                                                   &BlobMetaDataFamily,
                                                   &TreeFamily,
                                                   &HgProxyHashFamily,
                                                   &HgCommitToTreeFamily,
                                                   &BlobSizeFamily};
  static constexpr size_t kTotalCount = std::size(kAll);

 private:
  const KeySpaceRecord* record_;
};

} // namespace eden
} // namespace facebook
