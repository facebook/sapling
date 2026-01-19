/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/coro/Task.h>
#include <folly/futures/Future.h>
#include <folly/memory/not_null.h>
#include <memory>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/BlobAuxDataFwd.h"
#include "eden/fs/model/BlobFwd.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/model/TreeAuxDataFwd.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/fs/store/BackingStoreType.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace folly {
template <typename T>
class Future;
}

namespace facebook::eden {

class TreeEntry;
enum class TreeEntryType : uint8_t;

enum class ObjectComparison : uint8_t {
  /// Given the IDs alone, it's not possible to know whether the contents are
  /// the same or different, and they must be fetched to compare.
  Unknown,
  /// The IDs are known to point to the same objects.
  Identical,
  /// The IDs are known to point to different objects.
  Different,
};

/**
 * Abstract interface for a BackingStore.
 *
 * A BackingStore fetches tree and blob information from an external
 * authoritative data source.
 *
 * BackingStore implementations must be thread-safe, and perform their own
 * internal locking.
 */
class BackingStore : public RootIdCodec, public ObjectIdCodec {
 public:
  BackingStore() = default;
  virtual ~BackingStore() = default;

  /**
   * A BackingStore may support multiple object ID encodings. To help EdenFS
   * short-circuit recursive comparisons when IDs aren't identical but identify
   * the same contents, this function allows querying whether two IDs refer to
   * the same contents.
   *
   * Returns ObjectComparison::Unknown if they must be fetched and compared to
   * know.
   */
  virtual ObjectComparison compareObjectsById(
      const ObjectId& one,
      const ObjectId& two) = 0;

  /**
   * Determines whether two RootIds resolve to the same Root object.
   *
   * Similar to compareObjectsById, this allows the BackingStore to compare
   * RootIds using its knowledge of the encoding scheme.
   *
   * Returns ObjectComparison::Identical if the RootIds are known to point to
   * the same root, ObjectComparison::Different if they are known to point to
   * different roots, or ObjectComparison::Unknown if they must be fetched and
   * compared to know.
   */
  virtual ObjectComparison compareRootsById(
      const RootId& one,
      const RootId& two) = 0;

  struct GetRootTreeResult {
    /// The root tree object.
    TreePtr tree;
    /// The root tree's ID which can later be passed to getTree.
    ObjectId treeId;
  };

  /**
   * Return value of the getTree method.
   */
  struct GetTreeResult {
    /** The retrieved tree. */
    folly::not_null<TreePtr> tree;
    /** The fetch origin of the tree. */
    ObjectFetchContext::Origin origin;
  };

  /**
   * Return value of the getTreeAuxData method.
   */
  struct GetTreeAuxResult {
    /**
     * The retrieved tree aux data.
     */
    TreeAuxDataPtr treeAux;
    /** The fetch origin of the tree aux data. */
    ObjectFetchContext::Origin origin;
  };

  /**
   * Return value of the getBlob method.
   */
  struct GetBlobResult {
    /** The retrieved blob. */
    folly::not_null<BlobPtr> blob;
    /** The fetch origin of the blob. */
    ObjectFetchContext::Origin origin;
  };

  /**
   * Return value of the getBlobAuxData method.
   */
  struct GetBlobAuxResult {
    /**
     * The retrieved blob aux data.
     *
     * Setting this to nullptr will make ObjectStore::getBlobAuxData fallback to
     * fetching the blob from the BackingStore to compute the blob aux data.
     */
    BlobAuxDataPtr blobAux;
    /** The fetch origin of the blob aux data. */
    ObjectFetchContext::Origin origin;
  };

  /**
   * Return value of the getGlobFiles method.
   */
  struct GetGlobFilesResult {
    /**
     * The retrieved glob entries
     * This command is unimplemented on some backing store impls
     * and will return an error. This will trigger the client to fallback to
     * looking up the globs locally.
     */
    std::vector<std::string> globFiles;
    RootId rootId;
    bool isLocal = false;
  };

  virtual void periodicManagementTask() {}

  /**
   * Subclass of BackingStore will override these functions to record file
   * paths fetched. After startRecordingFetch() is called, the BackingStore
   * will record fetched file paths. stopRecordingFetch() will disable the
   * recording and return the fetched files since startRecordingFetch() is
   * called and clear the old records.
   *
   * Currently implemented in SaplingBackingStore.
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
  virtual ImmediateFuture<folly::Unit> importManifestForRoot(
      const RootId& /*rootId*/,
      const Hash20& /*manifest*/,
      const ObjectFetchContextPtr& /*context*/) {
    return folly::unit;
  }

  /**
   * If supported, returns the name of the underlying repo. The result name is
   * primarily for logging and may not be unique.
   */
  virtual std::optional<folly::StringPiece> getRepoName() {
    return std::nullopt;
  }

  /**
   * Returns a human-readable string representation of the given RootId for
   * display purposes. The default implementation hexlifies the raw bytes.
   */
  virtual std::string displayRootId(const RootId& rootId) {
    return folly::hexlify(rootId.value());
  }

  virtual int64_t dropAllPendingRequestsFromQueue() = 0;

 private:
  // Forbidden copy constructor and assignment operator
  BackingStore(BackingStore const&) = delete;
  BackingStore& operator=(BackingStore const&) = delete;

  /**
   * ObjectStore should be the only public place to access BackingStore.
   */
  friend class ObjectStore;

  /**
   * FilteredBackingStore also has underlying BackingStore and should have
   * access to the following functions to access the underlying BackingStore.
   */
  friend class FilteredBackingStore;

  /**
   * Return the root Tree corresponding to the passed in RootId.
   */
  virtual ImmediateFuture<GetRootTreeResult> getRootTree(
      const RootId& rootId,
      const ObjectFetchContextPtr& context) = 0;

  virtual ImmediateFuture<std::shared_ptr<TreeEntry>> getTreeEntryForObjectId(
      const ObjectId& objectId,
      TreeEntryType treeEntryType,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Fetch a tree from the backing store.
   *
   * Return the tree and where it was found.
   */
  virtual folly::SemiFuture<GetTreeResult> getTree(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Fetch the tree aux data from the backing store.
   *
   * Return the tree aux data and where it was found.
   */
  virtual folly::SemiFuture<GetTreeAuxResult> getTreeAuxData(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Fetch a blob from the backing store.
   *
   * Return the blob and where it was found.
   */
  virtual folly::SemiFuture<GetBlobResult> getBlob(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Fetch a blob from the backing store.
   *
   * Return the blob and where it was found.
   */
  virtual folly::coro::Task<GetBlobResult> co_getBlob(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Fetch the blob aux data from the backing store.
   *
   * Return the blob aux data and where it was found.
   */
  virtual folly::SemiFuture<GetBlobAuxResult> getBlobAuxData(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) = 0;

  /**
   * Fetch file paths matching the given glob suffixes
   *
   * Return the Glob result containing the list of file paths, dtype, and commit
   * If the implementing BackingStore does not impolement this method, it will
   * return an error. The caller should fallback to resolving globFiles locally
   * in this case.
   */
  virtual ImmediateFuture<GetGlobFilesResult> getGlobFiles(
      const RootId& id,
      const std::vector<std::string>& globs,
      const std::vector<std::string>& prefixes) = 0;

  /**
   * Prefetch all the blobs represented by the HashRange.
   *
   * The caller is responsible for making sure that the HashRange stays valid
   * for as long as the returned SemiFuture.
   */
  [[nodiscard]] virtual folly::SemiFuture<folly::Unit> prefetchBlobs(
      ObjectIdRange /*ids*/,
      const ObjectFetchContextPtr& /*context*/) {
    return folly::unit;
  }

  virtual void workingCopyParentHint(const RootId&) {}

  /**
   * Strip the ObjectId to a smaller representation for memory optimization.
   * For example, in SaplingBackingStore, this strips the path portion of the
   * ObjectId, keeping only the hash bytes.
   *
   * The default implementation returns a copy of the given id.
   */
  virtual ObjectId stripObjectId(const ObjectId& id) const {
    return id;
  }
};

/**
 * For the common case that a BackingStore has a one-to-one relationship between
 * its IDs and objects -- such as when objects are identified by a cryptograph
 * hash -- this base class provides an implementation of compareObjectsById and
 * compareRootsById.
 */
class BijectiveBackingStore : public BackingStore {
 public:
  ObjectComparison compareObjectsById(const ObjectId& one, const ObjectId& two)
      override {
    return one.bytesEqual(two) ? ObjectComparison::Identical
                               : ObjectComparison::Different;
  }

  ObjectComparison compareRootsById(const RootId& one, const RootId& two)
      override {
    return one == two ? ObjectComparison::Identical
                      : ObjectComparison::Different;
  }
};

} // namespace facebook::eden
