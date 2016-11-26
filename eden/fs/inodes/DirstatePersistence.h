/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <unordered_map>
#include "eden/fs/inodes/gen-cpp2/overlay_types.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/**
 * Persists dirstate data to a local file.
 */
class DirstatePersistence {
 public:
  explicit DirstatePersistence(AbsolutePathPiece storageFile)
      : storageFile_(storageFile) {}

  void save(
      const std::unordered_map<RelativePath, overlay::UserStatusDirective>&
          userDirectives);

  /**
   * If the underlying storage file does not exist, then this returns an empty
   * map.
   */
  std::unordered_map<RelativePath, overlay::UserStatusDirective> load();

 private:
  AbsolutePath storageFile_;
};
}
}
