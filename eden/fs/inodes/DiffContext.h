/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Range.h>
#include <string>
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class InodeDiffCallback;
class ObjectStore;

/**
 * A small helper class to store parameters for a TreeInode::diff() operation.
 *
 * These are parameters that remain fixed across all subdirectories being
 * diffed.  This class is mostly just for convenience so that we do not have to
 * pass these items in individually as separate parameters to each function
 * being called.
 */
class DiffContext {
 public:
  // Loads the system-wide ignore settings and user-specific
  // ignore settings into top level git ignore stack
  DiffContext(InodeDiffCallback* cb, bool listIgnored, const ObjectStore* os);

  // this constructor is primarily intended for testing
  DiffContext(
      InodeDiffCallback* cb,
      bool listIgnored,
      const ObjectStore* os,
      folly::StringPiece systemWideIgnoreFileContents,
      folly::StringPiece userIgnoreFileContents);

  const GitIgnoreStack* getToplevelIgnore() const {
    return ownedIgnores_.empty() ? nullptr : ownedIgnores_.back().get();
  }

  InodeDiffCallback* const callback;
  const ObjectStore* const store;
  /**
   * If listIgnored is true information about ignored files will be reported.
   * If listIgnored is false then ignoredFile() will never be called on the
   * callback.  The diff operation may be faster with listIgnored=false, since
   * it can completely omit processing ignored subdirectories.
   */
  bool const listIgnored;

 private:
  static AbsolutePath constructUserIgnoreFileName();
  static std::string tryIngestFile(folly::StringPiece fileName);
  void initOwnedIgnores(
      folly::StringPiece systemWideIgnoreFileContents,
      folly::StringPiece userIgnoreFileContents);
  void pushFrameIfAvailable(folly::StringPiece ignoreFileContents);

  static constexpr folly::StringPiece kSystemWideIgnoreFileName =
      "/etc/eden/ignore";
  const AbsolutePath userIgnoreFileName_;
  std::vector<std::unique_ptr<GitIgnoreStack>> ownedIgnores_;
};
} // namespace eden
} // namespace facebook
