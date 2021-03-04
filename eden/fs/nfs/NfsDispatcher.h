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

  /**
   * Return value of the create method.
   */
  struct CreateRes {
    /** InodeNumber of the created file */
    InodeNumber ino;
    /** Attributes of the created file */
    struct stat stat;

    /** Attributes of the directory prior to creating the file */
    std::optional<struct stat> preDirStat;
    /** Attributes of the directory after creating the file */
    std::optional<struct stat> postDirStat;
  };

  /**
   * Create a regular file in the directory referenced by the InodeNumber dir.
   *
   * Both the pre and post stat for that directory needs to be collected in an
   * atomic manner: no other operation on the directory needs to be allowed in
   * between them. This is to ensure that the NFS client can properly detect if
   * its cache needs to be invalidated. Setting them both to std::nullopt is an
   * acceptable approach if the stat cannot be collected atomically.
   */
  virtual folly::Future<CreateRes> create(
      InodeNumber dir,
      PathComponent name,
      mode_t mode,
      ObjectFetchContext& context) = 0;

  /**
   * Return value of the mkdir method.
   */
  struct MkdirRes {
    /** InodeNumber of the created directory */
    InodeNumber ino;
    /** Attributes of the created directory */
    struct stat stat;

    /** Attributes of the directory prior to creating the subdirectory */
    std::optional<struct stat> preDirStat;
    /** Attributes of the directory after creating the subdirectory */
    std::optional<struct stat> postDirStat;
  };

  /**
   * Create a subdirectory in the directory referenced by the InodeNumber dir.
   *
   * For the pre and post dir stat, refer to the documentation of the create
   * method above.
   */
  virtual folly::Future<MkdirRes> mkdir(
      InodeNumber dir,
      PathComponent name,
      mode_t mode,
      ObjectFetchContext& context) = 0;

 private:
  EdenStats* stats_{nullptr};
};

} // namespace facebook::eden

#endif
