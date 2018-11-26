/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/Overlay.h"

#include <boost/filesystem.hpp>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include <algorithm>
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

using apache::thrift::CompactSerializer;
using folly::ByteRange;
using folly::fbvector;
using folly::File;
using folly::IOBuf;
using folly::MutableStringPiece;
using folly::StringPiece;
using std::optional;
using folly::literals::string_piece_literals::operator""_sp;
using std::string;

/* Relative to the localDir, the metaFile holds the serialized rendition
 * of the overlay_ data.  We use thrift CompactSerialization for this.
 */
constexpr StringPiece kInfoFile{"info"};
constexpr StringPiece kMetadataFile{"metadata.table"};
constexpr const char* kNextInodeNumberFile{"next-inode-number"};

/**
 * 4-byte magic identifier to put at the start of the info file.
 * This merely helps confirm that we are in fact reading an overlay info file
 */
constexpr StringPiece kInfoHeaderMagic{"\xed\xe0\x00\x01"};

/**
 * A version number for the overlay directory format.
 *
 * If we change the overlay storage format in the future we can bump this
 * version number to help identify when eden is reading overlay data created by
 * an older version of the code.
 */
constexpr uint32_t kOverlayVersion = 1;
constexpr size_t kInfoHeaderSize =
    kInfoHeaderMagic.size() + sizeof(kOverlayVersion);

namespace {
/**
 * Get the name of the subdirectory to use for the overlay data for the
 * specified inode number.
 *
 * We shard the inode files across the 256 subdirectories using the least
 * significant byte.  Inode numbers are allocated in monotonically increasing
 * order, so this helps spread them out across the subdirectories.
 */
void formatSubdirPath(MutableStringPiece subdirPath, uint64_t inode) {
  constexpr char hexdigit[] = "0123456789abcdef";
  DCHECK_EQ(subdirPath.size(), 2);
  subdirPath[0] = hexdigit[(inode >> 4) & 0xf];
  subdirPath[1] = hexdigit[inode & 0xf];
}
} // namespace

constexpr folly::StringPiece Overlay::kHeaderIdentifierDir;
constexpr folly::StringPiece Overlay::kHeaderIdentifierFile;
constexpr uint32_t Overlay::kHeaderVersion;
constexpr size_t Overlay::kHeaderLength;

Overlay::Overlay(AbsolutePathPiece localDir) : localDir_(localDir) {
  initOverlay();
  tryLoadNextInodeNumber();

  gcThread_ = std::thread([this] { gcThread(); });
}

Overlay::~Overlay() {
  close();
}

void Overlay::close() {
  CHECK_NE(std::this_thread::get_id(), gcThread_.get_id());

  if (!infoFile_) {
    return;
  }

  // Make sure everything is shut down in reverse of construction order.

  gcQueue_.lock()->stop = true;
  gcCondVar_.notify_one();
  gcThread_.join();

  saveNextInodeNumber();

  inodeMetadataTable_.reset();
  dirFile_.close();
  infoFile_.close();
}

bool Overlay::hasInitializedNextInodeNumber() const {
  // nextInodeNumber_ is either 0 (uninitialized) or nonzero (initialized).
  // It's only initialized on one thread, so relaxed loads are okay.
  return 0 != nextInodeNumber_.load(std::memory_order_relaxed);
}

void Overlay::initOverlay() {
  // Read the info file.
  auto infoPath = localDir_ + PathComponentPiece{kInfoFile};
  int fd = folly::openNoInt(infoPath.value().c_str(), O_RDONLY | O_CLOEXEC);
  if (fd >= 0) {
    // This is an existing overlay directory.
    // Read the info file and make sure we are compatible with its version.
    infoFile_ = File{fd, /* ownsFd */ true};
    readExistingOverlay(infoFile_.fd());
  } else if (errno != ENOENT) {
    folly::throwSystemError(
        "error reading eden overlay info file ", infoPath.stringPiece());
  } else {
    // This is a brand new overlay directory.
    initNewOverlay();
    infoFile_ = File{infoPath.value().c_str(), O_RDONLY | O_CLOEXEC};
  }

  if (!infoFile_.try_lock()) {
    folly::throwSystemError("failed to acquire overlay lock on ", infoPath);
  }

  // Open a handle on the overlay directory itself
  int dirFd =
      open(localDir_.c_str(), O_RDONLY | O_PATH | O_DIRECTORY | O_CLOEXEC);
  folly::checkUnixError(
      dirFd, "error opening overlay directory handle for ", localDir_.value());
  dirFile_ = File{dirFd, /* ownsFd */ true};

  // To support migrating from an older Overlay format, unconditionally create
  // tmp/.
  // TODO: It would be a bit expensive, but it might be worth checking
  // all of the numbered subdirectories here too.
  ensureTmpDirectoryIsCreated();

  // Open after infoFile_'s lock is acquired because the InodeTable acquires
  // its own lock, which should be released prior to infoFile_.
  inodeMetadataTable_ = InodeMetadataTable::open(
      (localDir_ + PathComponentPiece{kMetadataFile}).c_str());
}

void Overlay::tryLoadNextInodeNumber() {
  // If we ever want to extend this file, it should be renamed and a proper
  // header with version number added. In the meantime, we enforce the file is
  // 8 bytes.

  int fd = openat(dirFile_.fd(), kNextInodeNumberFile, O_RDONLY | O_CLOEXEC);
  if (fd == -1) {
    if (errno == ENOENT) {
      // No max inode number file was written which usually means either Eden
      // was not shut down cleanly or an old overlay is being loaded.
      // Either way, a full scan of the overlay is necessary, so leave
      // nextInodeNumber_ at 0.
      return;
    } else {
      folly::throwSystemError("Failed to open ", kNextInodeNumberFile);
    }
  }

  folly::File nextInodeNumberFile{fd, /* ownsFd */ true};

  // Immediately unlink - the presence of the file indicates a clean shutdown.
  if (unlinkat(dirFile_.fd(), kNextInodeNumberFile, 0)) {
    folly::throwSystemError(
        "Failed to unlink ", kNextInodeNumberFile, " in overlay");
  }

  uint64_t nextInodeNumber;
  auto readResult =
      folly::readFull(fd, &nextInodeNumber, sizeof(nextInodeNumber));
  if (readResult < 0) {
    folly::throwSystemError(
        "Failed to read ", kNextInodeNumberFile, " from overlay");
  }
  if (readResult != sizeof(nextInodeNumber)) {
    XLOG(WARN) << "Failed to read entire inode number. Only read " << readResult
               << " bytes. Full overlay scan required.";
    return;
  }

  if (nextInodeNumber <= kRootNodeId.get()) {
    XLOG(WARN) << "Invalid max inode number " << nextInodeNumber
               << ". Full overlay scan required.";
    return;
  }

  nextInodeNumber_.store(nextInodeNumber, std::memory_order_relaxed);
}

void Overlay::saveNextInodeNumber() {
  // nextInodeNumber_ is either 0 (uninitialized) or nonzero (initialized).
  // It's only initialized on one thread, so relaxed loads are okay.
  auto nextInodeNumber = nextInodeNumber_.load(std::memory_order_relaxed);
  if (!nextInodeNumber) {
    return;
  }

  auto nextInodeNumberPath =
      localDir_ + PathComponentPiece{kNextInodeNumberFile};
  folly::writeFileAtomic(
      nextInodeNumberPath.value().c_str(),
      ByteRange(
          reinterpret_cast<const uint8_t*>(&nextInodeNumber),
          reinterpret_cast<const uint8_t*>(&nextInodeNumber + 1)));
}

void Overlay::readExistingOverlay(int infoFD) {
  // Read the info file header
  std::array<uint8_t, kInfoHeaderSize> infoHeader;
  auto sizeRead = folly::readFull(infoFD, infoHeader.data(), infoHeader.size());
  folly::checkUnixError(
      sizeRead,
      "error reading from overlay info file in ",
      localDir_.stringPiece());
  if (sizeRead != infoHeader.size()) {
    throw std::runtime_error(folly::to<string>(
        "truncated info file in overlay directory ", localDir_));
  }
  // Verify the magic value is correct
  if (memcmp(
          infoHeader.data(),
          kInfoHeaderMagic.data(),
          kInfoHeaderMagic.size()) != 0) {
    throw std::runtime_error(
        folly::to<string>("bad data in overlay info file for ", localDir_));
  }
  // Extract the version number
  uint32_t version;
  memcpy(
      &version, infoHeader.data() + kInfoHeaderMagic.size(), sizeof(version));
  version = folly::Endian::big(version);

  // Make sure we understand this version number
  if (version != kOverlayVersion) {
    throw std::runtime_error(folly::to<string>(
        "Unsupported eden overlay format ", version, " in ", localDir_));
  }
}

void Overlay::initNewOverlay() {
  // Make sure the overlay directory itself exists.  It's fine if it already
  // exists (although presumably it should be empty).
  auto result = ::mkdir(localDir_.value().c_str(), 0755);
  if (result != 0 && errno != EEXIST) {
    folly::throwSystemError(
        "error creating eden overlay directory ", localDir_.stringPiece());
  }
  auto localDirFile = File(localDir_.stringPiece(), O_RDONLY);

  // We split the inode files across 256 subdirectories.
  // Populate these subdirectories now.
  std::array<char, 3> subdirPath;
  subdirPath[2] = '\0';
  for (uint64_t n = 0; n < 256; ++n) {
    formatSubdirPath(MutableStringPiece{subdirPath.data(), 2}, n);
    result = ::mkdirat(localDirFile.fd(), subdirPath.data(), 0755);
    if (result != 0 && errno != EEXIST) {
      folly::throwSystemError(
          "error creating eden overlay directory ",
          StringPiece{subdirPath.data()});
    }
  }

  // For now we just write a simple header, with a magic number to identify
  // this as an eden overlay file, and the version number of the overlay
  // format.
  std::array<uint8_t, kInfoHeaderSize> infoHeader;
  memcpy(infoHeader.data(), kInfoHeaderMagic.data(), kInfoHeaderMagic.size());
  auto version = folly::Endian::big(kOverlayVersion);
  memcpy(
      infoHeader.data() + kInfoHeaderMagic.size(), &version, sizeof(version));

  auto infoPath = localDir_ + PathComponentPiece{kInfoFile};
  folly::writeFileAtomic(
      infoPath.stringPiece(), ByteRange(infoHeader.data(), infoHeader.size()));

  // kRootNodeId is reserved - start at the next one. No scan is necessary.
  nextInodeNumber_.store(kRootNodeId.get() + 1, std::memory_order_relaxed);
}

void Overlay::ensureTmpDirectoryIsCreated() {
  struct stat tmpStat;
  int statResult = fstatat(dirFile_.fd(), "tmp", &tmpStat, AT_SYMLINK_NOFOLLOW);
  if (statResult == 0) {
    if (!S_ISDIR(tmpStat.st_mode)) {
      folly::throwSystemErrorExplicit(
          ENOTDIR, "overlay tmp is not a directory");
    }
  } else {
    if (errno == ENOENT) {
      folly::checkUnixError(
          mkdirat(dirFile_.fd(), "tmp", 0700),
          "failed to create overlay tmp directory");
    } else {
      folly::throwSystemError("fstatat(\"tmp\") failed");
    }
  }
}

InodeNumber Overlay::allocateInodeNumber() {
  // InodeNumber should generally be 64-bits wide, in which case it isn't even
  // worth bothering to handle the case where nextInodeNumber_ wraps.  We don't
  // need to bother checking for conflicts with existing inode numbers since
  // this can only happen if we wrap around.  We don't currently support
  // platforms with 32-bit inode numbers.
  static_assert(
      sizeof(nextInodeNumber_) == sizeof(InodeNumber),
      "expected nextInodeNumber_ and InodeNumber to have the same size");
  static_assert(
      sizeof(InodeNumber) >= 8, "expected InodeNumber to be at least 64 bits");

  // This could be a relaxed atomic operation.  It doesn't matter on x86 but
  // might on ARM.
  auto previous = nextInodeNumber_++;
  DCHECK_NE(0, previous) << "allocateInodeNumber called before initialize";
  return InodeNumber{previous};
}

optional<std::pair<DirContents, InodeTimestamps>> Overlay::loadOverlayDir(
    InodeNumber inodeNumber) {
  InodeTimestamps timestamps;
  auto dirData = deserializeOverlayDir(inodeNumber, timestamps);
  if (!dirData.has_value()) {
    return std::nullopt;
  }
  const auto& dir = dirData.value();

  bool shouldMigrateToNewFormat = false;

  DirContents result;
  for (auto& iter : dir.entries) {
    const auto& name = iter.first;
    const auto& value = iter.second;

    bool isMaterialized = !value.__isset.hash || value.hash.empty();
    InodeNumber ino;
    if (value.inodeNumber) {
      ino = InodeNumber::fromThrift(value.inodeNumber);
    } else {
      ino = allocateInodeNumber();
      shouldMigrateToNewFormat = true;
    }

    if (isMaterialized) {
      result.emplace(PathComponentPiece{name}, value.mode, ino);
    } else {
      auto hash = Hash{folly::ByteRange{folly::StringPiece{value.hash}}};
      result.emplace(PathComponentPiece{name}, value.mode, ino, hash);
    }
  }

  if (shouldMigrateToNewFormat) {
    saveOverlayDir(inodeNumber, result);
  }

  return std::pair<DirContents, InodeTimestamps>{std::move(result), timestamps};
}

void Overlay::saveOverlayDir(InodeNumber inodeNumber, const DirContents& dir) {
  auto nextInodeNumber = nextInodeNumber_.load(std::memory_order_relaxed);
  CHECK_LT(inodeNumber.get(), nextInodeNumber)
      << "saveOverlayDir called with unallocated inode number";

  // TODO: T20282158 clean up access of child inode information.
  //
  // Translate the data to the thrift equivalents
  overlay::OverlayDir odir;

  for (auto& entIter : dir) {
    const auto& entName = entIter.first;
    const auto& ent = entIter.second;

    CHECK_LT(ent.getInodeNumber().get(), nextInodeNumber)
        << "saveOverlayDir called with entry using unallocated inode number";

    overlay::OverlayEntry oent;
    oent.mode = ent.getModeUnsafe();
    oent.inodeNumber = ent.getInodeNumber().get();
    bool isMaterialized = ent.isMaterialized();
    if (!isMaterialized) {
      auto entHash = ent.getHash();
      auto bytes = entHash.getBytes();
      oent.set_hash(std::string{reinterpret_cast<const char*>(bytes.data()),
                                bytes.size()});
    }

    odir.entries.emplace(
        std::make_pair(entName.stringPiece().str(), std::move(oent)));
  }

  // Ask thrift to serialize it.
  auto serializedData = CompactSerializer::serialize<std::string>(odir);

  // Add header to the overlay directory.
  auto header = createHeader(kHeaderIdentifierDir, kHeaderVersion);

  std::array<struct iovec, 2> iov;
  iov[0].iov_base = header.data();
  iov[0].iov_len = header.size();
  iov[1].iov_base = const_cast<char*>(serializedData.data());
  iov[1].iov_len = serializedData.size();
  (void)createOverlayFileImpl(inodeNumber, iov.data(), iov.size());
}

void Overlay::removeOverlayData(InodeNumber inodeNumber) {
  // TODO: batch request during GC
  getInodeMetadataTable()->freeInode(inodeNumber);

  auto path = getFilePath(inodeNumber);
  int result = ::unlinkat(dirFile_.fd(), path.c_str(), 0);
  if (result == 0) {
    XLOG(DBG4) << "removed overlay data for inode " << inodeNumber;
  } else if (errno != ENOENT) {
    folly::throwSystemError(
        "error unlinking overlay file: ", RelativePathPiece{path});
  }
}

void Overlay::recursivelyRemoveOverlayData(InodeNumber inodeNumber) {
  InodeTimestamps dummy;
  auto dirData = deserializeOverlayDir(inodeNumber, dummy);
  // This inode's data must be removed from the overlay before
  // recursivelyRemoveOverlayData returns to avoid a race condition if
  // recursivelyRemoveOverlayData(I) is called immediately prior to
  // saveOverlayDir(I).  There's also no risk of violating our durability
  // guarantees if the process dies after this call but before the thread could
  // remove this data.
  removeOverlayData(inodeNumber);

  if (dirData) {
    gcQueue_.lock()->queue.emplace_back(std::move(*dirData));
    gcCondVar_.notify_one();
  }
}

folly::Future<folly::Unit> Overlay::flushPendingAsync() {
  folly::Promise<folly::Unit> promise;
  auto future = promise.getFuture();
  gcQueue_.lock()->queue.emplace_back(std::move(promise));
  gcCondVar_.notify_one();
  return future;
}

bool Overlay::hasOverlayData(InodeNumber inodeNumber) {
  // TODO: It might be worth maintaining a memory-mapped set to rapidly
  // query whether the overlay has an entry for a particular inode.  As it is,
  // this function requires a syscall to see if the overlay has an entry.
  auto path = getFilePath(inodeNumber);
  struct stat st;
  if (0 == fstatat(dirFile_.fd(), path.c_str(), &st, AT_SYMLINK_NOFOLLOW)) {
    return S_ISREG(st.st_mode);
  } else {
    return false;
  }
}

InodeNumber Overlay::scanForNextInodeNumber() {
  if (auto ino = nextInodeNumber_.load(std::memory_order_relaxed)) {
    // Already defined.
    CHECK_GT(ino, 1);
    return InodeNumber{ino - 1};
  }

  // Walk the root directory downwards to find all (non-unlinked) directory
  // inodes stored in the overlay.
  //
  // TODO: It would be nicer if each overlay file contained a short header so
  // we could tell if it was a file or directory.  This way we could do a
  // simpler scan of opening every single file.  For now we have to walk the
  // directory tree from the root downwards.
  auto maxInode = kRootNodeId;
  std::vector<InodeNumber> toProcess;
  toProcess.push_back(maxInode);
  auto encounteredBrokenDirectory = false;
  while (!toProcess.empty()) {
    auto dirInodeNumber = toProcess.back();
    toProcess.pop_back();

    InodeTimestamps timeStamps;
    auto dir = optional<overlay::OverlayDir>{};
    try {
      dir = deserializeOverlayDir(dirInodeNumber, timeStamps);
    } catch (std::system_error& error) {
      XLOG_IF(WARN, !encounteredBrokenDirectory)
          << "Ignoring failure to load directory inode " << dirInodeNumber
          << ": " << error.what();
      encounteredBrokenDirectory = true;
    }
    if (!dir.has_value()) {
      continue;
    }

    for (const auto& entry : dir.value().entries) {
      if (entry.second.inodeNumber == 0) {
        continue;
      }
      auto entryInode = InodeNumber::fromThrift(entry.second.inodeNumber);
      maxInode = std::max(maxInode, entryInode);
      if (mode_to_dtype(entry.second.mode) == dtype_t::Dir) {
        toProcess.push_back(entryInode);
      }
    }
  }

  // Look through the subdirectories and increment maxInode based on the
  // filenames we see.  This is needed in case there are unlinked inodes
  // present.
  std::array<char, 2> subdir;
  for (uint64_t n = 0; n < 256; ++n) {
    formatSubdirPath(MutableStringPiece{subdir.data(), subdir.size()}, n);
    auto subdirPath = localDir_ +
        PathComponentPiece{StringPiece{subdir.data(), subdir.size()}};

    auto boostPath = boost::filesystem::path{subdirPath.value().c_str()};
    for (const auto& entry : boost::filesystem::directory_iterator(boostPath)) {
      auto entryInodeNumber =
          folly::tryTo<uint64_t>(entry.path().filename().string());
      if (entryInodeNumber.hasValue()) {
        maxInode = std::max(maxInode, InodeNumber{entryInodeNumber.value()});
      }
    }
  }

  nextInodeNumber_.store(maxInode.get() + 1, std::memory_order_relaxed);

  return maxInode;
}

Overlay::InodePath Overlay::getFilePath(InodeNumber inodeNumber) {
  InodePath outPath;
  auto& outPathArray = outPath.rawData();
  formatSubdirPath(
      MutableStringPiece{outPathArray.data(), 2}, inodeNumber.get());
  outPathArray[2] = '/';
  auto index =
      folly::uint64ToBufferUnsafe(inodeNumber.get(), outPathArray.data() + 3);
  DCHECK_LT(index + 3, outPathArray.size());
  outPathArray[index + 3] = '\0';
  return outPath;
}

optional<overlay::OverlayDir> Overlay::deserializeOverlayDir(
    InodeNumber inodeNumber,
    InodeTimestamps& timeStamps) const {
  // Open the file.  Return std::nullopt if the file does not exist.
  auto path = getFilePath(inodeNumber);
  int fd = openat(dirFile_.fd(), path.c_str(), O_RDWR | O_CLOEXEC | O_NOFOLLOW);
  if (fd == -1) {
    int err = errno;
    if (err == ENOENT) {
      // There is no overlay here
      return std::nullopt;
    }
    folly::throwSystemErrorExplicit(
        err,
        "error opening overlay file for inode ",
        inodeNumber,
        " in ",
        localDir_);
  }
  folly::File file{fd, /* ownsFd */ true};

  // Read the file data
  std::string serializedData;
  if (!folly::readFile(file.fd(), serializedData)) {
    int err = errno;
    if (err == ENOENT) {
      // There is no overlay here
      return std::nullopt;
    }
    folly::throwSystemErrorExplicit(
        errno, "failed to read ", RelativePathPiece{path});
  }

  // Removing header and deserializing the contents
  if (serializedData.size() < kHeaderLength) {
    // Something Wrong with the file(may be corrupted)
    folly::throwSystemErrorExplicit(
        EIO,
        "Overlay file ",
        RelativePathPiece{path},
        " is too short for header: size=",
        serializedData.size());
  }

  StringPiece header{serializedData, 0, kHeaderLength};
  // validate header and get the timestamps
  parseHeader(header, kHeaderIdentifierDir, timeStamps);

  StringPiece contents{serializedData};
  contents.advance(kHeaderLength);

  return CompactSerializer::deserialize<overlay::OverlayDir>(contents);
}

std::array<uint8_t, Overlay::kHeaderLength> Overlay::createHeader(
    folly::StringPiece identifier,
    uint32_t version) {
  std::array<uint8_t, kHeaderLength> headerStorage;
  IOBuf header{IOBuf::WRAP_BUFFER, folly::MutableByteRange{headerStorage}};
  header.clear();
  folly::io::Appender appender(&header, 0);

  appender.push(identifier);
  appender.writeBE(version);
  // The overlay header used to store timestamps for inodes but that has since
  // been moved to the InodeMetadataTable. Write zeroes instead.
  appender.writeBE<uint64_t>(0);  // atime.tv_sec
  appender.writeBE<uint64_t>(0);  // atime.tv_nsec
  appender.writeBE<uint64_t>(0);  // ctime.tv_sec
  appender.writeBE<uint64_t>(0);  // ctime.tv_nsec
  appender.writeBE<uint64_t>(0);  // mtime.tv_sec
  appender.writeBE<uint64_t>(0);  // mtime.tv_nsec
  auto paddingSize = kHeaderLength - header.length();
  appender.ensure(paddingSize);
  memset(appender.writableData(), 0, paddingSize);
  appender.append(paddingSize);

  return headerStorage;
}

// Helper function to open,validate,
// get file pointer of an overlay file
folly::File Overlay::openFile(
    InodeNumber inodeNumber,
    folly::StringPiece headerId,
    InodeTimestamps& timeStamps) {
  // Open the overlay file
  auto file = openFileNoVerify(inodeNumber);

  // Read the contents
  std::string contents;
  if (!folly::readFile(file.fd(), contents, kHeaderLength)) {
    folly::throwSystemErrorExplicit(
        errno,
        "failed to read overlay file for inode ",
        inodeNumber,
        " in ",
        localDir_);
  }

  StringPiece header{contents};
  parseHeader(header, headerId, timeStamps);
  return file;
}

folly::File Overlay::openFileNoVerify(InodeNumber inodeNumber) {
  auto path = getFilePath(inodeNumber);

  int fd = openat(dirFile_.fd(), path.c_str(), O_RDWR | O_CLOEXEC | O_NOFOLLOW);
  folly::checkUnixError(
      fd,
      "error opening overlay file for inode ",
      inodeNumber,
      " in ",
      localDir_);
  return folly::File{fd, /* ownsFd */ true};
}

namespace {

constexpr auto tmpPrefix = "tmp/"_sp;
using InodeTmpPath = std::
    array<char, tmpPrefix.size() + Overlay::kMaxDecimalInodeNumberLength + 1>;

InodeTmpPath getFileTmpPath(InodeNumber inodeNumber) {
  // It's substantially faster on XFS to create this temporary file in
  // an empty directory and then move it into its destination rather
  // than to create it directly in the subtree.
  InodeTmpPath tmpPath;
  memcpy(tmpPath.data(), tmpPrefix.data(), tmpPrefix.size());
  auto index = folly::uint64ToBufferUnsafe(
      inodeNumber.get(), tmpPath.data() + tmpPrefix.size());
  tmpPath[tmpPrefix.size() + index] = '\0';
  return tmpPath;
}

} // namespace

folly::File Overlay::createOverlayFileImpl(
    InodeNumber inodeNumber,
    iovec* iov,
    size_t iovCount) {
  CHECK_LT(inodeNumber.get(), nextInodeNumber_.load(std::memory_order_relaxed))
      << "createOverlayFile called with unallocated inode number";

  // We do not use mkstemp() to create the temporary file, since there is no
  // mkstempat() equivalent that can create files relative to dirFile_.  We
  // simply create the file with a fixed suffix, and do not use O_EXCL.  This
  // is not a security risk since only the current user should have permission
  // to create files inside the overlay directory, so no one else can create
  // symlinks inside the overlay directory.  We also open the temporary file
  // using O_NOFOLLOW.
  //
  // We could potentially use O_TMPFILE followed by linkat() to commit the
  // file.  However this may not be supported by all filesystems, and seems to
  // provide minimal benefits for our use case.
  auto path = getFilePath(inodeNumber);

  auto tmpPath = getFileTmpPath(inodeNumber);

  auto tmpFD = openat(
      dirFile_.fd(),
      tmpPath.data(),
      O_CREAT | O_RDWR | O_CLOEXEC | O_NOFOLLOW | O_TRUNC,
      0600);
  folly::checkUnixError(
      tmpFD,
      "failed to create temporary overlay file for inode ",
      inodeNumber,
      " in ",
      localDir_);
  folly::File file{tmpFD, /* ownsFd */ true};
  bool success = false;
  SCOPE_EXIT {
    if (!success) {
      unlinkat(dirFile_.fd(), tmpPath.data(), 0);
    }
  };

  auto sizeWritten = folly::writevFull(tmpFD, iov, iovCount);
  folly::checkUnixError(
      sizeWritten,
      "error writing to overlay file for inode ",
      inodeNumber,
      " in ",
      localDir_);

  // fdatasync() is required to ensure that we are really reliably and
  // atomically writing out the new file.  Without calling fdatasync() the file
  // contents may not be flushed to disk even though the rename has been
  // written.
  //
  // However, fdatasync() has a significant performance overhead.  We've
  // measured it at a nearly 300 microsecond cost, which can significantly
  // impact performance of source control update operations when many inodes are
  // affected.
  //
  // Per docs/InodeStorage.md, Eden does not claim to handle disk, kernel, or
  // power failure, so we do not call fdatasync() in the common case.  However,
  // the root inode is particularly important; if its data is corrupt Eden will
  // not be able to remount the checkout.  Therefore we always call fdatasync()
  // when writing out the root inode.
  if (inodeNumber == kRootNodeId) {
    auto syncReturnCode = folly::fdatasyncNoInt(tmpFD);
    folly::checkUnixError(
        syncReturnCode,
        "error flushing data to overlay file for inode ",
        inodeNumber,
        " in ",
        localDir_);
  }

  auto returnCode =
      renameat(dirFile_.fd(), tmpPath.data(), dirFile_.fd(), path.c_str());
  folly::checkUnixError(
      returnCode,
      "error committing overlay file for inode ",
      inodeNumber,
      " in ",
      localDir_);
  // We do not want to unlink the temporary file on exit now that we have
  // successfully renamed it.
  success = true;

  return file;
}

folly::File Overlay::createOverlayFile(
    InodeNumber inodeNumber,
    ByteRange contents) {
  auto header = createHeader(kHeaderIdentifierFile, kHeaderVersion);

  std::array<struct iovec, 2> iov;
  iov[0].iov_base = header.data();
  iov[0].iov_len = header.size();
  iov[1].iov_base = const_cast<uint8_t*>(contents.data());
  iov[1].iov_len = contents.size();
  return createOverlayFileImpl(inodeNumber, iov.data(), iov.size());
}

folly::File Overlay::createOverlayFile(
    InodeNumber inodeNumber,
    const IOBuf& contents) {
  // In the common case where there is just one element in the chain, use the
  // ByteRange version of createOverlayFile() to avoid having to allocate the
  // iovec array on the heap.
  if (contents.next() == &contents) {
    return createOverlayFile(
        inodeNumber, ByteRange{contents.data(), contents.length()});
  }

  auto header = createHeader(kHeaderIdentifierFile, kHeaderVersion);

  fbvector<struct iovec> iov;
  iov.resize(1);
  iov[0].iov_base = header.data();
  iov[0].iov_len = header.size();
  contents.appendToIov(&iov);

  return createOverlayFileImpl(inodeNumber, iov.data(), iov.size());
}

void Overlay::parseHeader(
    folly::StringPiece header,
    folly::StringPiece headerId,
    InodeTimestamps& timestamps) {
  IOBuf buf(IOBuf::WRAP_BUFFER, ByteRange{header});
  folly::io::Cursor cursor(&buf);

  // Validate header identifier
  auto id = cursor.readFixedString(kHeaderIdentifierDir.size());
  StringPiece identifier{id};
  if (identifier.compare(headerId) != 0) {
    folly::throwSystemError(
        EIO,
        "unexpected overlay header identifier : ",
        folly::hexlify(ByteRange{identifier}));
  }

  // Validate header version
  auto version = cursor.readBE<uint32_t>();
  if (version != kHeaderVersion) {
    folly::throwSystemError(EIO, "Unexpected overlay version :", version);
  }
  timespec atime, ctime, mtime;
  atime.tv_sec = cursor.readBE<uint64_t>();
  atime.tv_nsec = cursor.readBE<uint64_t>();
  ctime.tv_sec = cursor.readBE<uint64_t>();
  ctime.tv_nsec = cursor.readBE<uint64_t>();
  mtime.tv_sec = cursor.readBE<uint64_t>();
  mtime.tv_nsec = cursor.readBE<uint64_t>();
  timestamps.atime = atime;
  timestamps.ctime = ctime;
  timestamps.mtime = mtime;
}

void Overlay::gcThread() noexcept {
  for (;;) {
    std::vector<GCRequest> requests;
    {
      auto lock = gcQueue_.lock();
      while (lock->queue.empty()) {
        if (lock->stop) {
          return;
        }
        gcCondVar_.wait(lock.getUniqueLock());
        continue;
      }

      requests = std::move(lock->queue);
    }

    for (auto& request : requests) {
      try {
        handleGCRequest(request);
      } catch (const std::exception& e) {
        XLOG(ERR) << "handleGCRequest should never throw, but it did: "
                  << e.what();
      }
    }
  }
}

void Overlay::handleGCRequest(GCRequest& request) {
  if (request.flush) {
    request.flush->setValue();
    return;
  }

  // Should only include inode numbers for trees.
  std::queue<InodeNumber> queue;

  // TODO: For better throughput on large tree collections, it might make
  // sense to split this into two threads: one for traversing the tree and
  // another that makes the actual unlink calls.
  auto safeRemoveOverlayData = [&](InodeNumber inodeNumber) {
    try {
      removeOverlayData(inodeNumber);
    } catch (const std::exception& e) {
      XLOG(ERR) << "Failed to remove overlay data for inode " << inodeNumber
                << ": " << e.what();
    }
  };

  auto processDir = [&](const overlay::OverlayDir& dir) {
    for (const auto& entry : dir.entries) {
      const auto& value = entry.second;
      if (!value.inodeNumber) {
        // Legacy-only.  All new Overlay trees have inode numbers for all
        // children.
        continue;
      }
      auto ino = InodeNumber::fromThrift(value.inodeNumber);

      if (S_ISDIR(value.mode)) {
        queue.push(ino);
      } else {
        // No need to recurse, but delete any file at this inode.  Note that,
        // under normal operation, there should be nothing at this path
        // because files are only written into the overlay if they're
        // materialized.
        safeRemoveOverlayData(ino);
      }
    }
  };

  processDir(request.dir);

  while (!queue.empty()) {
    auto ino = queue.front();
    queue.pop();

    overlay::OverlayDir dir;
    try {
      InodeTimestamps dummy;
      auto dirData = deserializeOverlayDir(ino, dummy);
      if (!dirData.has_value()) {
        XLOG(DBG3) << "no dir data for inode " << ino;
        continue;
      } else {
        dir = std::move(*dirData);
      }
    } catch (const std::exception& e) {
      XLOG(ERR) << "While collecting, failed to load tree data for inode "
                << ino << ": " << e.what();
      continue;
    }

    safeRemoveOverlayData(ino);
    processDir(dir);
  }
}

Overlay::InodePath::InodePath() noexcept : path_{'\0'} {}

const char* Overlay::InodePath::c_str() const noexcept {
  return path_.data();
}

Overlay::InodePath::operator RelativePathPiece() const noexcept {
  return RelativePathPiece{folly::StringPiece{c_str()}};
}

std::array<char, Overlay::InodePath::kMaxPathLength>&
Overlay::InodePath::rawData() noexcept {
  return path_;
}

} // namespace eden
} // namespace facebook
