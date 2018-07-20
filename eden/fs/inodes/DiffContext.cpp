/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include "eden/fs/inodes/DiffContext.h"
#include "eden/fs/inodes/TopLevelIgnores.h"
#include "eden/fs/model/git/GitIgnoreStack.h"

namespace facebook {
namespace eden {

DiffContext::DiffContext(
    InodeDiffCallback* cb,
    bool listIgnored,
    const ObjectStore* os,
    std::unique_ptr<TopLevelIgnores> topLevelIgnores)
    : callback{cb},
      store{os},
      listIgnored{listIgnored},
      topLevelIgnores_(std::move(topLevelIgnores)) {}

DiffContext::~DiffContext() = default;

const GitIgnoreStack* DiffContext::getToplevelIgnore() const {
  return topLevelIgnores_->getStack();
}

} // namespace eden
} // namespace facebook
