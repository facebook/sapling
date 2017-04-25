/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Dirstate.h"
#include <folly/EvictingCacheMap.h>
#include <folly/Format.h>
#include <folly/Unit.h>
#include <folly/experimental/StringKeyedUnorderedMap.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/fuse/MountPoint.h"
#include "eden/fs/inodes/DirstatePersistence.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/EdenMounts.h"
#include "eden/fs/inodes/FileData.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodeDiffCallback.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/git/GitIgnore.h"
#include "eden/fs/model/git/GitIgnorePattern.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/ObjectStores.h"

using folly::Future;
using folly::makeFuture;
using folly::StringKeyedUnorderedMap;
using folly::StringPiece;
using folly::Unit;
using facebook::eden::overlay::UserStatusDirective;

namespace {
/**
 * When comparing two file entries for equality from a source control
 * perspective, exclude these bits when comparing their mode_t values.
 * Specifically, we ignore the "group" and "others" permissions,
 * as well as setuid, setgid, and the restricted deletion flag.
 */
constexpr mode_t kIgnoredModeBits =
    (S_IRWXG | S_IRWXO | S_ISUID | S_ISGID | S_ISVTX);

/**
 * Represents file (non-directory) changes in a directory. This reflects:
 * - New file in directory.
 * - File removed from directory (possibly replaced with directory of same
 *   name).
 * - Subdirectory removed form directory (possibly replaced with file of same
 *   name).
 *
 * However, it does not reflect:
 * - New subdirectory in directory.
 */
struct DirectoryDelta {
  // The contents of each vector is sorted by compare().
  std::vector<facebook::eden::PathComponent> added;
  std::vector<facebook::eden::PathComponent> removed;
  std::vector<facebook::eden::PathComponent> modified;
  std::vector<facebook::eden::PathComponent> removedDirectories;
};
}

namespace facebook {
namespace eden {

std::ostream& operator<<(
    std::ostream& os,
    const DirstateAddRemoveError& status) {
  return os << status.errorMessage;
}

std::string HgStatus::toString() const {
  // Sort the entries in the map.
  std::vector<std::pair<RelativePath, StatusCode>> entries(
      statuses_.begin(), statuses_.end());
  std::sort(entries.begin(), entries.end());

  auto buf = folly::IOBuf::create(50 * entries.size());
  folly::io::Appender appender(buf.get(), /* growSize */ 1024);
  for (auto pair : entries) {
    appender.write(hgStatusCodeChar(pair.second));
    appender(" ");
    appender(pair.first.stringPiece());
    appender("\n");
  }

  return buf->moveToFbString().toStdString();
}

namespace {
/**
 * All entries added to the manifest must be under the `prefix`.
 */
template <typename RelativePathType>
void updateManifestWithDirectives(
    RelativePathPiece prefix,
    const std::unordered_map<RelativePathType, overlay::UserStatusDirective>*
        unaccountedUserDirectives,
    std::unordered_map<RelativePath, StatusCode>* manifest) {
  // We should make sure that every entry in userDirectives_ is accounted for in
  // the HgStatus that we return.
  for (auto& pair : *unaccountedUserDirectives) {
    if (!prefix.isParentDirOf(RelativePathPiece(pair.first))) {
      continue;
    }

    switch (pair.second) {
      case overlay::UserStatusDirective::Add:
        // The file was marked for addition, but no longer exists in the working
        // copy. The user should either restore the file or run `hg forget`.
        manifest->emplace(RelativePath(pair.first), StatusCode::MISSING);
        break;
      case overlay::UserStatusDirective::Remove:
        // The file was marked for removal, but it still exists in the working
        // copy without any modifications. Although it may seem strange, it
        // should still show up as REMOVED in `hg status` even though it is
        // still on disk.
        //
        // Note that even if the file matches an ignore pattern, we currently
        // report it just as REMOVED.  This matches mercurial's current
        // behavior, but in the future it would probably be nicer to add a code
        // for REMOVED+IGNORED.
        manifest->emplace(RelativePath(pair.first), StatusCode::REMOVED);
        break;
    }
  }
}

void processRemovedFile(
    RelativePath pathToEntry,
    std::unordered_map<RelativePath, StatusCode>* manifest,
    const std::unordered_map<RelativePath, overlay::UserStatusDirective>*
        userDirectives,
    std::unordered_map<RelativePathPiece, overlay::UserStatusDirective>*
        copyOfUserDirectives) {
  auto result = userDirectives->find(pathToEntry);
  if (result != userDirectives->end()) {
    auto statusCode = result->second;
    switch (statusCode) {
      case overlay::UserStatusDirective::Add:
        // TODO(mbolin): Is there any weird sequence of modifications with
        // adding/removed files matched by .hgignore that could lead to this
        // state?
        throw std::runtime_error(folly::sformat(
            "Invariant violation: The user has marked {} for addition, "
            "but it already exists in the manifest "
            "(and is currently removed from disk).",
            pathToEntry.stringPiece()));
      case overlay::UserStatusDirective::Remove:
        manifest->emplace(pathToEntry, StatusCode::REMOVED);
        break;
    }
    copyOfUserDirectives->erase(pathToEntry);
  } else {
    // The file is not present on disk, but the user never ran `hg rm`.
    manifest->emplace(pathToEntry, StatusCode::MISSING);
  }
}

// Short-term helper class until we implement gitignore
// handling more efficiently.
class IgnoreChecker {
 public:
  explicit IgnoreChecker(EdenMount* mountPoint) : mountPoint_(mountPoint) {}

  bool isIgnored(RelativePathPiece path) {
    // If a path's parent directory is ignored, it is ignored
    // (as long as it hasn't been explicitly added to the dirstate, which
    // we already check before calling IgnoreChecker).
    //
    // Note that this is a potentially expensive recursion.
    // We could memoize the results for directories.  (Although eventually we
    // should just store this data directly in the Inode objects.)
    if (path.stringPiece().empty()) {
      return false;
    }
    if (isIgnored(path.dirname())) {
      return true;
    }

    StringPiece piece = path.stringPiece();
    auto idx = piece.rfind('/');
    auto fileType = GitIgnore::TYPE_FILE;
    while (true) {
      StringPiece dirName;
      StringPiece childName;
      if (idx == StringPiece::npos) {
        dirName = StringPiece();
        childName = piece;
      } else {
        dirName = StringPiece(piece.begin(), idx);
        childName = StringPiece(piece.begin() + idx + 1, piece.end());
      }

      VLOG(5) << "Check ignored: \"" << childName << "\" in \"" << dirName
              << "\"";
      const GitIgnore* ignore = getIgnoreData(dirName);
      auto matchResult = ignore->match(
          RelativePathPiece(childName, detail::SkipPathSanityCheck()),
          fileType);
      if (matchResult == GitIgnore::INCLUDE) {
        // Explicitly included.  We don't need to check parent directories.
        return false;
      } else if (matchResult == GitIgnore::EXCLUDE) {
        return true;
      }

      if (idx == StringPiece::npos) {
        // We checked everything up to the root.  The path is not ignored.
        return false;
      }
      idx = dirName.rfind('/');
      fileType = GitIgnore::TYPE_DIR;
    }
  }

 private:
  const GitIgnore* getIgnoreData(StringPiece directory) {
    auto it = ignoreCache_.find(directory);
    if (it != ignoreCache_.end()) {
      return &(it->second);
    }

    GitIgnore ignore;
    loadIgnoreFile(directory, &ignore);
    auto ret = ignoreCache_.emplace(directory, std::move(ignore));
    return &(ret.first->second);
  }

  void loadIgnoreFile(StringPiece directory, GitIgnore* ignore) {
    // Ugh.  This is rather inefficient.
    auto ignorePath =
        RelativePath(directory) + PathComponentPiece(".gitignore");
    VLOG(4) << "Loading ignore file at \"" << ignorePath << "\"";
    FileInodePtr ignoreInode;
    try {
      ignoreInode = mountPoint_->getFileInodeBlocking(ignorePath);
    } catch (const std::system_error& ex) {
      if (ex.code().category() != std::system_category() ||
          (ex.code().value() != ENOENT && ex.code().value() != ENOTDIR)) {
        throw;
      }
    }

    if (!ignoreInode) {
      // No gitignore file to load.
      return;
    }

    auto data = ignoreInode->getOrLoadData();
    auto materializeFuture = data->ensureDataLoaded();
    // TODO: Use a future callback rather than blocking here
    materializeFuture.get();
    ignore->loadFile(data->readAll());
  }

  EdenMount* const mountPoint_{nullptr};
  folly::StringKeyedUnorderedMap<GitIgnore> ignoreCache_;
};

class ThriftStatusCallback : public InodeDiffCallback {
 public:
  explicit ThriftStatusCallback(
      const std::unordered_map<RelativePath, UserStatusDirective>&
          userDirectives)
      : data_{folly::construct_in_place, userDirectives} {}

  void ignoredFile(RelativePathPiece path) override {
    processChangedFile(
        path, UserStatusDirective::Add, StatusCode::ADDED, StatusCode::IGNORED);
  }
  void untrackedFile(RelativePathPiece path) override {
    processChangedFile(
        path,
        UserStatusDirective::Add,
        StatusCode::ADDED,
        StatusCode::NOT_TRACKED);
  }
  void removedFile(
      RelativePathPiece path,
      const TreeEntry& /* sourceControlEntry */) override {
    processChangedFile(
        path,
        UserStatusDirective::Remove,
        StatusCode::REMOVED,
        StatusCode::MISSING);
  }
  void modifiedFile(
      RelativePathPiece path,
      const TreeEntry& /* sourceControlEntry */) override {
    processChangedFile(
        path,
        UserStatusDirective::Remove,
        StatusCode::REMOVED,
        StatusCode::MODIFIED);
  }
  void diffError(RelativePathPiece path, const folly::exception_wrapper& ew)
      override {
    // TODO: It would be nice to have a mechanism to return error info as part
    // of the thrift result.
    LOG(WARNING) << "error computing status data for " << path << ": "
                 << folly::exceptionStr(ew);
  }

  /**
   * Extract the ThriftHgStatus object from this callback.
   *
   * This method should be called no more than once, as this destructively
   * moves the results out of the callback.  It should only be invoked after
   * the diff operation has completed.
   */
  ThriftHgStatus extractStatus() {
    ThriftHgStatus status;

    {
      auto data = data_.wlock();
      status.entries.swap(data->status);

      // Process any remaining user directives that weren't seen during the diff
      // walk.
      //
      // TODO: I believe this isn't really right, but it should be good enough
      // for initial testing.
      //
      // We really need to also check if these entries exist currently on
      // disk and in source control.  For files that are removed but exist on
      // disk we also need to check their ignored status.
      //
      // - UserStatusDirective::Add, exists on disk, and in source control:
      //   -> skip
      // - UserStatusDirective::Add, exists on disk, not in SCM, but ignored:
      //   -> ADDED
      // - UserStatusDirective::Add, not on disk or in source control:
      //   -> MISSING
      // - UserStatusDirective::Remove, exists on disk, and in source control:
      //   -> REMOVED
      // - UserStatusDirective::Remove, exists on disk, not in SCM, but ignored:
      //   -> skip
      // - UserStatusDirective::Remove, not on disk, not in source control:
      //   -> skip
      for (const auto& entry : data->userDirectives) {
        auto hgStatusCode = (entry.second == UserStatusDirective::Add)
            ? StatusCode::MISSING
            : StatusCode::REMOVED;
        status.entries.emplace(entry.first.str(), hgStatusCode);
      }
    }

    return status;
  }

 private:
  /**
   * The implementation used for the ignoredFile(), untrackedFile(),
   * removedFile(), and modifiedFile().
   *
   * The logic is:
   * - If the file is present in userDirectives as userDirectiveType,
   *   then remove it from userDirectives and report the status as
   *   userDirectiveStatus.
   * - Otherwise, report the status as defaultStatus
   */
  void processChangedFile(
      RelativePathPiece path,
      UserStatusDirective userDirectiveType,
      StatusCode userDirectiveStatus,
      StatusCode defaultStatus) {
    auto data = data_.wlock();
    auto iter = data->userDirectives.find(path.stringPiece());
    if (iter != data->userDirectives.end()) {
      if (iter->second == userDirectiveType) {
        data->status.emplace(path.stringPiece().str(), userDirectiveStatus);
        data->userDirectives.erase(iter);
        return;
      }
    }
    data->status.emplace(path.stringPiece().str(), defaultStatus);
  }

  struct Data {
    explicit Data(
        const std::unordered_map<RelativePath, UserStatusDirective>& ud) {
      for (const auto& entry : ud) {
        userDirectives.emplace(entry.first.stringPiece(), entry.second);
      }
    }

    std::map<std::string, StatusCode> status;
    StringKeyedUnorderedMap<UserStatusDirective> userDirectives;
  };
  folly::Synchronized<Data> data_;
};
} // unnamed namespace

Dirstate::Dirstate(EdenMount* mount)
    : mount_(mount),
      persistence_(mount->getConfig()->getDirstateStoragePath()) {
  auto loadedData = persistence_.load();
  userDirectives_.wlock()->swap(loadedData);
}

Dirstate::~Dirstate() {}

ThriftHgStatus Dirstate::getStatus(bool listIgnored) const {
  ThriftStatusCallback callback(*userDirectives_.rlock());
  mount_->diff(&callback, listIgnored).get();
  return callback.extractStatus();
}

std::unique_ptr<HgStatus> Dirstate::getStatusForExistingDirectory(
    RelativePathPiece directory) const {
  std::unordered_set<RelativePathPiece> toIgnore;
  if (directory.empty()) {
    toIgnore.insert(RelativePathPiece(".hg"));
    toIgnore.insert(RelativePathPiece{kDotEdenName});
  }

  // Find the modified directories in the overlay and compare them with what is
  // in the root tree.
  auto modifiedDirectories =
      getModifiedDirectories(mount_, directory, &toIgnore);
  std::unordered_map<RelativePath, StatusCode> manifest;
  if (modifiedDirectories.empty()) {
    auto userDirectives = userDirectives_.rlock();
    updateManifestWithDirectives(directory, &*userDirectives, &manifest);
    return std::make_unique<HgStatus>(std::move(manifest));
  }

  auto userDirectives = userDirectives_.rlock();
  std::unordered_map<RelativePathPiece, overlay::UserStatusDirective>
      copyOfUserDirectives(userDirectives->begin(), userDirectives->end());

  // TODO: This code is somewhat inefficient.
  // We ideally should restructure this so that we can compute diff and ignore
  // data in a single pass through the tree.  (We shouldn't have separate walks
  // in getModifiedDirectoriesForMount(), then the for loop below, plus
  // additional computation to look up ignore data.)
  //
  // Doing this all at once would also make it possible to completely skip
  // ignored directories that have no tracked files inside of them.  Currently
  // we can't do this.  getModifiedDirectoriesForMount() has to descend into
  // ignored directories because it doesn't know if userDirectives_ contains
  // entries for files in these directories or not.
  IgnoreChecker ignoreChecker(mount_);

  auto rootTree = mount_->getRootTree();
  for (auto& directory : modifiedDirectories) {
    // Get the directory as a TreeInode.
    auto treeInode = mount_->getTreeInodeBlocking(directory);
    DCHECK(treeInode.get() != nullptr) << "Failed to get a TreeInode for "
                                       << directory;

    // Get the directory as a Tree.
    auto tree = getTreeForDirectory(
        directory, rootTree.get(), mount_->getObjectStore());
    DirectoryDelta delta;
    std::vector<TreeEntry> emptyEntries;
    // Note that if tree is NULL, then the directory must be new in the working
    // copy because there is no corresponding Tree in the manifest. Defining
    // treeEntries in this way avoids a heap allocation when tree is NULL.
    const auto* treeEntries = tree ? &tree->getTreeEntries() : &emptyEntries;
    computeDelta(treeEntries, *treeInode, delta);

    for (auto& removedDirectory : delta.removedDirectories) {
      // Must find the Tree that corresponds to removedDirectory and add
      // everything under it as REMOVED or MISSING in the manifest, as
      // appropriate.
      auto entry = tree->getEntryPtr(removedDirectory);
      auto subdirectory = directory + removedDirectory;
      DCHECK(entry != nullptr) << "Failed to find TreeEntry for "
                               << subdirectory;
      DCHECK(entry->getType() == TreeEntryType::TREE)
          << "Removed directory " << subdirectory
          << " did not correspond to a Tree.";
      auto removedTree = mount_->getObjectStore()->getTree(entry->getHash());
      addDeletedEntries(
          removedTree.get(),
          subdirectory,
          &manifest,
          &*userDirectives,
          &copyOfUserDirectives);
    }

    // Files in delta.added fall into one of three categories:
    // 1. ADDED
    // 2. NOT_TRACKED
    // 3. IGNORED
    for (auto& addedPath : delta.added) {
      auto pathToEntry = directory + addedPath;
      auto result = userDirectives->find(pathToEntry);
      if (result != userDirectives->end()) {
        auto statusCode = result->second;
        switch (statusCode) {
          case overlay::UserStatusDirective::Add:
            manifest.emplace(pathToEntry, StatusCode::ADDED);
            break;
          case overlay::UserStatusDirective::Remove:
            // TODO(mbolin): Is there any weird sequence of modifications with
            // adding/removed files matched by .hgignore that could lead to this
            // state?
            throw std::runtime_error(folly::sformat(
                "Invariant violation: The user has marked {} for removal, "
                "but it does not exist in the manifest.",
                pathToEntry.stringPiece()));
        }
        copyOfUserDirectives.erase(pathToEntry);
      } else if (ignoreChecker.isIgnored(pathToEntry)) {
        manifest.emplace(pathToEntry, StatusCode::IGNORED);
      } else {
        manifest.emplace(pathToEntry, StatusCode::NOT_TRACKED);
      }
    }

    // Files in delta.modified fall into one of two categories:
    // 1. MODIFIED
    // 2. REMOVED
    //
    // Technically, a file might be both REMOVED and IGNORED if the user asked
    // us to remove the file from source control tracking, but it still exists
    // in the file system, and it matches an ignore pattern.  Unfortunately
    // mercurial doesn't really represent the state properly in this case, and
    // just lists the file REMOVED, and not IGNORED.  (This does result in some
    // bugs: "hg clean --all" won't actually delete the file in this case.)
    //
    // Eventually it might be nice if we represent this state properly in our
    // thrift API.  For now though the mercurial code can't make use of it
    // properly though.
    for (auto& modifiedPath : delta.modified) {
      auto pathToEntry = directory + modifiedPath;
      auto result = userDirectives->find(pathToEntry);
      if (result != userDirectives->end()) {
        auto statusCode = result->second;
        switch (statusCode) {
          case overlay::UserStatusDirective::Add:
            // TODO(mbolin): Is there any weird sequence of modifications with
            // adding/removed files matched by .hgignore that could lead to this
            // state?
            throw std::runtime_error(folly::sformat(
                "Invariant violation: The user has marked {} for addition, "
                "but it already exists in the manifest.",
                pathToEntry.stringPiece()));
          case overlay::UserStatusDirective::Remove:
            manifest.emplace(pathToEntry, StatusCode::REMOVED);
            break;
        }
        copyOfUserDirectives.erase(pathToEntry);
      } else {
        manifest.emplace(pathToEntry, StatusCode::MODIFIED);
      }
    }

    // Files in delta.removed fall into one of two categories:
    // 1. REMOVED
    // 2. MISSING
    for (auto& removedPath : delta.removed) {
      auto pathToEntry = directory + removedPath;
      processRemovedFile(
          pathToEntry, &manifest, &*userDirectives, &copyOfUserDirectives);
    }
  }

  updateManifestWithDirectives(directory, &copyOfUserDirectives, &manifest);

  return std::make_unique<HgStatus>(std::move(manifest));
}

void Dirstate::addDeletedEntries(
    const Tree* tree,
    RelativePathPiece pathToTree,
    std::unordered_map<RelativePath, StatusCode>* manifest,
    const std::unordered_map<RelativePath, overlay::UserStatusDirective>*
        userDirectives,
    std::unordered_map<RelativePathPiece, overlay::UserStatusDirective>*
        copyOfUserDirectives) const {
  for (auto& entry : tree->getTreeEntries()) {
    auto pathToEntry = pathToTree + entry.getName();
    if (entry.getType() == TreeEntryType::BLOB) {
      processRemovedFile(
          pathToEntry, manifest, userDirectives, copyOfUserDirectives);
    } else {
      auto subtree = mount_->getObjectStore()->getTree(entry.getHash());
      addDeletedEntries(
          subtree.get(),
          pathToEntry,
          manifest,
          userDirectives,
          copyOfUserDirectives);
    }
  }
}

/**
 * Assumes that treeEntry and treeInode correspond to the same path. Returns
 * true if both the mode_t and file contents match for treeEntry and treeInode.
 */
bool hasMatchingAttributes(
    const TreeEntry* treeEntry,
    const TreeInode::Entry* inodeEntry,
    ObjectStore* objectStore,
    TreeInode& parent, // Has rlock
    const TreeInode::Dir& dir) {
  // As far as comparing mode bits is concerned, we ignore the "group" and
  // "other" permissions.
  if (((treeEntry->getMode() ^ inodeEntry->mode) & ~kIgnoredModeBits) != 0) {
    return false;
  }

  // TODO(t12183419): Once the file size is available in the TreeEntry,
  // compare file sizes before fetching SHA-1s.

  if (inodeEntry->isMaterialized()) {
    // If the the inode is materialized, then we cannot trust the Hash on the
    // TreeInode::Entry, so we must compare with the contents in the overlay.
    auto fileInode = dynamic_cast<FileInode*>(inodeEntry->inode);
    auto overlaySHA1 = fileInode->getSHA1().get();
    auto blobSHA1 = objectStore->getSha1ForBlob(treeEntry->getHash());
    return overlaySHA1 == blobSHA1;
  } else {
    return inodeEntry->getHash() == treeEntry->getHash();
  }
}

/**
 * @return true if `mode` corresponds to a file (regular or symlink) as opposed
 *   to a directory.
 */
inline bool isFile(mode_t mode) {
  return S_ISREG(mode) || S_ISLNK(mode);
}

void Dirstate::computeDelta(
    const std::vector<TreeEntry>* treeEntries,
    TreeInode& current,
    DirectoryDelta& delta) const {
  auto dir = current.getContents().rlock();
  auto& entries = dir->entries;

  auto baseIterator = treeEntries->begin();
  auto overlayIterator = entries.begin();
  auto baseEnd = treeEntries->end();
  auto overlayEnd = entries.end();
  if (baseIterator == baseEnd && overlayIterator == overlayEnd) {
    return;
  }

  while (true) {
    if (baseIterator == baseEnd) {
      // Each remaining entry in overlayIterator should be added to delta.added
      // (unless it is a directory).
      while (overlayIterator != overlayEnd) {
        auto mode = overlayIterator->second.get()->mode;
        if (isFile(mode)) {
          delta.added.push_back(overlayIterator->first);
        }
        ++overlayIterator;
      }
      break;
    } else if (overlayIterator == overlayEnd) {
      // Each remaining entry in baseIterator should be added to delta.removed
      // (unless it is a directory).
      while (baseIterator != baseEnd) {
        const auto& base = *baseIterator;
        auto mode = base.getMode();
        if (isFile(mode)) {
          delta.removed.push_back(base.getName());
        } else {
          delta.removedDirectories.push_back(base.getName());
        }
        ++baseIterator;
      }
      break;
    }

    const auto& base = *baseIterator;
    auto overlayName = overlayIterator->first;
    auto cmp = base.getName().stringPiece().compare(overlayName.stringPiece());
    if (cmp == 0) {
      // There are entries in the base commit and the overlay with the same
      // name. All four of the following are possible:
      // 1. Both entries correspond to files.
      // 2. Both entries correspond to directories.
      // 3. The entry was a file in the base commit but is now a directory.
      // 4. The entry was a directory in the base commit but is now a file.
      auto isFileInBase = isFile((*baseIterator).getMode());
      auto isFileInOverlay = isFile(overlayIterator->second.get()->mode);

      if (isFileInBase && isFileInOverlay) {
        if (!hasMatchingAttributes(
                &base,
                overlayIterator->second.get(),
                mount_->getObjectStore(),
                current,
                *dir)) {
          delta.modified.push_back(base.getName());
        }
      } else if (isFileInBase) {
        // It was a file in the base, but now is a directory in the overlay.
        // Hg should consider this file to be missing/removed.
        delta.removed.push_back(base.getName());
      } else if (isFileInOverlay) {
        // It was a directory in the base, but now is a file in the overlay.
        // Hg should consider this file to be added/untracked while the
        // directory's contents should be considered removed.
        delta.added.push_back(base.getName());
        delta.removedDirectories.push_back(base.getName());
      }

      baseIterator++;
      overlayIterator++;
    } else if (cmp < 0) {
      auto mode = base.getMode();
      if (isFile(mode)) {
        delta.removed.push_back(base.getName());
      } else {
        delta.removedDirectories.push_back(base.getName());
      }
      baseIterator++;
    } else {
      auto mode = overlayIterator->second.get()->mode;
      if (isFile(mode)) {
        delta.added.push_back(overlayName);
      }
      overlayIterator++;
    }
  }
  return;
}

namespace {
enum AddAction {
  Add,
  Erase,
};

void addDirstateAddRemoveError(
    RelativePathPiece path,
    StringPiece formatError,
    std::vector<DirstateAddRemoveError>* errorsToReport) {
  errorsToReport->push_back(DirstateAddRemoveError{
      path.copy(), folly::sformat(formatError, path.stringPiece())});
}

void assignAddAction(
    RelativePathPiece path,
    StatusCode code,
    std::unordered_map<RelativePath, AddAction>& actions) {
  if (code == StatusCode::NOT_TRACKED) {
    actions[path.copy()] = AddAction::Add;
  } else if (code == StatusCode::REMOVED) {
    actions[path.copy()] = AddAction::Erase;
  }
  // TODO(mbolin): Should we do anything for the other statuses? Do we
  // need to complain or anything like that?
}

enum WorkingCopyStatus { File, Directory, DoesNotExist, MagicPath };

static bool isMagicPath(RelativePathPiece path) {
  // If any component of the path name is .eden, then this path is a magic
  // path that we won't allow to be checked in or show up in the dirstate.
  for (auto p : path.paths()) {
    if (p.basename().stringPiece() == kDotEdenName) {
      return true;
    }
  }
  return false;
}

WorkingCopyStatus getPathStatus(
    RelativePathPiece path,
    const EdenMount* mount) {
  try {
    // If any component of the path name is .eden, then this path is a magic
    // path that we won't allow to be checked in or show up in the dirstate.
    if (isMagicPath(path)) {
      return WorkingCopyStatus::MagicPath;
    }

    // Use getInodeBlocking() as a test of whether the path exists.
    auto inodeBase = mount->getInodeBlocking(path);
    if (inodeBase.asFilePtrOrNull() != nullptr) {
      return WorkingCopyStatus::File;
    } else {
      return WorkingCopyStatus::Directory;
    }
  } catch (const std::system_error& e) {
    if (e.code().value() != ENOENT) {
      throw;
    } else {
      return WorkingCopyStatus::DoesNotExist;
    }
  }
}
}

void Dirstate::addAll(
    const std::vector<RelativePathPiece>& paths,
    std::vector<DirstateAddRemoveError>* errorsToReport) {
  // Find all of the untracked files and then update userDirectives, as
  // appropriate.
  std::unordered_map<RelativePath, AddAction> actions;
  for (auto& path : paths) {
    auto pathStatus = getPathStatus(path, mount_);
    if (pathStatus == WorkingCopyStatus::File) {
      // Admittedly, this getStatusForExistingDirectory() call will also
      // traverse subdirectories of path.dirname(), so it will do some extra
      // work. Similarly, if paths contains a list of files in the same
      // directory, getStatusForExistingDirectory() will be called once per
      // file instead of once for all files in that directory. If this turns
      // out to be a bottleneck, then we can do some extra bookkeeping to
      // reduce lookups.
      auto status = getStatusForExistingDirectory(path.dirname());
      auto code = status->statusForPath(path);
      assignAddAction(path, code, actions);
    } else if (pathStatus == WorkingCopyStatus::Directory) {
      auto status = getStatusForExistingDirectory(path);
      for (auto& pair : *status->list()) {
        // Only attempt to process the entry if it corresponds to a file in the
        // working copy.
        auto entryStatus = getPathStatus(pair.first, mount_);
        if (entryStatus == WorkingCopyStatus::File) {
          assignAddAction(pair.first, pair.second, actions);
        }
      }
    } else if (pathStatus == WorkingCopyStatus::DoesNotExist) {
      addDirstateAddRemoveError(
          path, "{}: No such file or directory", errorsToReport);
    } else if (pathStatus == WorkingCopyStatus::MagicPath) {
      addDirstateAddRemoveError(
          path, "{}: cannot be part of a commit", errorsToReport);
    } else {
      throw std::runtime_error("Unhandled enum value");
    }
  }

  // Apply all of the updates to userDirectives in one go.
  if (!actions.empty()) {
    auto userDirectives = userDirectives_.wlock();
    for (auto& pair : actions) {
      auto action = pair.second;
      switch (action) {
        case AddAction::Add:
          (*userDirectives)[pair.first] = overlay::UserStatusDirective::Add;
          break;
        case AddAction::Erase:
          userDirectives->erase(pair.first);
          break;
      }
    }
    persistence_.save(*userDirectives);
  }
}

namespace {
enum ShouldBeDeleted {
  YES,
  NO_BECAUSE_THE_FILE_WAS_ALREADY_DELETED,
  NO_BECAUSE_THE_FILE_WAS_MODIFIED,
};
}

/**
 * We need to delete the file from the working copy if either of the following
 * hold (note that it is a precondition that the file exists):
 * 1. The file is not materialized in the overlay, so it is unmodified.
 * 2. The file is in the overlay, but matches what is in the manifest.
 */
ShouldBeDeleted shouldFileBeDeletedByHgRemove(
    RelativePathPiece file,
    TreeInodePtr treeInode,
    const TreeEntry* treeEntry,
    ObjectStore* objectStore,
    std::vector<DirstateAddRemoveError>* errorsToReport) {
  if (treeInode == nullptr) {
    // The parent directory for the file is not in the overlay, so the file must
    // not have been modified. As such, `hg remove` should result in deleting
    // the file.
    return ShouldBeDeleted::YES;
  }

  auto name = file.basename();
  auto dir = treeInode->getContents().rlock();
  auto& entries = dir->entries;
  for (auto& entry : entries) {
    if (entry.first == name) {
      if (hasMatchingAttributes(
              treeEntry, entry.second.get(), objectStore, *treeInode, *dir)) {
        return ShouldBeDeleted::YES;
      } else {
        addDirstateAddRemoveError(
            file,
            "not removing {}: file is modified (use -f to force removal)",
            errorsToReport);
        return ShouldBeDeleted::NO_BECAUSE_THE_FILE_WAS_MODIFIED;
      }
    }
  }

  // If we have reached this point, then the file has already been removed. Note
  // that this line of code should be unreachable given the preconditions of
  // this function, but there could be a race condition where the file is
  // deleted after this function is entered and before we reach this line of
  // code, so we return false here just to be safe.
  return ShouldBeDeleted::NO_BECAUSE_THE_FILE_WAS_ALREADY_DELETED;
}

void collectAllPathsUnderTree(
    const Tree* directory,
    RelativePathPiece directoryName,
    const ObjectStore* objectStore,
    folly::EvictingCacheMap<RelativePath, folly::Unit>& collection) {
  for (auto& entry : directory->getTreeEntries()) {
    auto entryPath = directoryName + entry.getName();
    if (entry.getFileType() != FileType::DIRECTORY) {
      collection.set(entryPath, folly::unit);
    } else {
      auto tree = objectStore->getTree(entry.getHash());
      collectAllPathsUnderTree(tree.get(), entryPath, objectStore, collection);
    }
  }
}

void Dirstate::removeAll(
    const std::vector<RelativePathPiece>& paths,
    bool force,
    std::vector<DirstateAddRemoveError>* errorsToReport) {
  // First, let's collect all of the files to remove based on the `paths`
  // argument. We use an EvictingCacheMap as a set that preserves iteration
  // order.
  folly::EvictingCacheMap<RelativePath, folly::Unit> pathsToRemove(0);
  auto rootTree = mount_->getRootTree();
  auto objectStore = mount_->getObjectStore();
  for (auto& path : paths) {
    if (isMagicPath(path)) {
      addDirstateAddRemoveError(
          path, "{}: cannot be part of a commit", errorsToReport);
      continue;
    }

    // A file (or directory) must be tracked in order for it to be
    // removed, though it does not need to exist on disk (it could be in the
    // MISSING state when this is called, for example).
    auto entry = getEntryForPath(path, rootTree.get(), objectStore);
    if (entry != nullptr) {
      if (entry->getFileType() == FileType::DIRECTORY) {
        // This should take action on every file under entry, as well as any
        // files that are tracked as ADDED under path. Note that removing a
        // file that is tracked as ADDED will either trigger a "not removing:
        // file has been marked for add" error, or it will perform the remove if
        // --force has been specified. We leave that up to ::remove() to decide.
        auto directory = objectStore->getTree(entry->getHash());
        collectAllPathsUnderTree(
            directory.get(), path, objectStore, pathsToRemove);
        {
          auto userDirectives = userDirectives_.rlock();
          for (auto& pair : *userDirectives) {
            // Note that if the path is already marked as "Remove" in
            // userDirectives, ::remove() is a noop, so we can filter out those
            // paths here.
            if (pair.second != overlay::UserStatusDirective::Remove &&
                path.isParentDirOf(pair.first)) {
              pathsToRemove.set(pair.first, folly::unit);
            }
          }
        }
      } else {
        pathsToRemove.set(path.copy(), folly::unit);
      }
    } else {
      // The path does not exist in the manifest (but it could be tracked in
      // userChanges!), so now we must check its local state to determine what
      // to do next.
      auto inodeBase = getInodeBaseOrNull(path);
      if (inodeBase != nullptr) {
        auto stat = inodeBase->getattr().get().st;
        if (S_ISDIR(stat.st_mode)) {
          // This is a case where the directory is exists in the working copy,
          // but not in the manifest. It may have entries in userDirectives that
          // could be affected by this `hg rm` call.
          {
            auto userDirectives = userDirectives_.rlock();
            for (auto& pair : *userDirectives) {
              if (path.isParentDirOf(pair.first)) {
                pathsToRemove.set(pair.first, folly::unit);
              }
            }
          }
        } else {
          // We let remove() determine whether path is untracked or not.
          pathsToRemove.set(path.copy(), folly::unit);
        }
      } else {
        // path might not exist in the manifest, but it is possible that it is
        // tracked and marked for addition. For example, the user may create a
        // file, run `hg add`, and then delete it before running `hg remove`.
        {
          auto userDirectives = userDirectives_.rlock();
          auto result = userDirectives->find(path.copy());
          if (result != userDirectives->end()) {
            // We let remove() determine whether path is untracked or not.
            pathsToRemove.set(path.copy(), folly::unit);
            continue;
          }
        }
        addDirstateAddRemoveError(
            path, "{}: No such file or directory", errorsToReport);
      }
    }
  }

  // TODO(mbolin): It would be advantageous to redesign remove() to work in
  // batches. Every call to remove() acquires the userDirectives lock and
  // potentially writes to the persistence layer. It would be better to do both
  // of those things once for the entire set of paths instead of once per path.
  for (auto& pair : pathsToRemove) {
    remove(pair.first, force, errorsToReport);
  }

  // TODO(mbolin): If one of the original paths corresponds to a directory and
  // now that directory is empty (or contains only empty directories), then it
  // should also be removed. Note that this is not guaranteed to be the case
  // here because an individual file in the directory may have failed to have
  // been removed because it was modified, ignored, etc.
  // On further consideration, this is not specific to directory arguments. If
  // `hg rm <path>` is called and <path> is the last file in the directory
  // (other than the root), then the directory should be removed.
}

void Dirstate::remove(
    RelativePathPiece path,
    bool force,
    std::vector<DirstateAddRemoveError>* errorsToReport) {
  /*
   * Analogous to `hg rm <path>`. Note that the caller is responsible for
   * ensuring that `path` satisfies at least one of the following requirements:
   * a. The path corresponds to a file (non-directory) in the working copy.
   * b. The path corresponds to a file (non-directory) the manifest.
   * c. The path corresponds to a file (non-directory) in userDirectives.
   *
   * Note that this can have one of several possible outcomes:
   * 1. If the path is in the manifest but not in userDirectives, then it should
   *    be marked as REMOVED, but there are several cases to consider:
   *    a. It has already been removed from the working copy. If the user ran
   *      `hg status` right now, the file would be reported as MISSING at this
   *      point. Regardless, it should now be set to REMOVED in userDirectives.
   *    b. It exists in the working copy and matches what is in the manifest.
   *      In this case, it should be set to REMOVED in userDirectives and
   *      removed from the working copy.
   *    c. It has local changes in the working copy. In this case, nothing
   *      should be modified and an error should be returned:
   *      "not removing: file is modified (use -f to force removal)".
   * 2. If the path is in userDirectives as REMOVED, then this should be a noop.
   *    In particular, even if the user has performed the following sequence:
   *    $ hg rm a-file.txt   # deletes a-file.txt
   *    $ echo random-stuff > a-file.txt
   *    $ hg rm a-file.txt   # leaves a-file.txt alone
   *    The second call to `hg rm` should not delete a-file.txt even though
   *    the first one did. It should not return an error that the contents have
   *    changed, either.
   * 3. If the path is in userDirectives as ADD, then there are two
   *    possibilities:
   *    a. If the file exists, then no action is taken and an error should be
   *      returned:
   *      "not removing: file has been marked for add "
   *      "(use 'hg forget' to undo add)".
   *    b. If the file does not exist, then technically, it is MISSING rather
   *      than ADDED at this point. Regardless, now its entry should be removed
   *      from userDirectives.
   */
  // We look up the InodeBase and TreeEntry for `path` before acquiring the
  // write lock for userDirectives_ because these lookups could be slow, so we
  // prefer not to do them while holding the lock.
  TreeInodePtr parent;
  try {
    parent = mount_->getTreeInodeBlocking(path.dirname());
  } catch (const std::system_error& e) {
    auto value = e.code().value();
    if (value != ENOENT && value != ENOTDIR) {
      throw;
    }
  }

  InodePtr inode;
  if (parent != nullptr) {
    try {
      inode = parent->getChildByName(path.basename()).get();
    } catch (const std::system_error& e) {
      if (e.code().value() != ENOENT) {
        throw;
      }
    }
  }

  auto entry = getEntryForFile(
      path, mount_->getRootTree().get(), mount_->getObjectStore());

  auto shouldDelete = false;
  {
    auto userDirectives = userDirectives_.wlock();
    auto result = userDirectives->find(path.copy());
    if (result == userDirectives->end()) {
      // When there is no entry for the file in userChanges, we find the
      // corresponding TreeEntry in the manifest and compare it to its Entry in
      // the Overlay, if it exists.
      if (entry == nullptr) {
        addDirstateAddRemoveError(
            path, "not removing {}: file is untracked", errorsToReport);
        return;
      }

      if (inode != nullptr) {
        if (force) {
          shouldDelete = true;
        } else {
          auto shouldBeDeleted = shouldFileBeDeletedByHgRemove(
              path,
              parent,
              entry.get(),
              mount_->getObjectStore(),
              errorsToReport);
          switch (shouldBeDeleted) {
            case ShouldBeDeleted::YES:
              shouldDelete = true;
              break;
            case ShouldBeDeleted::NO_BECAUSE_THE_FILE_WAS_ALREADY_DELETED:
              // We still need to update userDirectives to mark the file as
              // removed in this case.
              break;
            case ShouldBeDeleted::NO_BECAUSE_THE_FILE_WAS_MODIFIED:
              // If the file was modified, then it should not even be marked as
              // removed. An error should have been added to errorsToReport, so
              // we just abort the entire method at this point.
              return;
          }
        }
      }
      (*userDirectives)[path.copy()] = overlay::UserStatusDirective::Remove;
      persistence_.save(*userDirectives);
    } else {
      switch (result->second) {
        case overlay::UserStatusDirective::Remove:
          // No-op: already removed.
          break;
        case overlay::UserStatusDirective::Add:
          if (inode != nullptr) {
            addDirstateAddRemoveError(
                path,
                "not removing {}: file has been marked for add "
                "(use 'hg forget' to undo add)",
                errorsToReport);
            return;
          } else {
            userDirectives->erase(path.copy());
            persistence_.save(*userDirectives);
          }
          break;
      }
    }
  }

  if (shouldDelete) {
    auto dispatcher = mount_->getDispatcher();
    try {
      dispatcher->unlink(parent->getNodeId(), path.basename()).get();
    } catch (const std::system_error& e) {
      // If the file has already been deleted, then mission accomplished.
      if (e.code().value() != ENOENT) {
        throw;
      }
    }
  }
}

InodePtr Dirstate::getInodeBaseOrNull(RelativePathPiece path) const {
  try {
    return mount_->getInodeBlocking(path);
  } catch (const std::system_error& e) {
    if (e.code().value() == ENOENT) {
      return nullptr;
    } else {
      throw;
    }
  }
}

Future<Unit> Dirstate::onSnapshotChanged(const Tree* rootTree) {
  auto* objectStore = mount_->getObjectStore();

  {
    auto userDirectives = userDirectives_.wlock();

    // TODO: It would be much nicer if we stored the user directives in a
    // tree-like structure, so we could avoid traversing the source control Tree
    // separately each time for every entry in userDirectives.
    auto iter = userDirectives->begin();
    bool madeChanges = false;
    while (iter != userDirectives->end()) {
      // If we need to erase this element, it will erase iterators pointing to
      // it, but other iterators will be unaffected.
      auto current = iter;
      ++iter;

      // Check to see if this entry exists in source control now.
      // TODO: We should look up the entry using a futures-based API.
      auto entry = getEntryForFile(current->first, rootTree, objectStore);
      auto actualStatus = entry ? overlay::UserStatusDirective::Add
                                : overlay::UserStatusDirective::Remove;
      if (current->second == actualStatus) {
        userDirectives->erase(current);
        madeChanges = true;
      }
    }

    if (madeChanges) {
      persistence_.save(*userDirectives);
    }
  }

  return makeFuture();
}

const char kStatusCodeCharClean = 'C';
const char kStatusCodeCharModified = 'M';
const char kStatusCodeCharAdded = 'A';
const char kStatusCodeCharRemoved = 'R';
const char kStatusCodeCharMissing = '!';
const char kStatusCodeCharNotTracked = '?';
const char kStatusCodeCharIgnored = 'I';

char hgStatusCodeChar(StatusCode code) {
  switch (code) {
    case StatusCode::CLEAN:
      return kStatusCodeCharClean;
    case StatusCode::MODIFIED:
      return kStatusCodeCharModified;
    case StatusCode::ADDED:
      return kStatusCodeCharAdded;
    case StatusCode::REMOVED:
      return kStatusCodeCharRemoved;
    case StatusCode::MISSING:
      return kStatusCodeCharMissing;
    case StatusCode::NOT_TRACKED:
      return kStatusCodeCharNotTracked;
    case StatusCode::IGNORED:
      return kStatusCodeCharIgnored;
  }
  throw std::runtime_error(folly::to<std::string>(
      "Unrecognized StatusCode: ",
      static_cast<typename std::underlying_type<StatusCode>::type>(code)));
}

StatusCode HgStatus::statusForPath(RelativePathPiece path) const {
  auto result = statuses_.find(path.copy());
  if (result != statuses_.end()) {
    return result->second;
  } else {
    // TODO(mbolin): Verify that path is in the tree and throw if not?
    return StatusCode::CLEAN;
  }
}

std::ostream& operator<<(std::ostream& os, const HgStatus& status) {
  os << status.toString();
  return os;
}
}
}
