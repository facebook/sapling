/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/DiffContext.h"

#include <thrift/lib/cpp2/async/ResponseChannel.h>

#include "eden/fs/inodes/TopLevelIgnores.h"
#include "eden/fs/model/git/GitIgnoreStack.h"

using apache::thrift::ResponseChannelRequest;

namespace facebook {
namespace eden {

DiffContext::DiffContext(
    DiffCallback* cb,
    bool listIgnored,
    const ObjectStore* os,
    std::unique_ptr<TopLevelIgnores> topLevelIgnores,
    ResponseChannelRequest* request)
    : callback{cb},
      store{os},
      listIgnored{listIgnored},
      topLevelIgnores_(std::move(topLevelIgnores)),
      request_{request} {}

DiffContext::~DiffContext() = default;

const GitIgnoreStack* DiffContext::getToplevelIgnore() const {
  return topLevelIgnores_->getStack();
}

bool DiffContext::isCancelled() const {
  // If request_ is null we do not have an associated thrift
  // request that can be cancelled, so we are always still active
  if (request_ && !request_->isActive()) {
    return true;
  }
  return false;
}

} // namespace eden
} // namespace facebook
