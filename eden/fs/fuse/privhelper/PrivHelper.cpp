/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/privhelper/PrivHelper.h"

#include <folly/File.h>
#include <folly/futures/Future.h>
#include <folly/io/async/EventBase.h>

namespace facebook {
namespace eden {

void PrivHelper::setLogFileBlocking(folly::File logFile) {
  folly::EventBase evb;
  attachEventBase(&evb);

  auto future = setLogFile(std::move(logFile));
  if (future.isReady()) {
    std::move(future).get();
    return;
  }

  future = std::move(future).ensure([&evb] { evb.terminateLoopSoon(); });
  evb.loopForever();
  std::move(future).get();
}

} // namespace eden
} // namespace facebook
