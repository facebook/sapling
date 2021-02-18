/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <sys/stat.h>

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace folly {
template <class T>
class Future;
}

namespace facebook::eden {

class EdenStats;

class NfsDispatcher {
 public:
  explicit NfsDispatcher(EdenStats* stats) : stats_(stats) {}

  virtual ~NfsDispatcher() {}

  EdenStats* getStats() const {
    return stats_;
  }

  /**
   * Get file attribute for the passed in InodeNumber.
   */
  virtual folly::Future<struct stat> getattr(
      InodeNumber ino,
      ObjectFetchContext& context) = 0;

 private:
  EdenStats* stats_{nullptr};
};

} // namespace facebook::eden

#endif
