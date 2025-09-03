/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"

#include <boost/filesystem.hpp>

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/lang/ToAscii.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/Throw.h"
#include "eden/fs/inodes/fscatalog/InodePath.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/utils/EdenError.h"

namespace facebook::eden {

using apache::thrift::CompactSerializer;
using folly::ByteRange;
using folly::fbvector;
using folly::File;
using folly::IOBuf;
using folly::MutableStringPiece;
using folly::StringPiece;
using folly::literals::string_piece_literals::operator""_sp;
using std::optional;
using std::string;

/* Relative to the localDir, the metaFile holds the serialized rendition
 * of the overlay_ data.  We use thrift CompactSerialization for this.
 */
constexpr StringPiece kInfoFile{"info"};
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

static void doFormatSubdirPath(
    uint64_t inodeNum,
    MutableStringPiece subdirPath) {
  constexpr char hexdigit[] = "0123456789abcdef";
  XDCHECK_EQ(subdirPath.size(), FsFileContentStore::kShardDirPathLength);
  subdirPath[0] = hexdigit[(inodeNum >> 4) & 0xf];
  subdirPath[1] = hexdigit[inodeNum & 0xf];
}

bool FsInodeCatalog::initialized() const {
  return core_->initialized();
}

void FsFileContentStore::formatSubdirPath(
    InodeNumber inodeNum,
    MutableStringPiece subdirPath) {
  return doFormatSubdirPath(inodeNum.get(), subdirPath);
}

void FsFileContentStore::formatSubdirShardPath(
    ShardID shardID,
    MutableStringPiece subdirPath) {
  XDCHECK_LE(shardID, 0xfful);
  return doFormatSubdirPath(shardID, subdirPath);
}

bool FsFileContentStore::initialize(
    bool createIfNonExisting,
    bool bypassLockFile) {
  // Read the info file.
  auto infoPath = localDir_ + PathComponentPiece{kInfoFile};
  int fd = folly::openNoInt(infoPath.value().c_str(), O_RDONLY | O_CLOEXEC);
  bool overlayCreated{false};
  if (fd >= 0) {
    // This is an existing overlay directory.
    // Read the info file and make sure we are compatible with its version.
    infoFile_ = File{fd, /* ownsFd */ true};
    validateExistingOverlay(infoFile_.fd());
  } else if (errno != ENOENT) {
    folly::throwSystemError(
        "error reading eden overlay info file ", infoPath.view());
  } else {
    if (!createIfNonExisting) {
      folly::throwSystemError("overlay does not exist at ", localDir_.view());
    }
    // This is a brand new overlay directory.
    initNewOverlay();
    infoFile_ = File{infoPath.value().c_str(), O_RDONLY | O_CLOEXEC};
    overlayCreated = true;
  }

  if (!infoFile_.try_lock() && !bypassLockFile) {
    folly::throwSystemError(
        "failed to acquire overlay lock on ", infoPath.view());
  }

  // Open a handle on the overlay directory itself
  int dirFd = open(
      localDir_.c_str(),
      O_RDONLY |
#ifdef O_PATH
          O_PATH |
#endif
          O_DIRECTORY | O_CLOEXEC);
  folly::checkUnixError(
      dirFd, "error opening overlay directory handle for ", localDir_.value());
  dirFile_ = File{dirFd, /* ownsFd */ true};

  return overlayCreated;
}

std::optional<InodeNumber> FsInodeCatalog::initOverlay(
    bool createIfNonExisting,
    bool bypassLockFile) {
  bool overlayCreated = core_->initialize(createIfNonExisting, bypassLockFile);

  if (overlayCreated) {
    return InodeNumber{kRootNodeId.get() + 1};
  }
  return core_->tryLoadNextInodeNumber();
}

struct statfs FsFileContentStore::statFs() const {
  struct statfs fs = {};
  fstatfs(infoFile_.fd(), &fs);
  return fs;
}

void FsFileContentStore::close() {
  dirFile_.close();
  infoFile_.close();
}

void FsInodeCatalog::close(std::optional<InodeNumber> inodeNumber) {
  if (inodeNumber) {
    core_->saveNextInodeNumber(inodeNumber.value());
  }
  core_->close();
}

std::optional<InodeNumber> FsFileContentStore::tryLoadNextInodeNumber() {
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
      return std::nullopt;
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
    XLOGF(
        WARN,
        "Failed to read entire inode number. Only read {} bytes. Full overlay scan required.",
        readResult);
    return std::nullopt;
  }

  if (nextInodeNumber <= kRootNodeId.get()) {
    XLOGF(
        WARN,
        "Invalid max inode number {}. Full overlay scan required.",
        nextInodeNumber);
    return std::nullopt;
  }
  return InodeNumber{nextInodeNumber};
}

void FsFileContentStore::saveNextInodeNumber(InodeNumber nextInodeNumber) {
  auto nextInodeNumberPath =
      localDir_ + PathComponentPiece{kNextInodeNumberFile};

  auto nextInodeVal = nextInodeNumber.get();
  writeFileAtomic(
      nextInodeNumberPath,
      ByteRange(
          reinterpret_cast<const uint8_t*>(&nextInodeVal),
          reinterpret_cast<const uint8_t*>(&nextInodeVal + 1)))
      .value();
}

void FsFileContentStore::validateExistingOverlay(int infoFD) {
  // Read the info file header
  std::array<uint8_t, kInfoHeaderSize> infoHeader;
  auto sizeRead = folly::readFull(infoFD, infoHeader.data(), infoHeader.size());
  folly::checkUnixError(
      sizeRead, "error reading from overlay info file in ", localDir_.view());
  if (sizeRead != infoHeader.size()) {
    throw_<std::runtime_error>(
        "truncated info file in overlay directory ", localDir_);
  }
  // Verify the magic value is correct
  if (memcmp(
          infoHeader.data(),
          kInfoHeaderMagic.data(),
          kInfoHeaderMagic.size()) != 0) {
    throw_<std::runtime_error>("bad data in overlay info file for ", localDir_);
  }
  // Extract the version number
  uint32_t version;
  memcpy(
      &version, infoHeader.data() + kInfoHeaderMagic.size(), sizeof(version));
  version = folly::Endian::big(version);

  // Make sure we understand this version number
  if (version != kOverlayVersion) {
    throw_<std::runtime_error>(
        "Unsupported eden overlay format ", version, " in ", localDir_);
  }
}

void FsFileContentStore::initNewOverlay() {
  // Make sure the overlay directory itself exists.  It's fine if it already
  // exists (although presumably it should be empty).
  auto result = ::mkdir(localDir_.value().c_str(), 0755);
  if (result != 0 && errno != EEXIST) {
    folly::throwSystemError(
        "error creating eden overlay directory ", localDir_.view());
  }
  auto localDirFile = File(localDir_.view(), O_RDONLY);

  // We split the inode files across 256 subdirectories.
  // Populate these subdirectories now.
  std::array<char, kShardDirPathLength + 1> subdirPath;
  subdirPath[kShardDirPathLength] = '\0';
  MutableStringPiece subdirStringPiece{subdirPath.data(), kShardDirPathLength};
  for (ShardID n = 0; n < kNumShards; ++n) {
    formatSubdirShardPath(n, subdirStringPiece);
    result = ::mkdirat(localDirFile.fd(), subdirPath.data(), 0755);
    if (result != 0 && errno != EEXIST) {
      folly::throwSystemError(
          "error creating eden overlay directory ",
          StringPiece{subdirPath.data()});
    }
  }

  // Create the "tmp" directory
  folly::checkUnixError(
      ::mkdirat(localDirFile.fd(), "tmp", 0700),
      "failed to create overlay tmp directory");

  // For now we just write a simple header, with a magic number to identify
  // this as an eden overlay file, and the version number of the overlay
  // format.
  std::array<uint8_t, kInfoHeaderSize> infoHeader;
  memcpy(infoHeader.data(), kInfoHeaderMagic.data(), kInfoHeaderMagic.size());
  auto version = folly::Endian::big(kOverlayVersion);
  memcpy(
      infoHeader.data() + kInfoHeaderMagic.size(), &version, sizeof(version));

  auto infoPath = localDir_ + PathComponentPiece{kInfoFile};
  writeFileAtomic(infoPath, ByteRange(infoHeader.data(), infoHeader.size()))
      .value();
}

optional<overlay::OverlayDir> FsInodeCatalog::loadOverlayDir(
    InodeNumber inodeNumber) {
  return core_->deserializeOverlayDir(inodeNumber);
}

std::optional<overlay::OverlayDir> FsInodeCatalog::loadAndRemoveOverlayDir(
    InodeNumber inodeNumber) {
  auto result = loadOverlayDir(inodeNumber);
  removeOverlayDir(inodeNumber);
  return result;
}

void FsInodeCatalog::saveOverlayDir(
    InodeNumber inodeNumber,
    overlay::OverlayDir&& odir) {
  // Ask thrift to serialize it.
  auto serializedData = CompactSerializer::serialize<std::string>(odir);

  // Add header to the overlay directory.
  auto header = FsFileContentStore::createHeader(
      FsFileContentStore::kHeaderIdentifierDir,
      FsFileContentStore::kHeaderVersion);

  std::array<struct iovec, 2> iov;
  iov[0].iov_base = header.data();
  iov[0].iov_len = header.size();
  iov[1].iov_base = const_cast<char*>(serializedData.data());
  iov[1].iov_len = serializedData.size();
  (void)core_->createOverlayFileImpl(inodeNumber, iov.data(), iov.size());
}

InodePath FsFileContentStore::getFilePath(InodeNumber inodeNumber) {
  InodePath outPath;
  auto& outPathArray = outPath.rawData();
  formatSubdirPath(
      inodeNumber,
      MutableStringPiece{outPathArray.data(), kShardDirPathLength});
  outPathArray[kShardDirPathLength] = '/';
  auto numberPathStart = kShardDirPathLength + 1;
  auto index = folly::to_ascii_decimal(
      outPathArray.data() + numberPathStart,
      outPathArray.end(),
      inodeNumber.get());
  XDCHECK_LT(index + numberPathStart, outPathArray.size());
  outPathArray[index + numberPathStart] = '\0';
  return outPath;
}

AbsolutePath FsFileContentStore::getAbsoluteFilePath(
    InodeNumber inodeNumber) const {
  auto inodePath = getFilePath(inodeNumber);
  return localDir_ + RelativePathPiece(inodePath.c_str());
}

std::optional<overlay::OverlayDir> FsFileContentStore::deserializeOverlayDir(
    InodeNumber inodeNumber) {
  // Open the file.  Return std::nullopt if the file does not exist.
  auto path = FsFileContentStore::getFilePath(inodeNumber);
  int fd = openat(dirFile_.fd(), path.c_str(), O_RDWR | O_CLOEXEC | O_NOFOLLOW);
  if (fd == -1) {
    int err = errno;
    if (err == ENOENT) {
      // There is no overlay here
      return std::nullopt;
    }
    folly::throwSystemErrorExplicit(
        err,
        fmt::format(
            "error opening overlay file for inode {} in {}",
            inodeNumber,
            localDir_.view()));
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
        errno, "failed to read ", RelativePathPiece{path}.view());
  }

  StringPiece contents{serializedData};
  FsFileContentStore::validateHeader(
      inodeNumber, contents, FsFileContentStore::kHeaderIdentifierDir);
  contents.advance(FsFileContentStore::kHeaderLength);

  return CompactSerializer::deserialize<overlay::OverlayDir>(contents);
}

std::array<uint8_t, FsFileContentStore::kHeaderLength>
FsFileContentStore::createHeader(
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
  appender.writeBE<uint64_t>(0); // atime.tv_sec
  appender.writeBE<uint64_t>(0); // atime.tv_nsec
  appender.writeBE<uint64_t>(0); // ctime.tv_sec
  appender.writeBE<uint64_t>(0); // ctime.tv_nsec
  appender.writeBE<uint64_t>(0); // mtime.tv_sec
  appender.writeBE<uint64_t>(0); // mtime.tv_nsec
  auto paddingSize = kHeaderLength - header.length();
  appender.ensure(paddingSize);
  memset(appender.writableData(), 0, paddingSize);
  appender.append(paddingSize);

  return headerStorage;
}

std::variant<folly::File, InodeNumber> FsFileContentStore::openFile(
    InodeNumber inodeNumber,
    folly::StringPiece headerId) {
  // Open the overlay file
  auto file = std::get<folly::File>(openFileNoVerify(inodeNumber));

  // Read the contents
  std::string contents;
  if (!folly::readFile(file.fd(), contents, kHeaderLength)) {
    folly::throwSystemErrorExplicit(
        errno,
        fmt::format(
            "failed to read overlay file for inode {} in {}",
            inodeNumber,
            localDir_.view()));
  }

  validateHeader(inodeNumber, contents, headerId);
  return file;
}

std::variant<folly::File, InodeNumber> FsFileContentStore::openFileNoVerify(
    InodeNumber inodeNumber) {
  auto path = FsFileContentStore::getFilePath(inodeNumber);

  int fd = openat(dirFile_.fd(), path.c_str(), O_RDWR | O_CLOEXEC | O_NOFOLLOW);
  folly::checkUnixError(
      fd,
      fmt::format(
          "error opening overlay file for inode {} in {}",
          inodeNumber,
          localDir_.view()));
  return folly::File{fd, /* ownsFd */ true};
}

namespace {

constexpr auto tmpPrefix = "tmp/"_sp;
using InodeTmpPath = std::array<
    char,
    tmpPrefix.size() + FsFileContentStore::kMaxDecimalInodeNumberLength + 1>;

InodeTmpPath getFileTmpPath(InodeNumber inodeNumber) {
  // It's substantially faster on XFS to create this temporary file in
  // an empty directory and then move it into its destination rather
  // than to create it directly in the subtree.
  InodeTmpPath tmpPath;
  memcpy(tmpPath.data(), tmpPrefix.data(), tmpPrefix.size());
  auto index = folly::to_ascii_decimal(
      tmpPath.data() + tmpPrefix.size(), tmpPath.end(), inodeNumber.get());
  tmpPath[tmpPrefix.size() + index] = '\0';
  return tmpPath;
}

} // namespace

folly::File FsFileContentStore::createOverlayFileImpl(
    InodeNumber inodeNumber,
    iovec* iov,
    size_t iovCount) {
  // We do not use mkstemp() to create the temporary file, since there is no
  // mkstempat() equivalent that can create files relative to dirFile.  We
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
      fmt::format(
          "failed to create temporary overlay file for inode {} in {}",
          inodeNumber,
          localDir_.view()));
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
      fmt::format(
          "error writing to overlay file for inode {} in {}",
          inodeNumber,
          localDir_.view()));

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
        fmt::format(
            "error flushing data to overlay file for inode {} in {}",
            inodeNumber,
            localDir_.view()));
  }

  auto returnCode =
      renameat(dirFile_.fd(), tmpPath.data(), dirFile_.fd(), path.c_str());
  folly::checkUnixError(
      returnCode,
      fmt::format(
          "error committing overlay file for inode {} in {}",
          inodeNumber,
          localDir_.view()));
  // We do not want to unlink the temporary file on exit now that we have
  // successfully renamed it.
  success = true;

  return file;
}

std::variant<folly::File, InodeNumber> FsFileContentStore::createOverlayFile(
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

std::variant<folly::File, InodeNumber> FsFileContentStore::createOverlayFile(
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

void FsFileContentStore::validateHeader(
    InodeNumber inodeNumber,
    folly::StringPiece contents,
    folly::StringPiece headerId) {
  if (contents.size() < kHeaderLength) {
    // Something wrong with the file (may be corrupted)
    throw newEdenError(
        EIO,
        EdenErrorType::POSIX_ERROR,
        "Overlay file (inode ",
        inodeNumber,
        ") is too short for header: size=",
        contents.size(),
        " expected headerId=",
        headerId);
  }

  IOBuf buf(IOBuf::WRAP_BUFFER, ByteRange{contents});
  folly::io::Cursor cursor(&buf);

  // Validate header identifier
  auto id = cursor.readFixedString(kHeaderIdentifierDir.size());
  StringPiece identifier{id};
  if (identifier.compare(headerId) != 0) {
    throw newEdenError(
        EINVAL,
        EdenErrorType::POSIX_ERROR,
        "unexpected overlay header identifier : ",
        folly::hexlify(ByteRange{identifier}));
  }

  // Validate header version
  auto version = cursor.readBE<uint32_t>();
  if (version != kHeaderVersion) {
    throw newEdenError(
        EINVAL,
        EdenErrorType::POSIX_ERROR,
        "Unexpected overlay version :",
        version);
  }
}

void FsFileContentStore::removeOverlayFile(InodeNumber inodeNumber) {
  auto path = getFilePath(inodeNumber);
  int result = ::unlinkat(dirFile_.fd(), path.c_str(), 0);
  if (result == 0) {
    XLOGF(DBG4, "removed overlay data for inode {}", inodeNumber);
  } else if (errno != ENOENT) {
    folly::throwSystemError(
        "error unlinking overlay file: ", RelativePathPiece{path}.view());
  }
}

void FsInodeCatalog::removeOverlayDir(InodeNumber inodeNumber) {
  core_->removeOverlayFile(inodeNumber);
}

bool FsFileContentStore::hasOverlayFile(InodeNumber inodeNumber) {
  // TODO: It might be worth maintaining a memory-mapped set to rapidly
  // query whether the overlay has an entry for a particular inode.  As it is,
  // this function requires a syscall to see if the overlay has an entry.
  auto path = FsFileContentStore::getFilePath(inodeNumber);
  struct stat st;
  if (0 == fstatat(dirFile_.fd(), path.c_str(), &st, AT_SYMLINK_NOFOLLOW)) {
    return S_ISREG(st.st_mode);
  } else {
    return false;
  }
}

bool FsInodeCatalog::hasOverlayDir(InodeNumber inodeNumber) {
  return core_->hasOverlayFile(inodeNumber);
}

namespace {
overlay::OverlayDir loadDirectoryChildren(folly::File& file) {
  std::string serializedData;
  if (!folly::readFile(file.fd(), serializedData)) {
    folly::throwSystemError("read failed");
  }

  return CompactSerializer::deserialize<overlay::OverlayDir>(serializedData);
}
} // namespace

std::optional<fsck::InodeInfo> FsFileContentStore::loadInodeInfo(
    InodeNumber number) {
  auto inodeError = [number](auto&&... args) -> std::optional<fsck::InodeInfo> {
    return {fsck::InodeInfo(
        number,
        fsck::InodeType::Error,
        fmt::to_string(fmt::join(std::make_tuple(args...), "")))};
  };

  // Open the inode file
  folly::File file;
  try {
    file = std::get<folly::File>(openFileNoVerify(number));
  } catch (const std::exception& ex) {
    return inodeError("error opening file: ", folly::exceptionStr(ex));
  }

  // Read the file header
  std::array<char, FsFileContentStore::kHeaderLength> headerContents;
  auto readResult =
      folly::readFull(file.fd(), headerContents.data(), headerContents.size());
  if (readResult < 0) {
    int errnum = errno;
    return inodeError("error reading from file: ", folly::errnoStr(errnum));
  } else if (readResult != FsFileContentStore::kHeaderLength) {
    return inodeError(fmt::format(
        "file was too short to contain overlay header: read {} bytes, expected {} bytes",
        readResult,
        FsFileContentStore::kHeaderLength));
  }

  // The first 4 bytes of the header are the file type identifier.
  static_assert(
      FsFileContentStore::kHeaderIdentifierDir.size() ==
          FsFileContentStore::kHeaderIdentifierFile.size(),
      "both header IDs must have the same length");
  StringPiece typeID(
      headerContents.data(),
      headerContents.data() + FsFileContentStore::kHeaderIdentifierDir.size());

  // The next 4 bytes are the version ID.
  uint32_t versionBE;
  memcpy(
      &versionBE,
      headerContents.data() + FsFileContentStore::kHeaderIdentifierDir.size(),
      sizeof(uint32_t));
  auto version = folly::Endian::big(versionBE);
  if (version != FsFileContentStore::kHeaderVersion) {
    return inodeError("unknown overlay file format version ", version);
  }

  fsck::InodeType type;
  if (typeID == FsFileContentStore::kHeaderIdentifierDir) {
    type = fsck::InodeType::Dir;
  } else if (typeID == FsFileContentStore::kHeaderIdentifierFile) {
    type = fsck::InodeType::File;
  } else {
    return inodeError(
        "unknown overlay file type ID: ", folly::hexlify(ByteRange{typeID}));
  }

  if (type == fsck::InodeType::Dir) {
    try {
      return {fsck::InodeInfo(number, loadDirectoryChildren(file))};
    } catch (const std::exception& ex) {
      return inodeError(
          "error parsing directory contents: ", folly::exceptionStr(ex));
    }
  } else {
    return {fsck::InodeInfo(number, type)};
  }
}

std::optional<fsck::InodeInfo> FsInodeCatalog::loadInodeInfo(
    InodeNumber number) {
  return core_->loadInodeInfo(number);
}
} // namespace facebook::eden

#endif
