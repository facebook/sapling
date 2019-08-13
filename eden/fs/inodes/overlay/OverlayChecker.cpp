/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/overlay/OverlayChecker.h"

#include <boost/filesystem.hpp>

#include <folly/Conv.h>
#include <folly/ExceptionWrapper.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/fs/inodes/overlay/FsOverlay.h"

using apache::thrift::CompactSerializer;
using folly::ByteRange;
using folly::MutableStringPiece;
using folly::StringPiece;
using std::make_unique;
using std::optional;
using std::string;
using std::unique_ptr;

namespace facebook {
namespace eden {

class OverlayChecker::ShardDirectoryEnumerationError
    : public OverlayChecker::Error {
 public:
  ShardDirectoryEnumerationError(
      AbsolutePathPiece path,
      boost::system::error_code error)
      : path_(path), error_(error) {}

  string getMessage(OverlayChecker*) const override {
    return folly::to<string>(
        "fsck error attempting to enumerate ", path_, ": ", error_.message());
  }

 private:
  AbsolutePath path_;
  boost::system::error_code error_;
};

class OverlayChecker::UnexpectedOverlayFile : public OverlayChecker::Error {
 public:
  explicit UnexpectedOverlayFile(AbsolutePathPiece path) : path_(path) {}

  string getMessage(OverlayChecker*) const override {
    return folly::to<string>("unexpected file present in overlay: ", path_);
  }

 private:
  AbsolutePath path_;
};

class OverlayChecker::UnexpectedInodeShard : public OverlayChecker::Error {
 public:
  UnexpectedInodeShard(InodeNumber number, ShardID shardID)
      : number_(number), shardID_(shardID) {}

  string getMessage(OverlayChecker*) const override {
    return folly::to<string>(
        "found a data file for inode ",
        number_,
        " in the wrong shard directory (",
        shardID_,
        ")");
  }

 private:
  InodeNumber number_;
  ShardID shardID_;
};

class OverlayChecker::InodeDataError : public OverlayChecker::Error {
 public:
  template <typename... Args>
  explicit InodeDataError(InodeNumber number, Args&&... args)
      : number_(number),
        message_(folly::to<string>(std::forward<Args>(args)...)) {}

  string getMessage(OverlayChecker*) const override {
    return folly::to<string>(
        "error reading data for inode ", number_, ": ", message_);
  }

 private:
  InodeNumber number_;
  std::string message_;
};

class OverlayChecker::MissingMaterializedInode : public OverlayChecker::Error {
 public:
  MissingMaterializedInode(
      InodeNumber number,
      StringPiece childName,
      overlay::OverlayEntry childInfo)
      : number_(number), childName_(childName), childInfo_(childInfo) {}

  string getMessage(OverlayChecker* checker) const override {
    auto fileTypeStr = S_ISDIR(childInfo_.mode)
        ? "directory"
        : (S_ISLNK(childInfo_.mode) ? "symlink" : "file");
    auto path = checker->computePath(number_, childName_);
    return folly::to<string>(
        "missing overlay file for materialized ",
        fileTypeStr,
        " inode ",
        childInfo_.inodeNumber,
        " (",
        path.toString(),
        ")");
  }

 private:
  InodeNumber number_;
  PathComponent childName_;
  overlay::OverlayEntry childInfo_;
};

class OverlayChecker::OrphanInode : public OverlayChecker::Error {
 public:
  explicit OrphanInode(InodeNumber number) : number_(number) {}

  string getMessage(OverlayChecker*) const override {
    return folly::to<string>("found orphan inode ", number_);
  }

 private:
  InodeNumber number_;
};

class OverlayChecker::HardLinkedInode : public OverlayChecker::Error {
 public:
  explicit HardLinkedInode(InodeNumber number, const InodeInfo* info)
      : number_(number) {
    parents_.insert(parents_.end(), info->parents.begin(), info->parents.end());
    // Sort the parent inode numbers, just to ensure deterministic ordering
    // of paths in the error message so we can check it more easily in the unit
    // tests.
    std::sort(parents_.begin(), parents_.end());
  }

  string getMessage(OverlayChecker* checker) const override {
    auto msg = folly::to<string>("found hard linked inode ", number_, ":");
    for (auto parent : parents_) {
      msg += "\n- " + checker->computePath(parent, number_).toString();
    }
    return msg;
  }

 private:
  InodeNumber number_;
  std::vector<InodeNumber> parents_;
};

class OverlayChecker::BadNextInodeNumber : public OverlayChecker::Error {
 public:
  BadNextInodeNumber(InodeNumber loadedNumber, InodeNumber expectedNumber)
      : loadedNumber_(loadedNumber), expectedNumber_(expectedNumber) {}

  string getMessage(OverlayChecker*) const override {
    return folly::to<string>(
        "bad stored next inode number: read ",
        loadedNumber_,
        " but should be at least ",
        expectedNumber_);
  }

 private:
  InodeNumber loadedNumber_;
  InodeNumber expectedNumber_;
};

OverlayChecker::OverlayChecker(
    FsOverlay* fs,
    optional<InodeNumber> nextInodeNumber)
    : fs_(fs), loadedNextInodeNumber_(nextInodeNumber) {}

OverlayChecker::~OverlayChecker() {}

void OverlayChecker::scanForErrors() {
  XLOG(INFO) << "Starting fsck scan on overlay " << fs_->getLocalDir();
  readInodes();
  linkInodeChildren();
  scanForParentErrors();
  checkNextInodeNumber();

  if (errors_.empty()) {
    XLOG(INFO) << "fsck:" << fs_->getLocalDir()
               << ": completed checking for errors, no problems found";
  } else {
    XLOG(ERR) << "fsck:" << fs_->getLocalDir()
              << ": completed checking for errors, found " << errors_.size()
              << " problems";
  }
}

void OverlayChecker::repairErrors() {
  // TODO: actually repair the problems
  logErrors();
}

void OverlayChecker::logErrors() {
  for (const auto& error : errors_) {
    XLOG(ERR) << "fsck:" << fs_->getLocalDir()
              << ": error: " << error->getMessage(this);
  }
}

std::string OverlayChecker::PathInfo::toString() const {
  if (parent == kRootNodeId) {
    return path.value();
  }
  return folly::to<string>("[unlinked(", parent.get(), ")]/", path.value());
}

template <typename Fn>
OverlayChecker::PathInfo OverlayChecker::cachedPathComputation(
    InodeNumber number,
    Fn&& fn) {
  if (number == kRootNodeId) {
    return PathInfo(kRootNodeId);
  }
  auto cacheIter = pathCache_.find(number);
  if (cacheIter != pathCache_.end()) {
    return cacheIter->second;
  }

  auto result = fn();
  pathCache_.emplace(number, result);
  return result;
}

OverlayChecker::PathInfo OverlayChecker::computePath(InodeNumber number) {
  return cachedPathComputation(number, [&]() {
    auto iter = inodes_.find(number);
    if (iter == inodes_.end()) {
      // We don't normally expect computePath() to be called on unknown inode
      // numbers.
      XLOG(WARN) << "computePath() called on unknown inode " << number;
      return PathInfo(number);
    } else if (iter->second->parents.empty()) {
      // This inode is unlinked/orphaned
      return PathInfo(number);
    } else {
      auto parentNumber = InodeNumber(iter->second->parents[0]);
      return computePath(parentNumber, number);
    }
  });
}

OverlayChecker::PathInfo OverlayChecker::computePath(const InodeInfo& info) {
  return cachedPathComputation(info.number, [&]() {
    if (info.parents.empty()) {
      return PathInfo(info.number);
    } else {
      return computePath(InodeNumber(info.parents[0]), info.number);
    }
  });
}

OverlayChecker::PathInfo OverlayChecker::computePath(
    InodeNumber parent,
    PathComponentPiece child) {
  return PathInfo(computePath(parent), child);
}

OverlayChecker::PathInfo OverlayChecker::computePath(
    InodeNumber parent,
    InodeNumber child) {
  auto iter = inodes_.find(parent);
  if (iter == inodes_.end()) {
    // This shouldn't ever happen unless we have a bug in the fsck code somehow.
    // The parent relationships are only set up if we found both inodes.
    XLOG(DFATAL) << "bug in fsck code: previously found parent " << parent
                 << " of " << child << " but can no longer find parent";
    return PathInfo(child);
  }

  const auto& parentInfo = *(iter->second);
  auto childName = findChildName(parentInfo, child);
  return PathInfo(computePath(parentInfo), childName);
}

PathComponent OverlayChecker::findChildName(
    const InodeInfo& parentInfo,
    InodeNumber child) {
  // We just scan through all of the parents children to find the matching
  // entry.  While we could build a full map of children information during
  // linkInodeChildren(), we only need this information when we actually find an
  // error, which is hopefully rare.  Therefore we avoid doing as much work as
  // possible during linkInodeChildren(), at the cost of doing extra work here
  // if we do actually need to compute paths.
  for (const auto& entry : parentInfo.children.entries) {
    if (static_cast<uint64_t>(entry.second.inodeNumber) == child.get()) {
      return PathComponent(entry.first);
    }
  }

  // This shouldn't ever happen unless we have a bug in the fsck code somehow.
  // We should only get here if linkInodeChildren() found a parent-child
  // relationship between these two inodes, and that relationship shouldn't ever
  // change during the fsck run.
  XLOG(DFATAL) << "bug in fsck code: cannot find child " << child
               << " in directory listing of parent " << parentInfo.number;
  return PathComponent(folly::to<string>("[missing_child(", child, ")]"));
}

void OverlayChecker::readInodes() {
  // Walk through all of the sharded subdirectories
  uint32_t progress10pct = 0;
  std::array<char, 2> subdirBuffer;
  MutableStringPiece subdir{subdirBuffer.data(), subdirBuffer.size()};
  for (uint32_t shardID = 0; shardID < FsOverlay::kNumShards; ++shardID) {
    // Log a DBG2 message every 10% done
    uint32_t progress = (10 * shardID) / FsOverlay::kNumShards;
    if (progress > progress10pct) {
      XLOG(DBG2) << "fsck:" << fs_->getLocalDir() << ": scan " << progress
                 << "0% complete: " << inodes_.size() << " inodes scanned";
      progress10pct = progress;
    }

    FsOverlay::formatSubdirShardPath(shardID, subdir);
    auto subdirPath = fs_->getLocalDir() + PathComponentPiece{subdir};

    readInodeSubdir(subdirPath, shardID);
  }
  XLOG(DBG1) << "fsck:" << fs_->getLocalDir() << ": scanned " << inodes_.size()
             << " inodes";
}

void OverlayChecker::readInodeSubdir(
    const AbsolutePath& path,
    ShardID shardID) {
  XLOG(DBG5) << "fsck:" << fs_->getLocalDir() << ": scanning " << path;

  boost::system::error_code error;
  auto boostPath = boost::filesystem::path{path.value().c_str()};
  auto iterator = boost::filesystem::directory_iterator(boostPath, error);
  if (error.value() != 0) {
    addError<ShardDirectoryEnumerationError>(path, error);
    return;
  }

  auto endIterator = boost::filesystem::directory_iterator();
  while (iterator != endIterator) {
    const auto& dirEntry = *iterator;
    AbsolutePath inodePath(dirEntry.path().string());
    auto entryInodeNumber =
        folly::tryTo<uint64_t>(inodePath.basename().value());
    if (entryInodeNumber.hasValue()) {
      loadInode(InodeNumber(*entryInodeNumber), shardID);
    } else {
      addError<UnexpectedOverlayFile>(inodePath);
    }

    iterator.increment(error);
    if (error.value() != 0) {
      addError<ShardDirectoryEnumerationError>(path, error);
      break;
    }
  }
}

void OverlayChecker::loadInode(InodeNumber number, ShardID shardID) {
  XLOG(DBG9) << "fsck: loading inode " << number;
  updateMaxInodeNumber(number);

  // Verify that we found this inode in the correct shard subdirectory.
  // Ignore the data if it is in the wrong directory.
  ShardID expectedShard = static_cast<ShardID>(number.get() & 0xff);
  if (expectedShard != shardID) {
    addError<UnexpectedInodeShard>(number, shardID);
    return;
  }

  auto info = loadInodeInfo(number);
  if (!info) {
    // loadInodeInfo() will have already added an error.
    // Add an error entry to the inode map to record that there was an inode
    // file here but we couldn't load it.  This helps us avoid recording
    // duplicate errors when its parent directory is checking to make sure that
    // all of the materialized children were present.
    info = make_unique<InodeInfo>(number, InodeType::Error);
  }
  inodes_.emplace(number, std::move(info));
}

unique_ptr<OverlayChecker::InodeInfo> OverlayChecker::loadInodeInfo(
    InodeNumber number) {
  // Open the inode file
  folly::File file;
  try {
    file = fs_->openFileNoVerify(number);
  } catch (const std::exception& ex) {
    addError<InodeDataError>(
        number, "error opening file: ", folly::exceptionStr(ex));
    return nullptr;
  }

  // Read the file header
  std::array<char, FsOverlay::kHeaderLength> headerContents;
  auto readResult =
      folly::readFull(file.fd(), headerContents.data(), headerContents.size());
  if (readResult < 0) {
    int errnum = errno;
    addError<InodeDataError>(
        number, "error reading from file: ", folly::errnoStr(errnum));
    return nullptr;
  } else if (readResult != FsOverlay::kHeaderLength) {
    addError<InodeDataError>(
        number,
        "file was too short to contain overlay header: read ",
        readResult,
        " bytes, expected ",
        FsOverlay::kHeaderLength,
        " bytes");
    return nullptr;
  }

  // The first 4 bytes of the header are the file type identifier.
  static_assert(
      FsOverlay::kHeaderIdentifierDir.size() ==
          FsOverlay::kHeaderIdentifierFile.size(),
      "both header IDs must have the same length");
  StringPiece typeID(
      headerContents.data(),
      headerContents.data() + FsOverlay::kHeaderIdentifierDir.size());

  // The next 4 bytes are the version ID.
  uint32_t versionBE;
  memcpy(
      &versionBE,
      headerContents.data() + FsOverlay::kHeaderIdentifierDir.size(),
      sizeof(uint32_t));
  auto version = folly::Endian::big(versionBE);
  if (version != FsOverlay::kHeaderVersion) {
    addError<InodeDataError>(
        number, "unknown overlay file format version ", version);
    return nullptr;
  }

  InodeType type;
  if (typeID == FsOverlay::kHeaderIdentifierDir) {
    type = InodeType::Dir;
  } else if (typeID == FsOverlay::kHeaderIdentifierFile) {
    type = InodeType::File;
  } else {
    addError<InodeDataError>(
        number,
        "unknown overlay file type ID: ",
        folly::hexlify(ByteRange{typeID}));
    return nullptr;
  }

  auto info = make_unique<InodeInfo>(number, type);
  if (type == InodeType::Dir) {
    try {
      info->children = loadDirectoryChildren(file);
    } catch (const std::exception& ex) {
      addError<InodeDataError>(
          number,
          "error parsing directory contents: ",
          folly::exceptionStr(ex));
      return nullptr;
    }
  }

  return info;
}

overlay::OverlayDir OverlayChecker::loadDirectoryChildren(folly::File& file) {
  std::string serializedData;
  if (!folly::readFile(file.fd(), serializedData)) {
    folly::throwSystemError("read failed");
  }

  return CompactSerializer::deserialize<overlay::OverlayDir>(serializedData);
}

void OverlayChecker::linkInodeChildren() {
  for (const auto& [parentInodeNumber, parent] : inodes_) {
    for (const auto& [childName, child] : parent->children.entries) {
      auto childRawInode = child.inodeNumber;
      if (childRawInode == 0) {
        // Older versions of edenfs would leave the inode number set to 0
        // if the child inode has never been loaded.  The child can't be
        // present in the overlay if it doesn't have an inode number
        // allocated for it yet.
        //
        // Newer versions of edenfs always allocate an inode number for all
        // children, even if they haven't been loaded yet.
        continue;
      }

      auto childInodeNumber = InodeNumber(childRawInode);
      updateMaxInodeNumber(childInodeNumber);
      auto childIter = inodes_.find(childInodeNumber);
      if (childIter == inodes_.end()) {
        const auto& hash = child.hash_ref();
        if (!hash.has_value() || hash->empty()) {
          // This child is materialized (since it doesn't have a hash
          // linking it to a source control object).  It's a problem if the
          // materialized data isn't actually present in the overlay.
          addError<MissingMaterializedInode>(
              parentInodeNumber, childName, child);
        }
      } else {
        childIter->second->addParent(parentInodeNumber);

        // TODO: It would be nice to also check for mismatch between
        // childIter->second.type and child.mode
      }
    }
  }
}

void OverlayChecker::scanForParentErrors() {
  for (const auto& [inodeNumber, inodeInfo] : inodes_) {
    if (inodeInfo->parents.empty()) {
      if (inodeNumber != kRootNodeId) {
        addError<OrphanInode>(inodeNumber);
      }
    } else if (inodeInfo->parents.size() != 1) {
      addError<HardLinkedInode>(inodeNumber, inodeInfo.get());
    }
  }
}

void OverlayChecker::checkNextInodeNumber() {
  auto expectedNextInodeNumber = getNextInodeNumber();
  // If loadedNextInodeNumber_ is unset we don't report this as an error.
  // Usually this is what triggered the fsck operation, so the caller will
  // likely already log an error message about that fact.  If the only problem
  // we find is this missing next inode number we don't want to create a new
  // fsck log directory.  We'll always write out the correct next inode number
  // file when we close the overlay next.
  //
  // We only report an error here if there was a next inode number file but it
  // contains incorrect data.  (This will probably only happen if someone forced
  // an fsck run even if it looks like the mount was shut down cleanly.)
  if (loadedNextInodeNumber_.has_value() &&
      *loadedNextInodeNumber_ < expectedNextInodeNumber) {
    addError<BadNextInodeNumber>(
        *loadedNextInodeNumber_, expectedNextInodeNumber);
  }
}

void OverlayChecker::addError(unique_ptr<Error> error) {
  // Note that we log with a very low verbosity level here, so that this message
  // is disabled by default.  The repairErrors() or logErrors() functions is
  // where errors are normally reported by default.
  //
  // When addError() is called we often haven't fully computed the inode
  // relationships yet, so computePath() won't return correct results for any
  // error messages that want to include path names.
  XLOG(DBG7) << "fsck: addError() called for " << fs_->getLocalDir() << ": "
             << error->getMessage(this);
  errors_.push_back(std::move(error));
}

} // namespace eden
} // namespace facebook
