/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/overlay/OverlayChecker.h"

#include <boost/filesystem.hpp>
#include <fcntl.h>
#include <folly/portability/Unistd.h>
#include <ctime>

#include <folly/Conv.h>
#include <folly/ExceptionWrapper.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Format.h>
#include <folly/Overload.h>
#include <folly/String.h>
#include <folly/gen/Base.h>
#include <folly/gen/ParallelMap.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/common/utils/EnumValue.h"
#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"

using apache::thrift::CompactSerializer;
using folly::ByteRange;
using folly::MutableStringPiece;
using folly::StringPiece;
using std::optional;
using std::string;
using std::unique_ptr;
using std::chrono::microseconds;
using std::chrono::seconds;

namespace facebook::eden {

using fsck::InodeInfo;
using fsck::InodeType;

struct OverlayChecker::Impl {
  InodeCatalog* const inodeCatalog;
  FsFileContentStore* const fcs;
  std::optional<InodeNumber> loadedNextInodeNumber;
  InodeCatalog::LookupCallback& lookupCallback;
  std::unordered_map<InodeNumber, InodeInfo> inodes;

  Impl(
      InodeCatalog* inodeCatalog,
      FsFileContentStore* fcs,
      std::optional<InodeNumber> nextInodeNumber,
      InodeCatalog::LookupCallback& lookupCallback)
      : inodeCatalog{inodeCatalog},
        fcs{fcs},
        loadedNextInodeNumber{nextInodeNumber},
        lookupCallback{lookupCallback} {}
};

class OverlayChecker::RepairState {
 public:
  explicit RepairState(OverlayChecker* checker)
      : checker_(checker),
        dir_(createRepairDir(checker_->impl_->fcs->getLocalDir())),
        logFile_(
            (dir_ + PathComponentPiece("fsck.log")).c_str(),
            O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC,
            0600) {}

  void log(std::string_view msg) {
    logLine(msg);
  }

  template <typename Arg1, typename Arg2, typename... Args>
  void log(const Arg1& arg1, const Arg2& arg2, const Args&... args) {
    logLine(fmt::to_string(fmt::join(
        std::make_tuple<const Arg1&, const Arg2&, const Args&...>(
            arg1, arg2, args...),
        "")));
  }

  template <typename Arg1, typename... Args>
  void warn(const Arg1& arg1, const Args&... args) {
    auto msg = fmt::to_string(
        fmt::join(std::make_tuple<const Arg1&, const Args&...>(arg1, args...)));
    XLOGF(WARN, "fsck:{}:{}", checker_->impl_->fcs->getLocalDir(), msg);
    logLine(msg);
  }

  AbsolutePath getRepairDir() const {
    return dir_;
  }

  OverlayChecker* checker() {
    return checker_;
  }
  InodeCatalog* inodeCatalog() {
    return checker_->impl_->inodeCatalog;
  }
  FsFileContentStore* fcs() {
    return checker_->impl_->fcs;
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
        PathComponent(fmt::format("{}{}", number, suffix));
  }

  AbsolutePath getLostAndFoundPath(const OverlayChecker::PathInfo& pathInfo) {
    // Note that we intentionally include pathInfo.parent in the path here,
    // even when it is kRootNodeId.  This helps avoid possible path collisions
    // in the lost+found directory if the root inode contained some children
    // whose names could also be the same as some other inode number.
    return getLostAndFoundPath() +
        PathComponent(fmt::to_string(pathInfo.parent)) + pathInfo.path;
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
    // exists).  If we can find contents id that way, it would be nicer to
    // just dematerialize this inode's entry in its parent directory.
    // That said, in practice most of the times when we have seen files or
    // directories get corrupted they are generated files that are updated
    // frequently by tools, and aren't files we could recover from source
    // control state.  If the files can be recovered from source control, users
    // can always recover it themselves afterwards with `hg revert`
    if (S_ISDIR(mode)) {
      inodeCatalog()->saveOverlayDir(number, overlay::OverlayDir{});
    } else if (S_ISLNK(mode)) {
      // symbolic links generally can't be empty in normal circumstances,
      // so put some dummy data in the link.
      fcs()->createOverlayFile(number, StringPiece("[lost]"));
    } else {
      fcs()->createOverlayFile(number, ByteRange());
    }
  }

  bool dematerializeDirEntry(InodeNumber parent, PathComponent childName) {
    auto parentPath = checker_->computePath(parent);
    auto path = checker_->computePath(parent, childName);
    auto treeOrTreeEntry = checker_->lookup(path.path).getTry();

    if (treeOrTreeEntry.hasValue()) {
      ObjectId id = std::visit(
          folly::overload(
              [](std::shared_ptr<const Tree>& tree) {
                return tree->getObjectId();
              },
              [](TreeEntry& treeEntry) { return treeEntry.getObjectId(); }),
          treeOrTreeEntry.value());

      auto parentDirOpt = inodeCatalog()->loadOverlayDir(parent);
      if (parentDirOpt.has_value()) {
        auto parentDir = parentDirOpt.value();
        auto entries = parentDir.entries();
        if (entries.is_set()) {
          // TODO: this copy can be avoided if we have overlay.thrift specify a
          // map with heterogeneous lookup for entries.
          auto result = entries->find(childName.asString());
          if (result != entries->end()) {
            overlay::OverlayEntry& entry = result->second;
            entry.hash() = id.asString();
            entry.inodeNumber() = 0;

            inodeCatalog()->saveOverlayDir(parent, std::move(parentDir));
            return true;
          }
        }
      }
    } else {
      XLOGF(
          WARN,
          "Unable to compare {} with source control: {}",
          path.path,
          treeOrTreeEntry.exception().what());
    }
    return false;
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
    auto timestampStr = fmt::format(
        "{:04d}{:02d}{:02d}_{:02d}{:02d}{:02d}",
        now.tm_year + 1900,
        now.tm_mon + 1,
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
        path =
            baseDir + PathComponent(fmt::format("{}.{}", timestampStr, iter));
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

  void logLine(std::string_view msg) {
    auto now = std::chrono::system_clock::now().time_since_epoch();
    auto nowSec = std::chrono::duration_cast<seconds>(now);
    auto us = std::chrono::duration_cast<microseconds>(now - nowSec);
    auto timeFields = getLocalTime(nowSec.count());
    auto header = fmt::format(
        "{:04d}-{:02d}-{:02d} {:02d}:{:02d}:{:02d}.{:06d}: ",
        timeFields.tm_year + 1900,
        timeFields.tm_mon + 1,
        timeFields.tm_mday,
        timeFields.tm_hour,
        timeFields.tm_min,
        timeFields.tm_sec,
        us.count());
    auto fullMsg = fmt::format("{}{}\n", header, msg);

    // We don't buffer output to the log file, and write each message
    // immediately.
    auto result =
        folly::writeFull(logFile_.fd(), fullMsg.data(), fullMsg.size());
    if (result == -1) {
      int errnum = errno;
      XLOGF(
          ERR,
          "error writing to fsck repair log file: {}",
          folly::errnoStr(errnum));
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
    return fmt::format(
        "fsck error attempting to enumerate {}: {}", path_, error_.message());
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
    return fmt::format("unexpected file present in overlay: {}", path_);
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
    return fmt::format(
        "found a data file for inode {} in the wrong shard directory ({})",
        number_,
        shardID_);
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
        message_(folly::sformat(std::forward<Args>(args)...)) {}

  string getMessage(OverlayChecker*) const override {
    return fmt::format(
        "error reading data for inode {}: {}", number_, message_);
  }

  bool repair(RepairState& repair) const override {
    // Move the bad file into the lost+found directory
    auto pathInfo = repair.checker()->computePath(number_);
    auto outputPath = repair.getLostAndFoundPath(pathInfo);
    ensureDirectoryExists(outputPath.dirname());
    auto srcPath = repair.fcs()->getAbsoluteFilePath(number_);
    auto ret = ::rename(srcPath.c_str(), outputPath.c_str());
    folly::checkUnixError(
        ret,
        "failed to rename inode data ",
        srcPath.view(),
        " to ",
        outputPath.view());

    // Create replacement data for this inode in the overlay.
    const auto& inodes = repair.checker()->impl_->inodes;
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
    auto fileTypeStr = S_ISDIR(*childInfo_.mode())
        ? "directory"
        : (S_ISLNK(*childInfo_.mode()) ? "symlink" : "file");
    auto path = checker->computePath(parent_, childName_);
    return fmt::format(
        "missing overlay file for materialized {} inode {} ({})",
        fileTypeStr,
        *childInfo_.inodeNumber(),
        path.toString());
  }

  bool repair(RepairState& repair) const override {
    // Create replacement data for this inode in the overlay
    XDCHECK_NE(*childInfo_.inodeNumber(), 0);
    InodeNumber childInodeNumber(*childInfo_.inodeNumber());

    // If we were unable to fetch the scm state of the file, let's replace it
    // with an empty tree/file. This could happen if we're offline during fsck
    // and can't fetch the scm state.
    if (!repair.dematerializeDirEntry(parent_, childName_)) {
      repair.createInodeReplacement(childInodeNumber, *childInfo_.mode());

      // Add an entry in the OverlayChecker's inodes_ set.
      // In case the parent directory was part of an orphaned subtree the
      // OrphanInode code will look for this child in the inodes_ map.
      auto type =
          S_ISDIR(*childInfo_.mode()) ? InodeType::Dir : InodeType::File;
      auto [iter, inserted] = repair.checker()->impl_->inodes.try_emplace(
          childInodeNumber, childInodeNumber, type);
      XDCHECK(inserted);
      iter->second.addParent(parent_, *childInfo_.mode());
    }

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
    return fmt::format(
        "found orphan {} inode {}",
        type_ == InodeType::Dir ? "directory" : "file",
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
        auto iter = repair.checker()->impl_->inodes.find(number_);
        if (iter == repair.checker()->impl_->inodes.end()) {
          XLOGF(
              DFATAL,
              "failed to look up previously-loaded children for orphan directory inode {}",
              number_);
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

    XLOGF(
        DFATAL,
        "unexpected inode type {} when processing orphan inode {}",
        enumValue(type_),
        number_);
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
          fmt::format(
              "failed to create directory to archive orphan directory inode {}",
              number));
    }

    auto* const checker = repair.checker();
    for (const auto& childEntry : *children.entries()) {
      auto childRawInode = *childEntry.second.inodeNumber();
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

    tryRemoveDirInode(repair, number);
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
        archiveOrphanFile(repair, info->number, archivePath, *dirEntry.mode());
        return;
      case InodeType::Dir:
        archiveOrphanDir(repair, info->number, archivePath, info->children);
        return;
      case InodeType::Error:
        processOrphanedError(repair, info->number);
        return;
    }

    XLOGF(
        DFATAL,
        "unexpected inode type {} when processing orphan inode {}",
        enumValue(info->type),
        info->number);
    throw std::runtime_error("unexpected inode type");
  }

  void archiveOrphanFile(
      RepairState& repair,
      InodeNumber number,
      AbsolutePath archivePath,
      mode_t mode) const {
    auto input = std::get<folly::File>(repair.fcs()->openFile(
        number, FsFileContentStore::kHeaderIdentifierFile));

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
          FsFileContentStore::kHeaderLength);
      if (bytesRead < 0) {
        folly::throwSystemError(fmt::format(
            "read error while copying symlink data from inode {} to {}",
            number,
            archivePath.view()));
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
        folly::throwSystemError(fmt::format(
            "read error while copying data from inode {} to {}",
            number,
            archivePath.view()));
      } else if (bytesRead == 0) {
        break;
      }
      auto bytesWritten =
          folly::writeFull(output.fd(), buffer.data(), bytesRead);
      folly::checkUnixError(
          bytesWritten,
          fmt::format(
              "write error while copying data from inode {} to {}",
              number,
              archivePath.view()));
    }

    // Now remove the orphan inode file
    tryRemoveFileInode(repair, number);
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
    tryRemoveFileInode(repair, number);
  }

  void tryRemoveDirInode(RepairState& repair, InodeNumber number) const {
    try {
      repair.inodeCatalog()->removeOverlayDir(number);
    } catch (const std::system_error& ex) {
      // If we fail to remove the file log an error, but proceed with the rest
      // of the fsck repairs rather than letting the exception propagate up
      // to our caller.
      XLOGF(
          ERR,
          "error removing overlay file for orphaned directory inode {} after archiving it: {}",
          number,
          ex.what());
    }
  }

  void tryRemoveFileInode(RepairState& repair, InodeNumber number) const {
    try {
      repair.fcs()->removeOverlayFile(number);
    } catch (const std::system_error& ex) {
      // If we fail to remove the file log an error, but proceed with the rest
      // of the fsck repairs rather than letting the exception propagate up
      // to our caller.
      XLOGF(
          ERR,
          "error removing overlay file for orphaned file inode {} after archiving it: {}",
          number,
          ex.what());
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
    auto msg = fmt::format("found hard linked inode {}:", number_);
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
    return fmt::format(
        "bad stored next inode number: read {} but should be at least {}",
        loadedNumber_,
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
    InodeCatalog* inodeCatalog,
    FsFileContentStore* fcs,
    optional<InodeNumber> nextInodeNumber,
    InodeCatalog::LookupCallback& lookupCallback,
    uint64_t numErrorDiscoveryThreads)
    : impl_{std::make_unique<Impl>(
          inodeCatalog,
          fcs,
          nextInodeNumber,
          lookupCallback)},
      numErrorDiscoveryThreads_{numErrorDiscoveryThreads} {
  XCHECK_GT(numErrorDiscoveryThreads_, 0u);
}

OverlayChecker::~OverlayChecker() = default;

void OverlayChecker::scanForErrors(const ProgressCallback& progressCallback) {
  XLOGF(INFO, "Starting fsck scan on overlay {}", impl_->fcs->getLocalDir());
  if (auto callback = progressCallback) {
    callback(0);
  }
  readInodes(progressCallback);
  linkInodeChildren();
  scanForParentErrors();
  checkNextInodeNumber();

  if (errors_.empty()) {
    XLOGF(
        INFO,
        "fsck:{}: completed checking for errors, no problems found",
        impl_->fcs->getLocalDir());
  } else {
    XLOGF(
        ERR,
        "fsck:{}: completed checking for errors, found {} problems",
        impl_->fcs->getLocalDir(),
        errors_.size());
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
  repair.log("Beginning fsck repair for ", impl_->fcs->getLocalDir());
  repair.log(errors_.size(), " problems detected");

  constexpr size_t maxPrintedErrors = 50;

  size_t errnum = 0;
  for (const auto& error : errors_) {
    ++errnum;
    auto description = error->getMessage(this);
    if (errnum < maxPrintedErrors) {
      XLOGF(ERR, "fsck:{}: error: {}", impl_->fcs->getLocalDir(), description);
    }
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
      XLOGF(
          ERR,
          "fsck:{}: unexpected error occurred while attempting repair: {}",
          impl_->fcs->getLocalDir(),
          folly::exceptionStr(ex));
      repair.log(
          "  ! failed to repair error ",
          errnum,
          ": unexpected exception: ",
          folly::exceptionStr(ex));
    }
  }

  auto numUnfixed = result.totalErrors - result.fixedErrors;
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
  XLOGF(INFO, "fsck:{}: {}", impl_->fcs->getLocalDir(), finalMsg);

  return result;
}

void OverlayChecker::logErrors() {
  for (const auto& error : errors_) {
    XLOGF(
        ERR,
        "fsck:{}: error: {}",
        impl_->fcs->getLocalDir(),
        error->getMessage(this));
  }
}

std::string OverlayChecker::PathInfo::toString() const {
  if (parent == kRootNodeId) {
    return path.value();
  }
  return fmt::format("[unlinked({})]/{}", parent.get(), path.value());
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

InodeInfo* FOLLY_NULLABLE OverlayChecker::getInodeInfo(InodeNumber number) {
  auto iter = impl_->inodes.find(number);
  if (iter == impl_->inodes.end()) {
    return nullptr;
  }
  return &(iter->second);
}

ImmediateFuture<std::variant<std::shared_ptr<const Tree>, TreeEntry>>
OverlayChecker::lookup(RelativePathPiece path) {
  return impl_->lookupCallback(nullptr, path);
}

OverlayChecker::PathInfo OverlayChecker::computePath(InodeNumber number) {
  return cachedPathComputation(number, [&]() {
    auto info = getInodeInfo(number);
    if (!info) {
      // We don't normally expect computePath() to be called on unknown inode
      // numbers.
      XLOGF(WARN, "computePath() called on unknown inode {}", number);
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
    XLOGF(
        DFATAL,
        "bug in fsck code: previously found parent {} of {} but can no longer find parent",
        parent,
        child);
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
  for (const auto& entry : *parentInfo.children.entries()) {
    if (static_cast<uint64_t>(*entry.second.inodeNumber()) == child.get()) {
      return PathComponent(entry.first);
    }
  }

  // This shouldn't ever happen unless we have a bug in the fsck code somehow.
  // We should only get here if linkInodeChildren() found a parent-child
  // relationship between these two inodes, and that relationship shouldn't ever
  // change during the fsck run.
  XLOGF(
      DFATAL,
      "bug in fsck code: cannot find child {} in directory listing of parent {}",
      child,
      parentInfo.number);
  return PathComponent(fmt::format("[missing_child({})]", child));
}

template <typename ErrorType, typename... Args>
std::unique_ptr<OverlayChecker::Error> make_error(Args&&... args) {
  return std::make_unique<ErrorType>(std::forward<Args>(args)...);
}

void OverlayChecker::readInodes(const ProgressCallback& progressCallback) {
  using namespace folly::gen;

  auto threads = numErrorDiscoveryThreads_;
  uint32_t progress10pct = 0;

  folly::Synchronized<std::vector<std::unique_ptr<Error>>> errors;

  // TODO: parallelize these loads
  std::vector<InodeNumber> dirs =
      this->impl_->inodeCatalog->getAllParentInodeNumbers();

  for (const auto& d : dirs) {
    auto inodeInfoOpt = loadInodeInfoFromInodeCatalog(d, errors);
    if (inodeInfoOpt.has_value()) {
      auto inodeInfo = inodeInfoOpt.value();
      updateMaxInodeNumber(inodeInfo.number);
      impl_->inodes.emplace(inodeInfo.number, inodeInfo);
    }
  }

  seq(0u, FsFileContentStore::kNumShards - 1) |
      pmap(
          [this, &errors](
              uint32_t shardID) -> std::vector<std::tuple<uint64_t, uint32_t>> {
            // Get entries in directory
            std::array<char, 2> subdirBuffer;
            MutableStringPiece subdir{subdirBuffer.data(), subdirBuffer.size()};
            FsFileContentStore::formatSubdirShardPath(shardID, subdir);
            auto path = impl_->fcs->getLocalDir() + PathComponentPiece{subdir};

            XLOGF(
                DBG5, "fsck:{}: scanning {}", impl_->fcs->getLocalDir(), path);

            std::vector<std::tuple<uint64_t, uint32_t>> inodes;

            boost::system::error_code error;
            auto boostPath = boost::filesystem::path{path.value().c_str()};
            auto iterator =
                boost::filesystem::directory_iterator(boostPath, error);
            if (error.value() != 0) {
              errors.wlock()->push_back(
                  make_error<ShardDirectoryEnumerationError>(path, error));
              return inodes;
            }

            auto endIterator = boost::filesystem::directory_iterator();
            while (iterator != endIterator) {
              const auto& dirEntry = *iterator;
              AbsolutePath inodePath = canonicalPath(dirEntry.path().string());
              auto entryInodeNumber =
                  folly::tryTo<uint64_t>(inodePath.basename().value());
              if (entryInodeNumber.hasValue()) {
                inodes.emplace_back(*entryInodeNumber, shardID);
              } else {
                errors.wlock()->push_back(
                    make_error<UnexpectedOverlayFile>(inodePath));
              }

              iterator.increment(error);
              if (error.value() != 0) {
                errors.wlock()->push_back(
                    make_error<ShardDirectoryEnumerationError>(path, error));
                break;
              }
            }

            return inodes;
          },
          threads) |
      rconcat | move |
      pmap(
          [this, &errors](std::tuple<uint64_t, uint32_t> result)
              -> std::optional<InodeInfo> {
            return this->loadInodeSharded(
                InodeNumber(std::get<0>(result)), std::get<1>(result), errors);
          },
          threads) |
      move |
      map([this, progressCallback, &progress10pct](
              std::optional<InodeInfo> inodeInfoOpt) -> bool {
        if (inodeInfoOpt.has_value()) {
          auto inodeInfo = inodeInfoOpt.value();
          ShardID shardID = static_cast<ShardID>(inodeInfo.number.get() & 0xff);
          uint32_t progress = (10 * shardID) / FsFileContentStore::kNumShards;
          if (progress > progress10pct) {
            XLOGF(
                INFO,
                "fsck:{}: scan {}0% complete: {} inodes scanned",
                impl_->fcs->getLocalDir(),
                progress,
                impl_->inodes.size());
            if (auto callback = progressCallback) {
              callback(progress);
            }
            progress10pct = progress;
          }

          updateMaxInodeNumber(inodeInfo.number);
          impl_->inodes.emplace(inodeInfo.number, inodeInfo);
          if (impl_->inodes.size() % 10000 == 0) {
            XLOGF(
                DBG5,
                "fsck: {}: scanned {} inodes",
                impl_->fcs->getLocalDir(),
                impl_->inodes.size());
          }
        }
        return true;
      }) |
      count;

  auto errorsLock = errors.wlock();
  while (!errorsLock->empty()) {
    addError(std::move(errorsLock->back()));
    errorsLock->pop_back();
  }

  XLOGF(
      INFO,
      "fsck:{}: scanned {} inodes",
      impl_->fcs->getLocalDir(),
      impl_->inodes.size());
}

std::optional<InodeInfo> OverlayChecker::loadInodeSharded(
    InodeNumber number,
    ShardID shardID,
    folly::Synchronized<std::vector<std::unique_ptr<Error>>>& errors) const {
  XLOGF(DBG9, "fsck: loading inode {}", number);

  // Verify that we found this inode in the correct shard subdirectory.
  // Ignore the data if it is in the wrong directory.
  ShardID expectedShard = static_cast<ShardID>(number.get() & 0xff);
  if (expectedShard != shardID) {
    auto error = make_error<UnexpectedInodeShard>(number, shardID);
    errors.wlock()->push_back(std::move(error));
    return std::nullopt;
  }

  return loadInodeInfoFromFileContentStore(number, errors);
}

std::optional<InodeInfo> OverlayChecker::loadInodeInfoFromInodeCatalog(
    InodeNumber number,
    folly::Synchronized<std::vector<std::unique_ptr<Error>>>& errors) const {
  auto info = this->impl_->inodeCatalog->loadInodeInfo(number);

  if (info.has_value() && info.value().type == InodeType::Error) {
    errors.wlock()->push_back(
        make_error<InodeDataError>(info.value().number, info.value().errorMsg));
  }
  return info;
}

std::optional<InodeInfo> OverlayChecker::loadInodeInfoFromFileContentStore(
    InodeNumber number,
    folly::Synchronized<std::vector<std::unique_ptr<Error>>>& errors) const {
  auto info = this->impl_->fcs->loadInodeInfo(number);

  if (info.has_value() && info.value().type == InodeType::Error) {
    errors.wlock()->push_back(
        make_error<InodeDataError>(info.value().number, info.value().errorMsg));
  }
  return info;
}

void OverlayChecker::linkInodeChildren() {
  for (const auto& [parentInodeNumber, parent] : impl_->inodes) {
    for (const auto& [childName, child] : *parent.children.entries()) {
      auto childRawInode = *child.inodeNumber();
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
        const auto& id = child.hash();
        if (!id.has_value() || id->empty()) {
          // This child is materialized (since it doesn't have a id
          // linking it to a source control object).  It's a problem if the
          // materialized data isn't actually present in the overlay.
          addError<MissingMaterializedInode>(
              parentInodeNumber, childName, child);
        }
      } else {
        childInfo->addParent(parentInodeNumber, *child.mode());

        // TODO: It would be nice to also check for mismatch between
        // childInfo->type and child.mode
      }
    }
  }
}

void OverlayChecker::scanForParentErrors() {
  for (const auto& [inodeNumber, inodeInfo] : impl_->inodes) {
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
  if (impl_->loadedNextInodeNumber.has_value() &&
      *impl_->loadedNextInodeNumber < expectedNextInodeNumber) {
    // contains incorrect data.  (This will probably only happen if someone
    // forced an fsck run even if it looks like the mount was shut down
    // cleanly.)
    addError<BadNextInodeNumber>(
        *impl_->loadedNextInodeNumber, expectedNextInodeNumber);
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
  XLOGF(
      DBG7,
      "fsck: addError() called for {}: {}",
      impl_->fcs->getLocalDir(),
      error->getMessage(this));
  errors_.push_back(std::move(error));
}

} // namespace facebook::eden

#endif
