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

#include "eden/fs/model/RootId.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
}

namespace facebook::eden {

class Blob;
class Tree;
class TreeEntry;
enum class TreeEntryType : uint8_t;

/**
 * Abstract interface for a BackingStore.
 *
 * A BackingStore fetches tree and blob information from an external
 * authoritative data source.
 *
 * BackingStore implementations must be thread-safe, and perform their own
 * internal locking.
 */
class BackingStore : public RootIdCodec {
 public:
  BackingStore() {}
  virtual ~BackingStore() {}

  virtual folly::SemiFuture<std::unique_ptr<Tree>> getRootTree(
      const RootId& rootId,
      ObjectFetchContext& context) = 0;
  /**
   * The API should accept object id instead of rootId. But Object is currently
   * a fixed 20 bytes, so temporariorly use rootId instead.
   * TODO: Replace rootID with objectId once objectId is widened.
   */
  virtual folly::SemiFuture<std::unique_ptr<TreeEntry>> getTreeEntryForRootId(
      const RootId& rootId,
      TreeEntryType treeEntryType,
      facebook::eden::PathComponentPiece pathComponentPiece,
      ObjectFetchContext& context) = 0;

  /**
   * Return value of the getTree method.
   */
  struct GetTreeRes {
    /** The retrieved tree. */
    std::unique_ptr<Tree> tree;
    /** The fetch origin of the tree. */
    ObjectFetchContext::Origin origin;
  };

  /**
   * Fetch a tree from the backing store.
   *
   * Return the tree and where it was found.
   */
  virtual folly::SemiFuture<GetTreeRes> getTree(
      const Hash& id,
      ObjectFetchContext& context) = 0;

  /**
   * Return value of the getBlob method.
   */
  struct GetBlobRes {
    /** The retrieved blob. */
    std::unique_ptr<Blob> blob;
    /** The fetch origin of the tree. */
    ObjectFetchContext::Origin origin;
  };

  /**
   * Fetch a blob from the backing store.
   *
   * Return the blob and where it was found.
   */
  virtual folly::SemiFuture<GetBlobRes> getBlob(
      const Hash& id,
      ObjectFetchContext& context) = 0;

  /**
   * Prefetch all the blobs represented by the HashRange.
   *
   * The caller is responsible for making sure that the HashRange stays valid
   * for as long as the returned SemiFuture.
   */
  FOLLY_NODISCARD virtual folly::SemiFuture<folly::Unit> prefetchBlobs(
      HashRange /*ids*/,
      ObjectFetchContext& /*context*/) {
    return folly::unit;
  }

  virtual void periodicManagementTask() {}

  /**
   * Subclass of BackingStore will override these functions to record file
   * paths fetched. After startRecordingFetch() is called, the BackingStore
   * will record fetched file paths. stopRecordingFetch() will disable the
   * recording and return the fetched files since startRecordingFetch() is
   * called and clear the old records.
   *
   * Currently implemented in HgQueuedBackingStore.
   *
   * Note: Only stopRecordingFetch() clears old records. Calling
   * startRecordingFetch() a second time has no effect.
   */
  virtual void startRecordingFetch() {}
  virtual std::unordered_set<std::string> stopRecordingFetch() {
    return {};
  }

  /**
   * Directly import a manifest for a root.
   *
   * Subclasses of BackingStore can override this to opportunistically import
   * known manifests for a particular root.
   *
   * This is called when the hg client informs EdenFS of a root to manifest
   * mapping.  This is useful when the commit has just been created, as
   * EdenFS won't be able to find out the manifest from the import helper
   * until it re-opens the repo.
   *
   * TODO: When EdenFS no longer uses hg import helper subprocesses and when
   * Hash is widened to variable-width, eliminating the need for proxy hashes,
   * this API should be removed.
   */
  virtual folly::SemiFuture<folly::Unit> importManifestForRoot(
      const RootId& /*rootId*/,
      const Hash& /*manifest*/) {
    return folly::unit;
  }

 private:
  // Forbidden copy constructor and assignment operator
  BackingStore(BackingStore const&) = delete;
  BackingStore& operator=(BackingStore const&) = delete;
};

} // namespace facebook::eden
