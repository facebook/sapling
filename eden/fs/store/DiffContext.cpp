/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/DiffContext.h"

#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/model/git/TopLevelIgnores.h"
#include "eden/fs/store/IObjectStore.h"

namespace facebook::eden {

DiffContext::DiffContext(
    DiffCallback* cb,
    folly::CancellationToken cancellation,
    bool listIgnored,
    CaseSensitivity caseSensitive,
    const ObjectStore* os,
    std::unique_ptr<TopLevelIgnores> topLevelIgnores)
    : callback{cb},
      store{os},
      listIgnored{listIgnored},
      topLevelIgnores_(std::move(topLevelIgnores)),
      cancellation_{std::move(cancellation)},
      caseSensitive_{caseSensitive} {}

DiffContext::~DiffContext() = default;

const GitIgnoreStack* DiffContext::getToplevelIgnore() const {
  return topLevelIgnores_->getStack();
}

bool DiffContext::isCancelled() const {
  return cancellation_.isCancellationRequested();
}

} // namespace facebook::eden
