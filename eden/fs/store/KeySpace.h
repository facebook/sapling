/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <assert.h>
#include <folly/Range.h>
#include <glog/logging.h>

namespace facebook {
namespace eden {

enum class Persistence : bool {
  Ephemeral = false,
  Persistent = true,
};

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

  static constexpr KeySpaceRecord BlobFamily{0, "blob", Persistence::Ephemeral};
  static constexpr KeySpaceRecord BlobMetaDataFamily{1,
                                                     "blobmeta",
                                                     Persistence::Ephemeral};
  // It is too costly to have trees be deleted by automatic
  // background GC when there are programs that cause every
  // tree in the repo to be fetched. Make ephemeral when GC
  // is smarter and when Eden can more efficiently read from
  // the hg cache.  This would also be better if programs
  // weren't scanning the entire repo for filenames, causing
  // every tree to be loaded.
  static constexpr KeySpaceRecord TreeFamily{2,
                                             "tree",
                                             Persistence::Persistent};
  // Proxy hashes are required to fetch objects from hg from a hash.
  // Deleting them breaks re-importing after an inode is unloaded.
  static constexpr KeySpaceRecord HgProxyHashFamily{3,
                                                    "hgproxyhash",
                                                    Persistence::Persistent};
  static constexpr KeySpaceRecord HgCommitToTreeFamily{4,
                                                       "hgcommit2tree",
                                                       Persistence::Ephemeral};
  // Deprecated. TODO: Clear at startup.
  static constexpr KeySpaceRecord BlobSizeFamily{5,
                                                 "blobsize",
                                                 Persistence::Ephemeral};

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
