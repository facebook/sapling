/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/experimental/StringKeyedUnorderedMap.h>
#include "eden/fs/inodes/gen-cpp2/hgdirstate_types.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

// This is similar to the data stored in Mercurial's dirstate.py. The biggest
// difference is that we only store "nonnormal" files whereas Mercurial's
// dirstate stores information about all files in the repo.
struct DirstateData {
  folly::StringKeyedUnorderedMap<hgdirstate::DirstateTuple> hgDirstateTuples;
  folly::StringKeyedUnorderedMap<RelativePath> hgDestToSourceCopyMap;
};

/**
 * Persists dirstate data to a local file.
 */
class DirstatePersistence {
 public:
  explicit DirstatePersistence(AbsolutePathPiece storageFile)
      : storageFile_(storageFile) {}

  void save(const DirstateData& data);

  /**
   * If the underlying storage file does not exist, then this returns an empty
   * map.
   */
  DirstateData load();

 private:
  void save(
      const std::map<std::string, hgdirstate::DirstateTuple>& hgDirstateTuples,
      const std::map<std::string, std::string>& hgDestToSourceCopyMap);

  AbsolutePath storageFile_;
};
}
}
