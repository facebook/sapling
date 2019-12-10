/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "folly/portability/Windows.h"

#include "eden/fs/model/Tree.h"
#include "eden/fs/win/mount/CurrentState.h"
#include "eden/fs/win/mount/StateDbNode.h"
#include "eden/fs/win/utils/StringConv.h"
#include "thrift/lib/cpp2/async/ResponseChannel.h"

namespace facebook {
namespace eden {

class DiffCallback;
class ObjectStore;
class CurrentState;
class DiffContext;

/**
 * GenerateStatus will parse the CurrentState and compute the status. The
 * algorithm here is little different than of Eden with FUSE, Eden with Prjfs
 * will loop over the CurrentState and only compare the entries present in it.
 * CurrentState has the list of all the entries modified and a complete status
 * could be generated from it.
 */
class GenerateStatus {
 public:
  /**
   * Constructor takes pointer to ObjectStore and CurrentState, plus mountPath,
   * which is required to compare the contents of the file.
   */
  explicit GenerateStatus(
      const ObjectStore* store,
      const CurrentState* state,
      WinAbsolutePathW mountPath,
      DiffCallback* callback,
      apache::thrift::ResponseChannelRequest* request)
      : store_{store},
        state_{state},
        mountPath_{std::move(mountPath)},
        callback_{callback},
        request_{request} {}

  GenerateStatus(const GenerateStatus&) = delete;
  GenerateStatus& operator=(const GenerateStatus&) = delete;

  GenerateStatus(GenerateStatus&&) = default;
  GenerateStatus& operator=(GenerateStatus&&) = default;

  /**
   * compute() will compute the status and pass it back in the
   * ctxPtr->callback. compute runs asynchronously and could be running even
   * after the function returns. The caller need to make sure the ctxPtr is
   * valid until the Future is complete.
   */

  FOLLY_NODISCARD folly::Future<folly::Unit> compute(
      std::shared_ptr<const Tree> tree);

 private:
  /**
   * This is a private recursive function which iterates over the CurrentState
   * Tree.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> compute(
      const WinRelativePathW& path,
      const std::shared_ptr<const Tree>& tree,
      DiffCallback* callback);

  /**
   * This function is used only in special cases, Mostly when a directory was
   * either deleted or renamed and then recreated. We ignore the keys under
   * a deleted key and may also remove them in the future versions. This is a
   * way to generate a status in such scenarios.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> computeCompareBoth(
      const WinRelativePathW& path,
      const std::shared_ptr<const Tree>& tree,
      DiffCallback* callback);

  /**
   * markAllScmSubEntriesRemoved iterates over all the backing store entries and
   * marks them removed. This function will not check the FS state. This
   * function will be called when FS doesn't have an equivalent entry.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> markAllScmSubEntriesRemoved(
      const WinRelativePathW& currentPath,
      DiffCallback* callback,
      const TreeEntry& scmEntry);

  /**
   * markAllFsSubEntriesAdded iterates over all the FS entries and
   * marks them as Added. This function will not check the backing store. This
   * function will be called when backing store doesn't the entry.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> markAllFsSubEntriesAdded(
      const WinRelativePathW& currentPath,
      DiffCallback* callback,
      const StateDirectoryEntry& dirEntry);

  /**
   * processBothPresent will be called for when the same entry is present in
   * both FS and Backing Store.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> processBothPresent(
      const WinRelativePathW& currentPath,
      DiffCallback* callback,
      const TreeEntry& scmEntry,
      const StateDirectoryEntry& dirEntry,
      bool compareBoth);

  /**
   * checkModified will fetch the SHA1 from backing store and compare it against
   * computed SHA1 from the FS. This is to make sure the file is really
   * modified and not that someone added and remove the contents.
   */
  FOLLY_NODISCARD folly::Future<bool> checkModified(
      const WinRelativePathW& currentPath,
      const TreeEntry& scmEntry);

  FOLLY_NODISCARD folly::Future<folly::Unit> addedEntry(
      const WinRelativePathW& currentPath,
      DiffCallback* callback,
      const StateDirectoryEntry& dirEntry);

  FOLLY_NODISCARD folly::Future<folly::Unit> removedEntry(
      const WinRelativePathW& currentPath,
      DiffCallback* callback,
      const TreeEntry& scmEntry);

  const ObjectStore* objectStore() const {
    return store_;
  }

  const CurrentState* currentState() const {
    return state_;
  }

  const WinAbsolutePathW& mountPath() const {
    return mountPath_;
  }

  const ObjectStore* store_;
  const CurrentState* state_;

  /**
   * mountPath_ is used to fetch the file contents from FS compare against the
   * backing store. The SHA1 is computed and compared from both.
   */
  const WinAbsolutePathW mountPath_;

  DiffCallback* callback_;

  apache::thrift::ResponseChannelRequest* request_;
};

} // namespace eden
} // namespace facebook
