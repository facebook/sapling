/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/futures/Future.h>

#include "eden/fs/store/StatsFetchContext.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
} // namespace folly

namespace apache {
namespace thrift {
class ResponseChannelRequest;
}
} // namespace apache

namespace facebook::eden {

class DiffCallback;
class GitIgnoreStack;
class ObjectFetchContext;
class ObjectStore;
class UserInfo;
class TopLevelIgnores;
class EdenMount;

/**
 * A helper class to store parameters for a TreeInode::diff() operation.
 *
 * These parameters remain fixed across all subdirectories being diffed.
 * Primarily intent is to compound related diff attributes.
 *
 * The DiffContext must be alive for the duration of the async operation it is
 * used in.
 */
class DiffContext {
 public:
  using LoadFileFunction = std::function<
      folly::Future<std::string>(ObjectFetchContext&, RelativePathPiece)>;

  DiffContext(
      DiffCallback* cb,
      bool listIgnored,
      CaseSensitivity caseSensitive,
      const ObjectStore* os,
      std::unique_ptr<TopLevelIgnores> topLevelIgnores,
      LoadFileFunction loadFileContentsFromPath,
      apache::thrift::ResponseChannelRequest* FOLLY_NULLABLE request = nullptr);

  /**
   * Test only constructor.
   */
  DiffContext(DiffCallback* cb, const ObjectStore* os);

  DiffContext(const DiffContext&) = delete;
  DiffContext& operator=(const DiffContext&) = delete;
  DiffContext(DiffContext&&) = delete;
  DiffContext& operator=(DiffContext&&) = delete;
  ~DiffContext();

  DiffCallback* const callback;
  const ObjectStore* const store;
  /**
   * If listIgnored is true information about ignored files will be reported.
   * If listIgnored is false then ignoredFile() will never be called on the
   * callback.  The diff operation may be faster with listIgnored=false, since
   * it can completely omit processing ignored subdirectories.
   */
  bool const listIgnored;

  /**
   * Controls the case sensitivity of the diff operation.
   */
  CaseSensitivity const caseSensitive;

  const GitIgnoreStack* getToplevelIgnore() const;
  bool isCancelled() const;
  LoadFileFunction getLoadFileContentsFromPath() const;
  StatsFetchContext& getFetchContext() {
    return fetchContext_;
  }

  /** Whether this repository is mounted in case-sensitive mode */
  CaseSensitivity getCaseSensitive() const {
    return caseSensitive_;
  }

 private:
  std::unique_ptr<TopLevelIgnores> topLevelIgnores_;
  const LoadFileFunction loadFileContentsFromPath_;
  apache::thrift::ResponseChannelRequest* const FOLLY_NULLABLE request_;
  StatsFetchContext fetchContext_;
  CaseSensitivity caseSensitive_;
};

} // namespace facebook::eden
