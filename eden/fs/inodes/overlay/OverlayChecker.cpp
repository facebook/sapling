/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/overlay/OverlayChecker.h"

#include <boost/filesystem.hpp>
#include <fcntl.h>
#include <time.h>
#include <unistd.h>

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
using std::chrono::microseconds;
using std::chrono::seconds;

namespace facebook {
namespace eden {

class OverlayChecker::RepairState {
 public:
  explicit RepairState(OverlayChecker* checker)
      : checker_(checker),
        dir_(createRepairDir(checker_->fs_->getLocalDir())),
        logFile_(
            (dir_ + PathComponentPiece("fsck.log")).c_str(),
            O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC,
            0600) {}

  void log(StringPiece msg) {
    logLine(msg);
  }
  template <typename Arg1, typename... Args>
  void log(Arg1&& arg1, Args&&... args) {
    auto msg = folly::to<string>(
        std::forward<Arg1>(arg1), std::forward<Args>(args)...);
    logLine(msg);
  }

  template <typename Arg1, typename... Args>
  void warn(Arg1&& arg1, Args&&... args) {
    auto msg = folly::to<string>(
        std::forward<Arg1>(arg1), std::forward<Args>(args)...);
    XLOG(WARN) << "fsck:" << checker_->fs_->getLocalDir() << ":" << msg;
    logLine(msg);
  }

  AbsolutePath getRepairDir() const {
    return dir_;
  }

  OverlayChecker* checker() {
    return checker_;
  }
  FsOverlay* fs() {
    return checker_->fs_;
  }

  AbsolutePath getLostAndFoundPath() {
    auto lostNFound = dir_ + PathComponentPiece("lost+found");
    ensureDirectoryExists(lostNFound);
    return lostNFound;
  }

  /**
   * Get the path inside the repair directory where we should save
   * data for an orphan inode.
   */
  AbsolutePath getLostAndFoundPath(
      InodeNumber number,
      StringPiece suffix = {}) {
    return getLostAndFoundPath() +
        PathComponent(folly::to<string>(number, suffix));
  }
  AbsolutePath getLostAndFoundPath(const OverlayChecker::PathInfo& pathInfo) {
    // Note that we intentionally include pathInfo.parent in the path here,
    // even when it is kRootNodeId.  This helps avoid possible path collisions
    // in the lost+found directory if the root inode contained some children
    // whose names could also be the same as some other inode number.
    return getLostAndFoundPath() +
        PathComponent(folly::to<string>(pathInfo.parent)) + pathInfo.path;
  }

  /**
   * Create an overlay entry for the specified inode number.
   *
   * This helper function is used by InodeDataError and
   * MissingMaterializedInode.
   */
  void createInodeReplacement(InodeNumber number, mode_t mode) {
    // Create a new empty directory or file in this location.
    //
    // TODO: It would be somewhat nicer to look in the ObjectStore and see what
    // data would exist at this path in the current commit (if this path
    // exists).  If we can find contents hash that way, it would be nicer to
    // just dematerialize this inode's entry in its parent directory.
    // That said, in practice most of the times when we have seen files or
    // directories get corrupted they are generated files that are updated
    // frequently by tools, and aren't files we could recover from source
    // control state.  If the files can be recovered from source control, users
    // can always recover it themselves afterwards with `hg revert`
    if (S_ISDIR(mode)) {
      overlay::OverlayDir contents;
      fs()->saveOverlayDir(number, contents);
    } else if (S_ISLNK(mode)) {
      // symbolic links generally can't be empty in normal circumstances,
      // so put some dummy data in the link.
      fs()->createOverlayFile(number, StringPiece("[lost]"));
    } else {
      fs()->createOverlayFile(number, ByteRange());
    }
  }

 private:
  static struct tm getLocalTime(time_t now) {
    struct tm result;
    if (localtime_r(&now, &result) == nullptr) {
      folly::throwSystemError("error getting local time during fsck repair");
    }
    return result;
  }

  static AbsolutePath createRepairDir(AbsolutePathPiece overlayDir) {
    // Put all repair directories in a sibling directory of the overlay.
    auto baseDir = overlayDir.dirname() + PathComponentPiece("fsck");
    ensureDirectoryExists(baseDir);

    // Name the repair directory based on the current timestamp
    auto now = getLocalTime(time(nullptr));
    auto timestampStr = folly::sformat(
        "{:04d}{:02d}{:02d}_{:02d}{:02d}{:02d}",
        now.tm_year + 1900,
        now.tm_mon,
        now.tm_mday,
        now.tm_hour,
        now.tm_min,
        now.tm_sec);

    // Support adding an extra count number to the directory name in the
    // unlikely event that a directory already exists for the current second.
    for (size_t iter = 0; iter < 100; ++iter) {
      AbsolutePath path;
      if (iter == 0) {
        path = baseDir + PathComponentPiece(timestampStr);
      } else {
        path = baseDir +
            PathComponentPiece(folly::to<string>(timestampStr, ".", iter));
      }

      int rc = mkdir(path.c_str(), 0700);
      if (rc == 0) {
        return path;
      }
      if (errno != EEXIST) {
        folly::throwSystemError("error creating fsck repair directory");
      }
    }

    // We should only reach here if we tried 100 different directory names for
    // the current second and they all already existed.  This is very unlikely.
    // We use a limit of 100 just to ensure we can't ever have an infinite loop,
    // even in the event of some other bug.
    throw std::runtime_error(
        "failed to create an fsck repair directory: retry limit exceeded");
  }

  void logLine(StringPiece msg) {
    auto now = std::chrono::system_clock::now().time_since_epoch();
    auto nowSec = std::chrono::duration_cast<seconds>(now);
    auto us = std::chrono::duration_cast<microseconds>(now - nowSec);
    auto timeFields = getLocalTime(nowSec.count());
    auto header = folly::sformat(
        "{:04d}-{:02d}-{:02d} {:02d}:{:02d}:{:02d}.{:06d}: ",
        timeFields.tm_year + 1900,
        timeFields.tm_mon,
        timeFields.tm_mday,
        timeFields.tm_hour,
        timeFields.tm_min,
        timeFields.tm_sec,
        us.count());
    auto fullMsg = folly::to<string>(header, msg, "\n");

    // We don't buffer output to the log file, and write each message
    // immediately.
    auto result =
        folly::writeFull(logFile_.fd(), fullMsg.data(), fullMsg.size());
    if (result == -1) {
      int errnum = errno;
      XLOG(ERR) << "error writing to fsck repair log file: "
                << folly::errnoStr(errnum);
    }
  }

  OverlayChecker* const checker_;
  AbsolutePath dir_;
  folly::File logFile_;
};

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

  bool repair(RepairState& /* repair */) const override {
    // The only error we can really handle here is if the shard directory didn't
    // exist.  Try creating the directory, in hopes that this was the problem.
    // (We could check the error code in error_ too to confirm that this is the
    // issue.)
    int rc = mkdir(path_.c_str(), 0700);
    if (rc == 0) {
      // If we created the shard directory this likely fixed the problem.
      return true;
    } else {
      return false;
    }
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

  bool repair(RepairState& /* repair */) const override {
    // TODO: Move the file into the repair directory, with some unique name
    return false;
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

  bool repair(RepairState& /* repair */) const override {
    // TODO: Move the file into the repair directory, with some unique name
    return false;
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

  bool repair(RepairState& repair) const override {
    // Move the bad file into the lost+found directory
    auto pathInfo = repair.checker()->computePath(number_);
    auto outputPath = repair.getLostAndFoundPath(pathInfo);
    ensureDirectoryExists(outputPath.dirname());
    auto srcPath = repair.fs()->getAbsoluteFilePath(number_);
    auto ret = ::rename(srcPath.c_str(), outputPath.c_str());
    folly::checkUnixError(
        ret, "failed to rename inode data ", srcPath, " to ", outputPath);

    // Create replacement data for this inode in the overlay.
    const auto& inodes = repair.checker()->inodes_;
    auto iter = inodes.find(number_);
    mode_t mode = (iter == inodes.end()) ? 0 : iter->second.modeFromParent;
    if (mode == 0) {
      mode = (S_IFREG | 0644);
    }
    repair.createInodeReplacement(number_, mode);
    return true;
  }

 private:
  InodeNumber number_;
  std::string message_;
};

class OverlayChecker::MissingMaterializedInode : public OverlayChecker::Error {
 public:
  MissingMaterializedInode(
      InodeNumber parentDirInode,
      StringPiece childName,
      overlay::OverlayEntry childInfo)
      : parent_(parentDirInode), childName_(childName), childInfo_(childInfo) {}

  string getMessage(OverlayChecker* checker) const override {
    auto fileTypeStr = S_ISDIR(childInfo_.mode)
        ? "directory"
        : (S_ISLNK(childInfo_.mode) ? "symlink" : "file");
    auto path = checker->computePath(parent_, childName_);
    return folly::to<string>(
        "missing overlay file for materialized ",
        fileTypeStr,
        " inode ",
        childInfo_.inodeNumber,
        " (",
        path.toString(),
        ")");
  }

  bool repair(RepairState& repair) const override {
    // Create replacement data for this inode in the overlay
    XDCHECK_NE(childInfo_.inodeNumber, 0);
    InodeNumber childInodeNumber(childInfo_.inodeNumber);
    repair.createInodeReplacement(childInodeNumber, childInfo_.mode);

    // Add an entry in the OverlayChecker's inodes_ set.
    // In case the parent directory was part of an orphaned subtree the
    // OrphanInode code will look for this child in the inodes_ map.
    auto type = S_ISDIR(childInfo_.mode) ? InodeType::Dir : InodeType::File;
    auto [iter, inserted] = repair.checker()->inodes_.try_emplace(
        childInodeNumber, childInodeNumber, type);
    XDCHECK(inserted);
    iter->second.addParent(parent_, childInfo_.mode);
    return true;
  }

 private:
  InodeNumber parent_;
  PathComponent childName_;
  overlay::OverlayEntry childInfo_;
};

class OverlayChecker::OrphanInode : public OverlayChecker::Error {
 public:
  explicit OrphanInode(const InodeInfo& info)
      : number_(info.number), type_(info.type) {}

  string getMessage(OverlayChecker*) const override {
    return folly::to<string>(
        "found orphan ",
        type_ == InodeType::Dir ? "directory" : "file",
        " inode ",
        number_);
  }

  bool repair(RepairState& repair) const override {
    switch (type_) {
      case InodeType::File: {
        auto outputPath = repair.getLostAndFoundPath(number_);
        archiveOrphanFile(repair, number_, outputPath, S_IFREG | 0644);
        return true;
      }
      case InodeType::Dir: {
        // Look up the previously loaded children data
        auto iter = repair.checker()->inodes_.find(number_);
        if (iter == repair.checker()->inodes_.end()) {
          XLOG(DFATAL) << "failed to look up previously-loaded children for "
                       << "orphan directory inode " << number_;
          return false;
        }
        auto outputPath = repair.getLostAndFoundPath(number_);
        archiveOrphanDir(repair, number_, outputPath, iter->second.children);
        return true;
      }
      case InodeType::Error: {
        processOrphanedError(repair, number_);
        return false;
      }
    }

    XLOG(DFATAL) << "unexpected inode type " << static_cast<int>(type_)
                 << " when processing orphan inode " << number_;
    return false;
  }

 private:
  void archiveOrphanDir(
      RepairState& repair,
      InodeNumber number,
      AbsolutePath archivePath,
      const overlay::OverlayDir& children) const {
    auto rc = mkdir(archivePath.value().c_str(), 0700);
    if (rc != 0 && errno != EEXIST) {
      // EEXIST is okay.  Another error repair step (like InodeDataError) may
      // have already created a lost+found directory for other files that are
      // part of our orphaned subtree.
      folly::checkUnixError(
          rc,
          "failed to create directory to archive orphan directory inode ",
          number);
    }

    auto* const checker = repair.checker();
    for (const auto& childEntry : children.entries) {
      auto childRawInode = childEntry.second.inodeNumber;
      if (childRawInode == 0) {
        // If this child does not have an inode number allocated it cannot
        // be materialized.
        continue;
      }

      // Look up the inode information that we previously loaded for this child.
      InodeNumber childInodeNumber(childRawInode);
      auto childInfo = checker->getInodeInfo(childInodeNumber);
      if (!childInfo) {
        // This child was not present in the overlay.
        // This means that it wasn't materialized, so there is nothing for us to
        // do here.
        continue;
      }

      auto childPath = archivePath + PathComponentPiece(childEntry.first);
      archiveDirectoryEntry(repair, childInfo, childEntry.second, childPath);
    }

    tryRemoveInode(repair, number);
  }

  void archiveDirectoryEntry(
      RepairState& repair,
      InodeInfo* info,
      overlay::OverlayEntry dirEntry,
      AbsolutePath archivePath) const {
    // If this directory entry has multiple parents skip it.
    // We don't want to remove it from the overlay if another parent is still
    // referencing it.  If all parents were themselves orphans this entry would
    // be detected as an orphan by a second fsck run.
    if (info->parents.size() > 1) {
      return;
    }

    switch (info->type) {
      case InodeType::File:
        archiveOrphanFile(repair, info->number, archivePath, dirEntry.mode);
        return;
      case InodeType::Dir:
        archiveOrphanDir(repair, info->number, archivePath, info->children);
        return;
      case InodeType::Error:
        processOrphanedError(repair, info->number);
        return;
    }

    XLOG(DFATAL) << "unexpected inode type " << static_cast<int>(info->type)
                 << " when processing orphan inode " << info->number;
    throw std::runtime_error("unexpected inode type");
  }

  void archiveOrphanFile(
      RepairState& repair,
      InodeNumber number,
      AbsolutePath archivePath,
      mode_t mode) const {
    auto input =
        repair.fs()->openFile(number, FsOverlay::kHeaderIdentifierFile);

    // If the file is a symlink, try to create the file in the archive
    // directory as a symlink.
    if (S_ISLNK(mode)) {
      // The maximum symlink size on Linux is really filesystem dependent.
      // _POSIX_SYMLINK_MAX is typically defined as 255, but various filesystems
      // have larger limits.  In practice ext4, btrfs, and tmpfs appear to limit
      // symlinks to 4095 bytes.  xfs appears to have a limit of 1023 bytes.
      //
      // Try reading up to 4096 bytes here.  If the data is longer than this, or
      // if we get ENAMETOOLONG when creating the symlink, we fall back and
      // extract the data as a regular file.
      constexpr size_t maxLength = 4096;
      std::vector<char> contents(maxLength);
      auto bytesRead = folly::preadFull(
          input.fd(),
          contents.data(),
          contents.size(),
          FsOverlay::kHeaderLength);
      if (bytesRead < 0) {
        folly::throwSystemError(
            "read error while copying symlink data from inode ",
            number,
            " to ",
            archivePath);
      }
      if (0 < bytesRead && static_cast<size_t>(bytesRead) < maxLength) {
        auto rc = ::symlink(contents.data(), archivePath.value().c_str());
        if (rc == 0) {
          // We successfully created a symlink of the contents, so we're done.
          return;
        }
      }
      // If we can't save the contents as a symlink, fall through and just
      // save them as a regular file.  We used pread() above, so the input file
      // position will still be at the start of the data, and we don't need to
      // reset it.
    }

    // Copy the data
    folly::File output(
        archivePath.value(), O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    size_t blockSize = 1024 * 1024;
    std::vector<uint8_t> buffer;
    buffer.resize(blockSize);
    while (true) {
      auto bytesRead =
          folly::readFull(input.fd(), buffer.data(), buffer.size());
      if (bytesRead < 0) {
        folly::throwSystemError(
            "read error while copying data from inode ",
            number,
            " to ",
            archivePath);
      } else if (bytesRead == 0) {
        break;
      }
      auto bytesWritten =
          folly::writeFull(output.fd(), buffer.data(), bytesRead);
      folly::checkUnixError(
          bytesWritten,
          "write error while copying data from inode ",
          number,
          " to ",
          archivePath);
    }

    // Now remove the orphan inode file
    tryRemoveInode(repair, number);
  }

  void processOrphanedError(RepairState& repair, InodeNumber number) const {
    // Inodes with a type of InodeType::Error should have already had their
    // broken data moved to the fsck repair directory by
    // InodeDataError::repair().  We are guaranteed to process all
    // InodeDataError objects before OrphanInode errors, since we find the
    // OrphanInode errors last.
    //
    // The InodeDataError::repair() code will have replaced the broken inode
    // contents with an empty file or directory.  We just need to remove that
    // here if it is part of an orphan subtree.
    tryRemoveInode(repair, number);
  }

  void tryRemoveInode(RepairState& repair, InodeNumber number) const {
    try {
      repair.fs()->removeOverlayFile(number);
    } catch (const std::system_error& ex) {
      // If we fail to remove the file log an error, but proceed with the rest
      // of the fsck repairs rather than letting the exception propagate up
      // to our caller.
      XLOG(ERR) << "error removing overlay file for orphaned inode " << number
                << " after archiving it: " << ex.what();
    }
  }

  InodeNumber number_;
  InodeType type_;
};

class OverlayChecker::HardLinkedInode : public OverlayChecker::Error {
 public:
  explicit HardLinkedInode(const InodeInfo& info) : number_(info.number) {
    parents_.insert(parents_.end(), info.parents.begin(), info.parents.end());
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

  bool repair(RepairState& /* repair */) const override {
    // TODO: split the inode into 2 separate copies
    return false;
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

  bool repair(RepairState& /* repair */) const override {
    // We don't need to do anything here.
    // We will always write out the correct next inode number when we close the
    // overlay next.
    return true;
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

optional<OverlayChecker::RepairResult> OverlayChecker::repairErrors() {
  if (errors_.empty()) {
    return std::nullopt;
  }

  // Create an output directory.  We will record a log of errors here,
  // and will move orphan inodes and other unrepairable data here.
  RepairState repair(this);
  RepairResult result;
  result.repairDir = repair.getRepairDir();
  result.totalErrors = errors_.size();
  repair.log("Beginning fsck repair for ", fs_->getLocalDir());
  repair.log(errors_.size(), " problems detected");

  size_t errnum = 0;
  for (const auto& error : errors_) {
    ++errnum;
    auto description = error->getMessage(this);
    XLOG(ERR) << "fsck:" << fs_->getLocalDir() << ": error: " << description;
    repair.log("error ", errnum, ": ", description);
    try {
      bool repaired = error->repair(repair);
      if (repaired) {
        ++result.fixedErrors;
        repair.log("  - successfully repaired error ", errnum);
      } else {
        repair.log("  ! unable to repair error ", errnum);
      }
    } catch (const std::exception& ex) {
      XLOG(ERR) << "fsck:" << fs_->getLocalDir()
                << ": unexpected error occurred while attempting repair: "
                << folly::exceptionStr(ex);
      repair.log(
          "  ! failed to repair error ",
          errnum,
          ": unexpected exception: ",
          folly::exceptionStr(ex));
    }
  }

  auto numUnfixed = errors_.size() - result.fixedErrors;
  string finalMsg;
  if (numUnfixed) {
    finalMsg = folly::to<string>(
        "repaired ",
        result.fixedErrors,
        " problems; ",
        numUnfixed,
        " were unfixable");
  } else {
    finalMsg = folly::to<string>(
        "successfully repaired all ", result.fixedErrors, " problems");
  }
  repair.log(finalMsg);
  XLOG(INFO) << "fsck:" << fs_->getLocalDir() << ": " << finalMsg;

  return result;
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

OverlayChecker::InodeInfo* FOLLY_NULLABLE
OverlayChecker::getInodeInfo(InodeNumber number) {
  auto iter = inodes_.find(number);
  if (iter == inodes_.end()) {
    return nullptr;
  }
  return &(iter->second);
}

OverlayChecker::PathInfo OverlayChecker::computePath(InodeNumber number) {
  return cachedPathComputation(number, [&]() {
    auto info = getInodeInfo(number);
    if (!info) {
      // We don't normally expect computePath() to be called on unknown inode
      // numbers.
      XLOG(WARN) << "computePath() called on unknown inode " << number;
      return PathInfo(number);
    } else if (info->parents.empty()) {
      // This inode is unlinked/orphaned
      return PathInfo(number);
    } else {
      auto parentNumber = InodeNumber(info->parents[0]);
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
  auto parentInfo = getInodeInfo(parent);
  if (!parentInfo) {
    // This shouldn't ever happen unless we have a bug in the fsck code somehow.
    // The parent relationships are only set up if we found both inodes.
    XLOG(DFATAL) << "bug in fsck code: previously found parent " << parent
                 << " of " << child << " but can no longer find parent";
    return PathInfo(child);
  }

  auto childName = findChildName(*parentInfo, child);
  return PathInfo(computePath(*parentInfo), childName);
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

  inodes_.emplace(number, loadInodeInfo(number));
}

OverlayChecker::InodeInfo OverlayChecker::loadInodeInfo(InodeNumber number) {
  auto inodeError = [this, number](auto&&... args) {
    addError<InodeDataError>(number, args...);
    return InodeInfo(number, InodeType::Error);
  };

  // Open the inode file
  folly::File file;
  try {
    file = fs_->openFileNoVerify(number);
  } catch (const std::exception& ex) {
    return inodeError("error opening file: ", folly::exceptionStr(ex));
  }

  // Read the file header
  std::array<char, FsOverlay::kHeaderLength> headerContents;
  auto readResult =
      folly::readFull(file.fd(), headerContents.data(), headerContents.size());
  if (readResult < 0) {
    int errnum = errno;
    return inodeError("error reading from file: ", folly::errnoStr(errnum));
  } else if (readResult != FsOverlay::kHeaderLength) {
    return inodeError(
        "file was too short to contain overlay header: read ",
        readResult,
        " bytes, expected ",
        FsOverlay::kHeaderLength,
        " bytes");
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
    return inodeError("unknown overlay file format version ", version);
  }

  InodeType type;
  if (typeID == FsOverlay::kHeaderIdentifierDir) {
    type = InodeType::Dir;
  } else if (typeID == FsOverlay::kHeaderIdentifierFile) {
    type = InodeType::File;
  } else {
    return inodeError(
        "unknown overlay file type ID: ", folly::hexlify(ByteRange{typeID}));
  }

  if (type == InodeType::Dir) {
    try {
      return InodeInfo(number, loadDirectoryChildren(file));
    } catch (const std::exception& ex) {
      return inodeError(
          "error parsing directory contents: ", folly::exceptionStr(ex));
    }
  }
  return InodeInfo(number, type);
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
    for (const auto& [childName, child] : parent.children.entries) {
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
      auto childInfo = getInodeInfo(childInodeNumber);
      if (!childInfo) {
        const auto& hash = child.hash_ref();
        if (!hash.has_value() || hash->empty()) {
          // This child is materialized (since it doesn't have a hash
          // linking it to a source control object).  It's a problem if the
          // materialized data isn't actually present in the overlay.
          addError<MissingMaterializedInode>(
              parentInodeNumber, childName, child);
        }
      } else {
        childInfo->addParent(parentInodeNumber, child.mode);

        // TODO: It would be nice to also check for mismatch between
        // childInfo->type and child.mode
      }
    }
  }
}

void OverlayChecker::scanForParentErrors() {
  for (const auto& [inodeNumber, inodeInfo] : inodes_) {
    if (inodeInfo.parents.empty()) {
      if (inodeNumber != kRootNodeId) {
        addError<OrphanInode>(inodeInfo);
      }
    } else if (inodeInfo.parents.size() != 1) {
      addError<HardLinkedInode>(inodeInfo);
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
