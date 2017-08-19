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

#include <folly/Format.h>
#include <folly/MapUtil.h>
#include <folly/Range.h>
#include <folly/Unit.h>
#include <folly/experimental/StringKeyedUnorderedMap.h>
#include <folly/experimental/logging/xlog.h>

#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/fuse/MountPoint.h"
#include "eden/fs/inodes/DirstatePersistence.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodeDiffCallback.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ObjectStore.h"

using folly::Future;
using folly::makeFuture;
using folly::StringKeyedUnorderedMap;
using folly::StringPiece;
using folly::Unit;
using facebook::eden::hgdirstate::DirstateNonnormalFileStatus;
using facebook::eden::hgdirstate::DirstateMergeState;
using facebook::eden::hgdirstate::DirstateTuple;
using std::string;

namespace facebook {
namespace eden {

namespace {

class ThriftStatusCallback : public InodeDiffCallback {
 public:
  explicit ThriftStatusCallback(
      const folly::StringKeyedUnorderedMap<DirstateTuple>& hgDirstateTuples)
      : data_{folly::in_place, hgDirstateTuples} {}

  void ignoredFile(RelativePathPiece path) override {
    processChangedFile(
        path,
        DirstateNonnormalFileStatus::MarkedForAddition,
        StatusCode::ADDED,
        StatusCode::IGNORED);
  }

  void untrackedFile(RelativePathPiece path) override {
    auto data = data_.wlock();
    auto dirstateTuple =
        folly::get_ptr(data->hgDirstateTuples, path.stringPiece());
    auto statusCode = StatusCode::NOT_TRACKED;
    if (dirstateTuple != nullptr) {
      auto nnFileStatus = dirstateTuple->get_status();
      if (nnFileStatus == DirstateNonnormalFileStatus::MarkedForAddition) {
        statusCode = StatusCode::ADDED;
      } else if (nnFileStatus == DirstateNonnormalFileStatus::Normal) {
        auto mergeState = dirstateTuple->get_mergeState();
        // TODO(mbolin): Also need to set to ADDED if path is in the copymap.
        if (mergeState == DirstateMergeState::OtherParent) {
          statusCode = StatusCode::ADDED;
        }
      }
    }
    data->status.emplace(path.stringPiece().str(), statusCode);
  }

  void removedFile(
      RelativePathPiece path,
      const TreeEntry& /* sourceControlEntry */) override {
    processChangedFile(
        path,
        DirstateNonnormalFileStatus::MarkedForRemoval,
        StatusCode::REMOVED,
        StatusCode::MISSING);
  }

  void modifiedFile(
      RelativePathPiece path,
      const TreeEntry& /* sourceControlEntry */) override {
    processChangedFile(
        path,
        DirstateNonnormalFileStatus::MarkedForRemoval,
        StatusCode::REMOVED,
        StatusCode::MODIFIED);
  }

  void diffError(RelativePathPiece path, const folly::exception_wrapper& ew)
      override {
    // TODO: It would be nice to have a mechanism to return error info as part
    // of the thrift result.
    XLOG(WARNING) << "error computing status data for " << path << ": "
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
      for (const auto& entry : data->hgDirstateTuples) {
        auto nnFileStatus = entry.second.get_status();
        if (nnFileStatus != DirstateNonnormalFileStatus::MarkedForAddition &&
            nnFileStatus != DirstateNonnormalFileStatus::MarkedForRemoval) {
          // TODO(mbolin): Handle this case.
          continue;
        }
        auto hgStatusCode =
            (nnFileStatus == DirstateNonnormalFileStatus::MarkedForAddition)
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
   * - If the file is present in hgDirstateTuples as userDirectiveStatus,
   *   then remove it from hgDirstateTuples and report the status as
   *   userDirectiveStatus.
   * - Otherwise, report the status as defaultStatus
   */
  void processChangedFile(
      RelativePathPiece path,
      DirstateNonnormalFileStatus userDirectiveType,
      StatusCode userDirectiveStatus,
      StatusCode defaultStatus) {
    auto data = data_.wlock();
    auto iter = data->hgDirstateTuples.find(path.stringPiece());
    StatusCode newStatus = defaultStatus;
    if (iter != data->hgDirstateTuples.end() &&
        iter->second.get_status() == userDirectiveType) {
      newStatus = userDirectiveStatus;
      data->hgDirstateTuples.erase(iter);
    }
    data->status.emplace(path.stringPiece().str(), newStatus);
    XLOG(INFO) << "ThriftStatusCallback::processChangedFile(" << path << ") -> "
               << _StatusCode_VALUES_TO_NAMES.at(newStatus);
  }

  struct Data {
    explicit Data(const folly::StringKeyedUnorderedMap<DirstateTuple>& ud)
        : hgDirstateTuples(ud) {}

    std::map<std::string, StatusCode> status;
    StringKeyedUnorderedMap<DirstateTuple> hgDirstateTuples;
  };
  folly::Synchronized<Data> data_;
};
} // unnamed namespace

Dirstate::Dirstate(EdenMount* mount)
    : mount_(mount),
      persistence_(mount->getConfig()->getDirstateStoragePath()) {
  auto loadedData = persistence_.load();
}

Dirstate::~Dirstate() {}

ThriftHgStatus Dirstate::getStatus(bool listIgnored) const {
  ThriftStatusCallback callback(data_.rlock()->hgDirstateTuples);
  mount_->diff(&callback, listIgnored).get();
  return callback.extractStatus();
}

namespace {

static bool isMagicPath(RelativePathPiece path) {
  // If any component of the path name is .eden, then this path is a magic
  // path that we won't allow to be checked in or show up in the dirstate.
  for (auto c : path.components()) {
    if (c.stringPiece() == kDotEdenName) {
      return true;
    }
  }
  return false;
}
}

Future<Unit> Dirstate::onSnapshotChanged(const Tree* rootTree) {
  XLOG(INFO) << "Dirstate::onSnapshotChanged(" << rootTree->getHash() << ")";
  {
    auto data = data_.wlock();
    bool madeChanges = false;

    if (!data->hgDestToSourceCopyMap.empty()) {
      // For now, we blindly assume that when the snapshot changes, the copymap
      // data is no longer valid.
      data->hgDestToSourceCopyMap.clear();
      madeChanges = true;
    }

    // For now, we also blindly assume that when the snapshot changes, we can
    // remove all dirstate tuples except for those that have a merge state of
    // OtherParent.
    auto iter = data->hgDirstateTuples.begin();
    while (iter != data->hgDirstateTuples.end()) {
      // If we need to erase this element, it will erase iterators pointing to
      // it, but other iterators will be unaffected.
      auto current = iter;
      ++iter;

      if (current->second.get_mergeState() != DirstateMergeState::OtherParent) {
        data->hgDirstateTuples.erase(current);
        madeChanges = true;
      }
    }

    if (madeChanges) {
      persistence_.save(*data);
    }
  }

  return makeFuture();
}

DirstateTuple Dirstate::hgGetDirstateTuple(const RelativePathPiece filename) {
  {
    auto data = data_.rlock();
    auto& hgDirstateTuples = data->hgDirstateTuples;
    auto* ptr = folly::get_ptr(hgDirstateTuples, filename.stringPiece());
    if (ptr != nullptr) {
      return *ptr;
    }
  }

  if (filename == RelativePathPiece{".hgsub"} ||
      filename == RelativePathPiece{".hgsubstate"}) {
    // Currently, these are the only files that Hg appears to ask about that are
    // not expected to be in the dirstate when the request is made. This is
    // admittedly pretty sloppy, but since we don't seem to be planning to
    // support subrepos in Eden, this seems to have the desired effect as it is
    // ultimately reflected as a KeyError in the Hg extension (though it could
    // be swallowing a real logical error in that case, as well).
    throw std::domain_error(folly::to<std::string>(
        "No hgDirstateTuple for ",
        filename.stringPiece(),
        " because Eden acts as if this file does not exist."));
  }

  // If the filename is in the manifest, return it.
  auto mode = isInManifestAsFile(filename);
  if (mode.hasValue()) {
    DirstateTuple tuple;
    tuple.set_status(DirstateNonnormalFileStatus::Normal);
    // Lower bits? Should be 644 not 100644.
    tuple.set_mode(mode.value());
    tuple.set_mergeState(DirstateMergeState::NotApplicable);
    return tuple;
  } else {
    throw std::domain_error(folly::to<std::string>(
        "No hgDirstateTuple for ",
        filename.stringPiece(),
        " because there is no entry for it in the root Tree as a file."));
  }
}

folly::Optional<mode_t> Dirstate::isInManifestAsFile(
    const RelativePathPiece filename) const {
  auto tree = mount_->getRootTree();
  auto parentDirectory = filename.dirname();
  auto objectStore = mount_->getObjectStore();
  for (auto piece : parentDirectory.components()) {
    auto entry = tree->getEntryPtr(piece);
    if (entry != nullptr && entry->getFileType() == FileType::DIRECTORY) {
      tree = objectStore->getTree(entry->getHash()).get();
    } else {
      return folly::none;
    }
  }

  if (tree != nullptr) {
    auto entry = tree->getEntryPtr(filename.basename());
    if (entry != nullptr && entry->getFileType() != FileType::DIRECTORY) {
      return entry->getMode();
    }
  }

  return folly::none;
}

void Dirstate::hgSetDirstateTuple(
    const RelativePathPiece filename,
    const DirstateTuple* tuple) {
  auto data = data_.wlock();
  data->hgDirstateTuples[filename.stringPiece()] = *tuple;
  persistence_.save(*data);
}

bool Dirstate::hgDeleteDirstateTuple(const RelativePathPiece filename) {
  return data_.wlock()->hgDirstateTuples.erase(filename.stringPiece());
}

std::unordered_map<RelativePath, DirstateTuple> Dirstate::hgGetNonnormalFiles()
    const {
  std::unordered_map<RelativePath, DirstateTuple> out;
  auto& hgDirstateTuples = data_.rlock()->hgDirstateTuples;
  for (const auto& pair : hgDirstateTuples) {
    out.emplace(RelativePath{pair.first}, pair.second);
  }
  return out;
}

void Dirstate::hgCopyMapPut(
    const RelativePathPiece dest,
    const RelativePathPiece source) {
  auto data = data_.wlock();
  if (source.empty()) {
    data->hgDestToSourceCopyMap.erase(dest.stringPiece());
  } else {
    data->hgDestToSourceCopyMap.emplace(dest.stringPiece(), source.copy());
  }
  persistence_.save(*data);
}

RelativePath Dirstate::hgCopyMapGet(const RelativePathPiece dest) const {
  auto& hgDestToSourceCopyMap = data_.rlock()->hgDestToSourceCopyMap;
  return folly::get_or_throw(hgDestToSourceCopyMap, dest.stringPiece());
}

folly::StringKeyedUnorderedMap<RelativePath> Dirstate::hgCopyMapGetAll() const {
  return data_.rlock()->hgDestToSourceCopyMap;
}

std::ostream& operator<<(
    std::ostream& os,
    const DirstateAddRemoveError& error) {
  return os << error.errorMessage;
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

std::ostream& operator<<(std::ostream& os, const ThriftHgStatus& status) {
  os << "{";
  for (const auto& pair : status.get_entries()) {
    os << hgStatusCodeChar(pair.second) << " " << pair.first << "; ";
  }
  os << "}";
  return os;
}
}
}
