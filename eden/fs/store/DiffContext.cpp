/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/DiffContext.h"

#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/model/git/TopLevelIgnores.h"
#include "eden/fs/store/ObjectStore.h"

namespace facebook::eden {

DiffContext::DiffContext(
    DiffCallback* cb,
    folly::CancellationToken cancellation,
    const ObjectFetchContextPtr& fetchContext,
    bool listIgnored,
    CaseSensitivity caseSensitive,
    bool windowsSymlinksEnabled,
    std::shared_ptr<ObjectStore> os,
    std::unique_ptr<TopLevelIgnores> topLevelIgnores)
    : callback{cb},
      store{std::move(os)},
      listIgnored{listIgnored},
      topLevelIgnores_(std::move(topLevelIgnores)),
      cancellation_{std::move(cancellation)},
      statsContext_{makeRefPtr<StatsFetchContext>(
          fetchContext->getClientPid(),
          fetchContext->getCause(),
          fetchContext->getCauseDetail(),
          fetchContext->getRequestInfo())},
      fetchContext_{statsContext_.copy()},
      caseSensitive_{caseSensitive},
      windowsSymlinksEnabled_{windowsSymlinksEnabled} {}

DiffContext::~DiffContext() = default;

const GitIgnoreStack* DiffContext::getToplevelIgnore() const {
  return topLevelIgnores_->getStack();
}

bool DiffContext::isCancelled() const {
  return cancellation_.isCancellationRequested();
}

} // namespace facebook::eden
