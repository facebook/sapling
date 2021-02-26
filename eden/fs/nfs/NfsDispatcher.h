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
#include "eden/fs/utils/PathFuncs.h"

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

  /**
   * Racily obtain the parent directory of the passed in directory.
   *
   * Can be used to handle a ".." filename.
   */
  virtual folly::Future<InodeNumber> getParent(
      InodeNumber ino,
      ObjectFetchContext& context) = 0;

  /**
   * Find the given file in the passed in directory. It's InodeNumber and
   * attributes are returned.
   */
  virtual folly::Future<std::tuple<InodeNumber, struct stat>>
  lookup(InodeNumber dir, PathComponent name, ObjectFetchContext& context) = 0;

  /**
   * For a symlink, return its destination, fail otherwise.
   */
  virtual folly::Future<std::string> readlink(
      InodeNumber ino,
      ObjectFetchContext& context) = 0;

 private:
  EdenStats* stats_{nullptr};
};

} // namespace facebook::eden

#endif
