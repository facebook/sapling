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

#include "eden/fs/model/hg/Dirstate.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/**
 * Implementation of DirstatePersistence that persists data to a local file.
 */
class LocalDirstatePersistence : public DirstatePersistence {
 public:
  explicit LocalDirstatePersistence(AbsolutePathPiece storageFile)
      : storageFile_(storageFile) {}

  virtual ~LocalDirstatePersistence() {}

  void save(const std::unordered_map<RelativePath, HgUserStatusDirective>&
                userDirectives) override;

  /**
   * If the underlying storage file does not exist, then this returns an empty
   * map.
   */
  std::unordered_map<RelativePath, HgUserStatusDirective> load();

 private:
  AbsolutePath storageFile_;
};
}
}
