/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Dirstate.h"
#include "eden/fs/inodes/TreeEntryFileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/service/EdenMountHandler.h"
#include "eden/fs/store/ObjectStores.h"
#include "eden/fuse/MountPoint.h"

namespace {
struct DirectoryDelta {
  std::vector<facebook::eden::PathComponent> added;
  std::vector<facebook::eden::PathComponent> removed;
  std::vector<facebook::eden::PathComponent> modified;
};
}

namespace facebook {
namespace eden {

std::unique_ptr<HgStatus> Dirstate::getStatus() {
  // Find the modified directories in the overlay and compare them with what is
  // in the root tree.
  auto mountPoint = edenMount_->getMountPoint();
  auto modifiedDirectories = getModifiedDirectoriesForMount(edenMount_.get());
  if (modifiedDirectories->empty()) {
    // There are no changes in the overlay, so the status should be empty?
    return std::make_unique<HgStatus>(
        std::unordered_map<RelativePath, HgStatusCode>());
  }

  auto userChanges = userChanges_.rlock();
  std::unordered_map<RelativePathPiece, HgStatusCode> copyOfUserChanges(
      userChanges->begin(), userChanges->end());

  std::unordered_map<RelativePath, HgStatusCode> manifest;
  auto rootTree = edenMount_->getRootTree();
  auto objectStore = edenMount_->getObjectStore();
  for (auto& directory : *modifiedDirectories) {
    // Get the directory as a TreeInode.
    auto dirInode = mountPoint->getDirInodeForPath(directory);
    auto treeInode = std::dynamic_pointer_cast<TreeInode>(dirInode);
    DCHECK_NOTNULL(treeInode.get());

    // Get the directory as a Tree.
    auto tree = getTreeForDirectory(directory, rootTree.get(), objectStore);
    DCHECK_NOTNULL(tree.get());

    DirectoryDelta delta;
    computeDelta(*rootTree, *treeInode, delta);

    // Look at the delta and convert the results into HgStatuses.
    for (auto& addedPath : delta.added) {
      auto pathToEntry = directory + addedPath;
      auto result = userChanges->find(pathToEntry);
      if (result != userChanges->end()) {
        auto statusCode = result->second;
        if (statusCode == HgStatusCode::ADDED) {
          manifest.emplace(pathToEntry, HgStatusCode::ADDED);
        } else {
          LOG(ERROR) << "File in delta.added was not ADDED. Support this!";
        }
        copyOfUserChanges.erase(pathToEntry);
      } else {
        manifest.emplace(pathToEntry, HgStatusCode::NOT_TRACKED);
      }
    }

    // TODO(mbolin): It probably is not quite this simple.
    for (auto& modifiedPath : delta.modified) {
      auto pathToEntry = directory + modifiedPath;
      manifest.emplace(pathToEntry, HgStatusCode::MODIFIED);
      copyOfUserChanges.erase(pathToEntry);
    }

    // TODO(mbolin): Process the delta.removed collection.
  }

  // We should make sure that every entry in userChanges_ is accounted for in
  // the HgStatus that we return.
  for (auto& pair : copyOfUserChanges) {
    if (pair.second == HgStatusCode::ADDED) {
      manifest.emplace(RelativePath(pair.first), HgStatusCode::MISSING);
    } else {
      LOG(INFO) << "Leftover entry in copyOfUserChanges that is not handled: "
                << pair.first;
    }
  }

  return std::make_unique<HgStatus>(std::move(manifest));
}

bool hasMatchingAttributes(
    TreeEntry& treeEntry,
    TreeInode::Entry& treeInode,
    ObjectStore& objectStore,
    TreeInode& parent, // Has rlock
    const TreeInode::Dir& dir) {
  if (treeEntry.getMode() != treeInode.mode) {
    return false;
  }

  // TODO(t12183419): Once the file size is available in the TreeEntry,
  // compare file sizes before fetching SHA-1s.

  if (treeInode.materialized) {
    // If the the inode is materialized, then we cannot trust the Hash on the
    // TreeInode::Entry, so we must compare with the contents in the overlay.
    auto overlayInode =
        parent.lookupChildByNameLocked(&dir, treeEntry.getName());
    auto fileInode =
        std::dynamic_pointer_cast<TreeEntryFileInode>(overlayInode);
    auto overlaySHA1 = fileInode->getSHA1().get();
    auto blobSHA1 = objectStore.getSha1ForBlob(treeEntry.getHash());
    return overlaySHA1 == *blobSHA1;
  } else {
    auto optionalHash = treeInode.hash;
    DCHECK(optionalHash.hasValue()) << "non-materialized file must have a hash";
    return *optionalHash.get_pointer() == treeEntry.getHash();
  }
}

void Dirstate::computeDelta(
    const Tree& original,
    TreeInode& current,
    DirectoryDelta& delta) const {
  auto treeEntries = original.getTreeEntries();
  auto dir = current.getContents().rlock();
  auto& entries = dir->entries;

  auto baseIterator = treeEntries.begin();
  auto overlayIterator = entries.begin();
  auto baseEnd = treeEntries.end();
  auto overlayEnd = entries.end();
  if (baseIterator == baseEnd && overlayIterator == overlayEnd) {
    return;
  }

  while (true) {
    if (baseIterator == baseEnd) {
      // Remaining entries in overlayIterator should be added to delta.added.
      while (overlayIterator != overlayEnd) {
        delta.added.push_back(overlayIterator->first);
        ++overlayIterator;
      }
      break;
    } else if (overlayIterator == overlayEnd) {
      // Remaining entries in baseIterator should be added to delta.removed.
      while (baseIterator != baseEnd) {
        delta.removed.push_back((*baseIterator).getName());
        ++baseIterator;
      }
      break;
    }

    auto base = *baseIterator;
    auto overlayName = overlayIterator->first;
    // TODO(mbolin): Support directories! Currently, this logic only makes sense
    // if all of the entries are files.

    auto cmp = base.getName().stringPiece().compare(overlayName.stringPiece());
    if (cmp == 0) {
      if (!hasMatchingAttributes(
              base,
              *overlayIterator->second.get(),
              *edenMount_->getObjectStore(),
              current,
              *dir)) {
        delta.modified.push_back(base.getName());
      }
      baseIterator++;
      overlayIterator++;
    } else if (cmp < 0) {
      delta.removed.push_back(base.getName());
      baseIterator++;
    } else {
      delta.added.push_back(overlayName);
      overlayIterator++;
    }
  }
  return;
}

void Dirstate::add(RelativePathPiece path) {
  // TODO(mbolin): Verify that path corresponds to a regular file or symlink.
  applyUserStatusChange_(path, HgStatusCode::ADDED);
}

void Dirstate::applyUserStatusChange_(
    RelativePathPiece pathToFile,
    HgStatusCode code) {
  auto userChanges = userChanges_.wlock();

  if (code == HgStatusCode::ADDED) {
    // TODO(mbolin): Honor the detailed behavior described below.
    /*
     * Analogous to `hg add <path>`. Note that this can have one of several
     * possible outcomes:
     * 1. If the path does not exist in the working copy, return an error. (Note
     *    that this happens even if path is in the manifest as REMOVED. It will
     *    also happen in the case where the path is MISSING.)
     * 2. If the path refers to a directory, return an error. (Currently, the
     *    caller is responsible for enumerating the transitive set of files in
     *    the directory and invoking this method once for each file.)
     * 3. If the path was in the manifest as ADDED or MODIFIED, or was
     *    not in the manifest at all (implying it is a normal file), then return
     *    a warning as Hg does: "<path> already tracked!".
     * 4. If the path was in the manifest as REMOVED, then it will be removed
     *    from the manifest (transitioning it back to a normal file). However,
     *    if its TreeEntry differs at all from its entry in the parent snapshot,
     *    then it will be updated in the manifest as MODIFIED.
     * 5. Otherwise, the file must be in the manifest as NOT_TRACKED or IGNORED.
     *    In either case, it will be updated in the manifest as ADDED.
     */
    (*userChanges)[pathToFile.copy()] = code;
  }

  // TODO(mbolin): Make sure that all code paths that modify userChanges_
  // perform a save() like this.
  persistence_->save(*userChanges);
}

HgStatusCode HgStatus::statusForPath(RelativePath path) const {
  auto result = statuses_.find(path);
  if (result != statuses_.end()) {
    return result->second;
  } else {
    // TODO(mbolin): Verify that path is in the tree and throw if not?
    return HgStatusCode::CLEAN;
  }
}
}
}
