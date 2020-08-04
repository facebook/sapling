/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/futures/Future.h>
#include <memory>

#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace folly {
template <typename T>
class Future;
}

namespace facebook {
namespace eden {

class Blob;
class Hash;
class Tree;
/**
 * Abstract interface for a BackingStore.
 *
 * A BackingStore fetches tree and blob information from an external
 * authoritative data source.
 *
 * BackingStore implementations must be thread-safe, and perform their own
 * internal locking.
 */
class BackingStore {
 public:
  BackingStore() {}
  virtual ~BackingStore() {}

  virtual folly::SemiFuture<std::unique_ptr<Tree>> getTree(
      const Hash& id,
      ObjectFetchContext& context) = 0;
  virtual folly::SemiFuture<std::unique_ptr<Blob>> getBlob(
      const Hash& id,
      ObjectFetchContext& context) = 0;

  virtual folly::SemiFuture<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) = 0;
  virtual folly::SemiFuture<std::unique_ptr<Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID) = 0;
  FOLLY_NODISCARD virtual folly::SemiFuture<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& /*ids*/,
      ObjectFetchContext& /*context*/) {
    return folly::unit;
  }

  virtual void periodicManagementTask() {}

  /**
   * Subclass of BackingStore will override these functions to record file paths
   * fetched. By default, recordFetch() does nothing. After
   * startRecordingFetch() is called, recordFetch() starts to records fetched
   * file paths. stopRecordingFetch() will disable recordFetch()'s function and
   * return the fetched files since startRecordingFetch() is called and clear
   * the old records.
   *
   * Currently implemented in HgQueuedBackingStore.
   *
   * Note: Only stopRecordingFetch() clears old records. Calling
   * startRecordingFetch() a second time has no effect.
   */
  virtual void startRecordingFetch() {}
  virtual void recordFetch(folly::StringPiece) {}
  virtual std::unordered_set<std::string> stopRecordingFetch() {
    return {};
  }

 private:
  // Forbidden copy constructor and assignment operator
  BackingStore(BackingStore const&) = delete;
  BackingStore& operator=(BackingStore const&) = delete;
};
} // namespace eden
} // namespace facebook
