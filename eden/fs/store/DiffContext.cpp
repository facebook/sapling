/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/DiffContext.h"

#include <thrift/lib/cpp2/async/ResponseChannel.h>

#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/model/git/TopLevelIgnores.h"
#include "eden/fs/store/IObjectStore.h"

using apache::thrift::ResponseChannelRequest;

namespace facebook::eden {

DiffContext::DiffContext(
    DiffCallback* cb,
    bool listIgnored,
    CaseSensitivity caseSensitive,
    const ObjectStore* os,
    std::unique_ptr<TopLevelIgnores> topLevelIgnores,
    LoadFileFunction loadFileContentsFromPath,
    ResponseChannelRequest* request)
    : callback{cb},
      store{os},
      listIgnored{listIgnored},
      caseSensitive{caseSensitive},
      topLevelIgnores_(std::move(topLevelIgnores)),
      loadFileContentsFromPath_{loadFileContentsFromPath},
      request_{request} {}

DiffContext::DiffContext(DiffCallback* cb, const ObjectStore* os)
    : callback{cb},
      store{os},
      listIgnored{true},
      caseSensitive{kPathMapDefaultCaseSensitive},
      topLevelIgnores_{std::unique_ptr<TopLevelIgnores>()},
      loadFileContentsFromPath_{nullptr},
      request_{nullptr} {};

DiffContext::~DiffContext() = default;

const GitIgnoreStack* DiffContext::getToplevelIgnore() const {
  return topLevelIgnores_->getStack();
}

DiffContext::LoadFileFunction DiffContext::getLoadFileContentsFromPath() const {
  return loadFileContentsFromPath_;
}

bool DiffContext::isCancelled() const {
  // If request_ is null we do not have an associated thrift
  // request that can be cancelled, so we are always still active
  if (request_ && !request_->isActive()) {
    return true;
  }
  return false;
}

} // namespace facebook::eden
