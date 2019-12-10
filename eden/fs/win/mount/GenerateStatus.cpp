/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "GenerateStatus.h"

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/git/TopLevelIgnores.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/DiffCallback.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/win/mount/CurrentState.h"
#include "eden/fs/win/utils/FileUtils.h"
#include "eden/fs/win/utils/StringConv.h"
#include "folly/container/Array.h"
#include "folly/futures/Future.h"
#include "folly/logging/xlog.h"

/**
 * TODO(puneetk):
 * 1. GenerateStatus should hold the reference to the objects it needs during
 * its life time - ObjectStore and CurrentState.
 *
 * 2. Implement .gitignore support.
 *
 * 3. Handle resetParentCommit case: The current implementation doesn't take
 * care of the case where nothing is Materialized but the parent commit is
 * changed.
 */

namespace facebook {
namespace eden {

FOLLY_NODISCARD folly::Future<folly::Unit> GenerateStatus::addedEntry(
    const WinRelativePathW& currentPath,
    DiffCallback* callback,
    const StateDirectoryEntry& dirEntry) {
  // TODO: check if currentPath is ignored, and skip it if we were not requested
  // to report ignored paths.
  if (dirEntry.isDirectory()) {
    return markAllFsSubEntriesAdded(currentPath, callback, dirEntry);
  }
  callback->addedFile(RelativePathPiece(winToEdenPath(currentPath)));

  // The interface of this function is future based for consistency - we don't
  // do any async processing in this function.
  return folly::makeFuture();
}

FOLLY_NODISCARD folly::Future<folly::Unit> GenerateStatus::removedEntry(
    const WinRelativePathW& currentPath,
    DiffCallback* callback,
    const TreeEntry& scmEntry) {
  if (scmEntry.isTree()) {
    return (markAllScmSubEntriesRemoved(currentPath, callback, scmEntry));
  }
  callback->removedFile(RelativePathPiece(winToEdenPath(currentPath)));
  return folly::makeFuture();
}

FOLLY_NODISCARD folly::Future<folly::Unit>
GenerateStatus::markAllFsSubEntriesAdded(
    const WinRelativePathW& currentPath,
    DiffCallback* callback,
    const StateDirectoryEntry& dirEntry) {
  StateDbNode dirNode = currentState()->getDbNode(currentPath);
  auto subDirectoryEntries = dirNode.getDirectoryEntries();
  WinRelativePathW childPath;
  std::vector<FOLLY_NODISCARD folly::Future<folly::Unit>> futures;

  for (const auto& entry : subDirectoryEntries) {
    childPath = currentPath / entry.getName();
    futures.push_back(addedEntry(childPath, callback, entry));
  }
  return folly::collectAll(futures).thenValue([](auto&&) {});
}

FOLLY_NODISCARD folly::Future<folly::Unit>
GenerateStatus::markAllScmSubEntriesRemoved(
    const WinRelativePathW& currentPath,
    DiffCallback* callback,
    const TreeEntry& scmEntry) {
  return objectStore()
      ->getTree(scmEntry.getHash())
      .thenValue(
          [this, currentPath, callback](std::shared_ptr<const Tree> tree) {
            const auto& scmEntries = tree->getTreeEntries();
            WinRelativePathW childPath;
            std::vector<folly::Future<folly::Unit>> futures;

            for (const auto& entry : scmEntries) {
              childPath = currentPath /
                  multibyteToWideString<folly::StringPiece>(
                              entry.getName().stringPiece());
              futures.push_back(removedEntry(childPath, callback, entry));
            }
            folly::collectAll(futures).thenValue([](auto&&) {});
          });
}

FOLLY_NODISCARD folly::Future<bool> GenerateStatus::checkModified(
    const WinRelativePathW& currentPath,
    const TreeEntry& scmEntry) {
  const auto path = mountPath() / currentPath;

  //
  // TODO(puneetk): getFileSha1() is not a good implementation for large files.
  // We need to do two things - first, read the data in chunks and instead of
  // entire file. Plus Make the call async so the current thread doesn't have to
  // wait for it to complete.
  //
  Hash fileSha1 = getFileSha1(path.c_str());

  return objectStore()
      ->getBlobMetadata(scmEntry.getHash())
      .thenValue([fileSha1 = std::move(fileSha1)](BlobMetadata metadata) {
        return (fileSha1 != metadata.sha1);
      });
}

/**
 * If the ScmEntry is tree and direntry is Directory - we recurse without
 * marking anything.
 *
 * If the ScmEntry is tree and direntry is file - we add the file as
 * added. And we recursively add all the sub entries of ScmEntry as missing.
 *
 * If the ScmEntry is blob and direntry is Directory - we mark the Scmentry
 * missing and add all the sub entries of FS.
 *
 * If the ScmEntry is blob and direntry is file - we compare the content SHA1.
 * If it doesn't match we mark it modified.
 **/

FOLLY_NODISCARD folly::Future<folly::Unit> GenerateStatus::processBothPresent(
    const WinRelativePathW& currentPath,
    DiffCallback* callback,
    const TreeEntry& scmEntry,
    const StateDirectoryEntry& dirEntry,
    bool compareBoth) {
  if ((scmEntry.isTree()) && (dirEntry.isDirectory())) {
    return objectStore()
        ->getTree(scmEntry.getHash())
        .thenValue([this, currentPath, callback, compareBoth](
                       std::shared_ptr<const Tree> tree) {
          if (compareBoth) {
            return computeCompareBoth(currentPath, tree, callback);
          } else {
            return compute(currentPath, tree, callback);
          }
        });

  } else if ((scmEntry.isTree()) && (!dirEntry.isDirectory())) {
    callback->addedFile(RelativePathPiece{winToEdenPath(currentPath).c_str()});
    return markAllScmSubEntriesRemoved(currentPath, callback, scmEntry);

  } else if ((!scmEntry.isTree()) && (dirEntry.isDirectory())) {
    callback->removedFile(
        RelativePathPiece{winToEdenPath(currentPath).c_str()});
    return markAllFsSubEntriesAdded(currentPath, callback, dirEntry);

  } else if ((!scmEntry.isTree()) && (!dirEntry.isDirectory())) {
    // If the entry is Materialized, check if the contents are same.
    if (dirEntry.state() == EntryState::MATERIALIZED) {
      return checkModified(currentPath, scmEntry)
          .thenValue([currentPath, callback](bool isModified) {
            if (isModified) {
              callback->modifiedFile(
                  RelativePathPiece{winToEdenPath(currentPath).c_str()});
            }
          });
    }
  }

  // We have covered all the cases, we should not reach here.
  folly::assume_unreachable();
}

FOLLY_NODISCARD folly::Future<folly::Unit> GenerateStatus::compute(
    const WinRelativePathW& path,
    const std::shared_ptr<const Tree>& tree,
    DiffCallback* callback) {
  const auto dirNode = currentState()->getDbNode(path);
  const auto dirEntries = dirNode.getDirectoryEntries();
  std::vector<folly::Future<folly::Unit>> futures;

  for (const auto& dirEntry : dirEntries) {
    const auto scmEntry = tree->getEntryPtr(
        PathComponentPiece{winToEdenName(dirEntry.getName()).c_str()});

    if ((!scmEntry) && (dirEntry.state() != EntryState::REMOVED)) {
      //
      // If we don't have a source control entry for this file or directory,
      // it should be either a newly created file, which would have EntryState
      // as Materialized or it would have been removed in some previous
      // operation.
      //
      WinRelativePathW currentPath = path / dirEntry.getName();
      futures.push_back(addedEntry(currentPath, callback, dirEntry));
      DCHECK(dirEntry.state() == EntryState::MATERIALIZED);

    } else if (scmEntry && (dirEntry.state() == EntryState::REMOVED)) {
      // If the source control entry exist and the file dir was removed on FS
      // then report it removed.
      const WinRelativePathW currentPath = path / dirEntry.getName();
      futures.push_back(removedEntry(currentPath, callback, *scmEntry));

    } else if (scmEntry) {
      // TODO(puneetk): // We don't yet mark all parent directories materialized
      // when a file is modified, so here we recurse for irrespective of the
      // state. Once we add that we should update the above if as: if
      // ((scmEntry) && (dirEntry.state() == EntryState::MATERIALIZED))
      WinRelativePathW currentPath = path / dirEntry.getName();
      futures.push_back(processBothPresent(
          currentPath, callback, *scmEntry, dirEntry, dirEntry.wasDeleted()));
    }
  }

  // Convert vector of futures to one future<Unit>
  return folly::collectAll(std::move(futures)).thenValue([](auto&&) {});
}

FOLLY_NODISCARD folly::Future<folly::Unit> GenerateStatus::computeCompareBoth(
    const WinRelativePathW& path,
    const std::shared_ptr<const Tree>& tree,
    DiffCallback* callback) {
  CHECK(tree);
  const auto dirNode = currentState()->getDbNode(path);
  const auto dirEntries = dirNode.getDirectoryEntries();
  std::vector<folly::Future<folly::Unit>> futures;

  const auto& scmEntries = tree->getTreeEntries();

  size_t scmIdx = 0;
  size_t fsIdx = 0;
  while (true) {
    if (scmIdx >= scmEntries.size()) {
      if (fsIdx >= dirEntries.size()) {
        // All done
        break;
      }

      // This entry is present in FS but not in source control.
      if (dirEntries[fsIdx].state() != EntryState::REMOVED) {
        WinRelativePathW currentPath = path / dirEntries[fsIdx].getName();
        futures.push_back(addedEntry(currentPath, callback, dirEntries[fsIdx]));
        DCHECK(dirEntries[fsIdx].state() == EntryState::MATERIALIZED);
      }
      ++fsIdx;
    } else if (fsIdx >= dirEntries.size()) {
      // This entry is present in the SCM but not in the FS.

      const WinRelativePathW currentPath = path /
          multibyteToWideString(scmEntries[scmIdx].getName().stringPiece());
      futures.push_back(
          removedEntry(currentPath, callback, scmEntries[scmIdx]));
      ++scmIdx;
    } else if (
        scmEntries[scmIdx].getName() <
        PathComponent(winToEdenName(dirEntries[fsIdx].getName()))) {
      // This entry is present in the SCM but not in the FS.
      const WinRelativePathW currentPath = path /
          multibyteToWideString(scmEntries[scmIdx].getName().stringPiece());
      futures.push_back(
          removedEntry(currentPath, callback, scmEntries[scmIdx]));
      ++scmIdx;
    } else if (
        scmEntries[scmIdx].getName() >
        PathComponent(winToEdenName(dirEntries[fsIdx].getName()))) {
      // This entry is present in the FS but not in the SCM.

      if (dirEntries[fsIdx].state() != EntryState::REMOVED) {
        WinRelativePathW currentPath = path / dirEntries[fsIdx].getName();
        futures.push_back(addedEntry(currentPath, callback, dirEntries[fsIdx]));
        DCHECK(dirEntries[fsIdx].state() == EntryState::MATERIALIZED);
      }
      ++fsIdx;
    } else {
      const auto& scmEntry = scmEntries[scmIdx];
      const auto& dirEntry = dirEntries[fsIdx];

      DCHECK(
          dirEntries[fsIdx].getName() ==
          multibyteToWideString(scmEntries[scmIdx].getName().stringPiece()));

      if (dirEntry.state() == EntryState::REMOVED) {
        // If the source control entry exist and the FS entry was removed on
        // FS then report it removed.
        const WinRelativePathW currentPath = path / dirEntry.getName();
        futures.push_back(removedEntry(currentPath, callback, scmEntry));

      } else {
        auto currentPath = path / dirEntry.getName();
        futures.push_back(processBothPresent(
            currentPath, callback, scmEntry, dirEntry, true));
      }
      ++scmIdx;
      ++fsIdx;
    }
  }

  return folly::collectAll(std::move(futures)).thenValue([](auto&&) {});
}

FOLLY_NODISCARD folly::Future<folly::Unit> GenerateStatus::compute(
    std::shared_ptr<const Tree> tree) {
  return compute(L"", tree, callback_);
}

} // namespace eden
} // namespace facebook
