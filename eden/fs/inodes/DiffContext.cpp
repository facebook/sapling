/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/DiffContext.h"
#include "eden/fs/inodes/TopLevelIgnores.h"
#include "eden/fs/model/git/GitIgnoreStack.h"

namespace facebook {
namespace eden {

DiffContext::DiffContext(
    DiffCallback* cb,
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
