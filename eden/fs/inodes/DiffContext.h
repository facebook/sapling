/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Range.h>

namespace facebook {
namespace eden {

class InodeDiffCallback;
class GitIgnoreStack;
class ObjectStore;
class UserInfo;
class TopLevelIgnores;

/**
 * A helper class to store parameters for a TreeInode::diff() operation.
 *
 * These parameters remain fixed across all subdirectories being diffed.
 * Primarily intent is to compound related diff attributes.
 */
class DiffContext {
 public:
  DiffContext(
      InodeDiffCallback* cb,
      bool listIgnored,
      const ObjectStore* os,
      std::unique_ptr<TopLevelIgnores> topLevelIgnores);

  DiffContext(const DiffContext&) = delete;
  DiffContext& operator=(const DiffContext&) = delete;
  DiffContext(DiffContext&&) = delete;
  DiffContext& operator=(DiffContext&&) = delete;
  ~DiffContext();

  InodeDiffCallback* const callback;
  const ObjectStore* const store;
  /**
   * If listIgnored is true information about ignored files will be reported.
   * If listIgnored is false then ignoredFile() will never be called on the
   * callback.  The diff operation may be faster with listIgnored=false, since
   * it can completely omit processing ignored subdirectories.
   */
  bool const listIgnored;

  const GitIgnoreStack* getToplevelIgnore() const;

 private:
  std::unique_ptr<TopLevelIgnores> topLevelIgnores_;
};
} // namespace eden
} // namespace facebook
