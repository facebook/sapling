/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenMountHandler.h"

#include <folly/Range.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/TreeEntryFileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fuse/MountPoint.h"
#include "eden/fuse/fuse_headers.h"
#include "eden/utils/PathFuncs.h"

using folly::StringPiece;
using std::unique_ptr;

namespace facebook {
namespace eden {

void getMaterializedEntriesRecursive(
    std::map<std::string, FileInformation>& out,
    RelativePathPiece dirPath,
    TreeInode* dir);

void getMaterializedEntriesForMount(
    EdenMount* edenMount,
    MaterializedResult& out) {
  auto inodeDispatcher = edenMount->getMountPoint()->getDispatcher();
  auto rootInode = inodeDispatcher->getDirInode(FUSE_ROOT_ID);

  auto latest = edenMount->getJournal().rlock()->getLatest();

  out.currentPosition.mountGeneration = edenMount->getMountGeneration();
  out.currentPosition.sequenceNumber = latest->toSequence;
  out.currentPosition.snapshotHash =
      StringPiece(latest->toHash.getBytes()).str();

  auto treeInode = std::dynamic_pointer_cast<TreeInode>(rootInode);
  if (treeInode) {
    getMaterializedEntriesRecursive(
        out.fileInfo, RelativePathPiece(), treeInode.get());
  }
}

// Convert from a system timespec to our thrift TimeSpec
static inline void timespecToTimeSpec(const timespec& src, TimeSpec& dest) {
  dest.seconds = src.tv_sec;
  dest.nanoSeconds = src.tv_nsec;
}

void getMaterializedEntriesRecursive(
    std::map<std::string, FileInformation>& out,
    RelativePathPiece dirPath,
    TreeInode* dir) {
  dir->getContents().withRLock([&](const auto& contents) mutable {
    if (contents.materialized) {
      FileInformation dirInfo;
      auto attr = dir->getAttrLocked(&contents);

      dirInfo.mode = attr.st.st_mode;
      timespecToTimeSpec(attr.st.st_mtim, dirInfo.mtime);

      out[dirPath.value().toString()] = std::move(dirInfo);
    } else {
      return;
    }

    for (auto& entIter : contents.entries) {
      const auto& name = entIter.first;
      const auto& ent = entIter.second;

      if (!ent->materialized) {
        continue;
      }

      auto childInode = dir->lookupChildByNameLocked(&contents, name);
      auto childPath = dirPath + name;

      if (S_ISDIR(ent->mode)) {
        auto childDir = std::dynamic_pointer_cast<TreeInode>(childInode);
        DCHECK(childDir->getContents().rlock()->materialized)
            << (dirPath + name) << " entry " << ent.get()
            << " materialized is true, but the contained dir is !materialized";
        getMaterializedEntriesRecursive(out, childPath, childDir.get());
      } else {
        auto fileInode =
            std::dynamic_pointer_cast<TreeEntryFileInode>(childInode);
        auto attr = fileInode->getattr().get();

        FileInformation fileInfo;
        fileInfo.mode = attr.st.st_mode;
        fileInfo.size = attr.st.st_size;
        timespecToTimeSpec(attr.st.st_mtim, fileInfo.mtime);

        out[childPath.value().toStdString()] = std::move(fileInfo);
      }
    }
  });
}

void getModifiedDirectoriesRecursive(
    RelativePathPiece dirPath,
    TreeInode* dir,
    std::vector<RelativePath>* modifiedDirectories) {
  dir->getContents().withRLock([&](const auto& contents) mutable {
    if (!contents.materialized) {
      return;
    }

    modifiedDirectories->push_back(dirPath.copy());
    for (auto& entIter : contents.entries) {
      const auto& ent = entIter.second;
      if (S_ISDIR(ent->mode) && ent->materialized) {
        const auto& name = entIter.first;
        auto childInode = dir->lookupChildByNameLocked(&contents, name);
        auto childPath = dirPath + name;
        auto childDir = std::dynamic_pointer_cast<TreeInode>(childInode);
        DCHECK(childDir->getContents().rlock()->materialized)
            << (dirPath + name) << " entry " << ent.get()
            << " materialized is true, but the contained dir is !materialized";

        getModifiedDirectoriesRecursive(
            childPath, childDir.get(), modifiedDirectories);
      }
    }
  });
}

unique_ptr<std::vector<RelativePath>> getModifiedDirectoriesForMount(
    EdenMount* edenMount) {
  auto inodeDispatcher = edenMount->getMountPoint()->getDispatcher();
  auto rootInode = inodeDispatcher->getDirInode(FUSE_ROOT_ID);
  auto treeInode = std::dynamic_pointer_cast<TreeInode>(rootInode);
  if (treeInode) {
    auto modifiedDirectories = std::make_unique<std::vector<RelativePath>>();
    getModifiedDirectoriesRecursive(
        RelativePathPiece(), treeInode.get(), modifiedDirectories.get());
    return modifiedDirectories;
  } else {
    throw std::runtime_error(folly::to<std::string>(
        "Could not find root TreeInode for ", edenMount->getPath()));
  }
}
}
}
