/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/overlay/FsOverlay.h"

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
#include <chrono>
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

constexpr folly::StringPiece FsOverlay::kMetadataFile;

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

constexpr folly::StringPiece FsOverlay::kHeaderIdentifierDir;
constexpr folly::StringPiece FsOverlay::kHeaderIdentifierFile;
constexpr uint32_t FsOverlay::kHeaderVersion;
constexpr size_t FsOverlay::kHeaderLength;

static void doFormatSubdirPath(
    MutableStringPiece subdirPath,
    uint64_t inodeNum) {
  constexpr char hexdigit[] = "0123456789abcdef";
  DCHECK_EQ(subdirPath.size(), 2);
  subdirPath[0] = hexdigit[(inodeNum >> 4) & 0xf];
  subdirPath[1] = hexdigit[inodeNum & 0xf];
}

void FsOverlay::formatSubdirPath(
    MutableStringPiece subdirPath,
    InodeNumber inodeNum) {
  return doFormatSubdirPath(subdirPath, inodeNum.get());
}

std::optional<InodeNumber> FsOverlay::initOverlay(bool createIfNonExisting) {
  // Read the info file.
  auto infoPath = localDir_ + PathComponentPiece{kInfoFile};
  int fd = folly::openNoInt(infoPath.value().c_str(), O_RDONLY | O_CLOEXEC);
  bool overlayCreated{false};
  if (fd >= 0) {
    // This is an existing overlay directory.
    // Read the info file and make sure we are compatible with its version.
    infoFile_ = File{fd, /* ownsFd */ true};
    readExistingOverlay(infoFile_.fd());
  } else if (errno != ENOENT) {
    folly::throwSystemError(
        "error reading eden overlay info file ", infoPath.stringPiece());
  } else {
    if (!createIfNonExisting) {
      folly::throwSystemError("overlay does not exist at ", localDir_);
    }
    // This is a brand new overlay directory.
    initNewOverlay();
    infoFile_ = File{infoPath.value().c_str(), O_RDONLY | O_CLOEXEC};
    overlayCreated = true;
  }

  if (!infoFile_.try_lock()) {
    folly::throwSystemError("failed to acquire overlay lock on ", infoPath);
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

  if (overlayCreated) {
    return InodeNumber{kRootNodeId.get() + 1};
  }
  return tryLoadNextInodeNumber();
}

void FsOverlay::close(std::optional<InodeNumber> inodeNumber) {
  if (inodeNumber) {
    saveNextInodeNumber(inodeNumber.value());
  }
  dirFile_.close();
  infoFile_.close();
}

std::optional<InodeNumber> FsOverlay::tryLoadNextInodeNumber() {
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
    XLOG(WARN) << "Failed to read entire inode number. Only read " << readResult
               << " bytes. Full overlay scan required.";
    return std::nullopt;
  }

  if (nextInodeNumber <= kRootNodeId.get()) {
    XLOG(WARN) << "Invalid max inode number " << nextInodeNumber
               << ". Full overlay scan required.";
    return std::nullopt;
  }
  return InodeNumber{nextInodeNumber};
}

void FsOverlay::saveNextInodeNumber(InodeNumber nextInodeNumber) {
  auto nextInodeNumberPath =
      localDir_ + PathComponentPiece{kNextInodeNumberFile};

  auto nextInodeVal = nextInodeNumber.get();
  folly::writeFileAtomic(
      nextInodeNumberPath.value().c_str(),
      ByteRange(
          reinterpret_cast<const uint8_t*>(&nextInodeVal),
          reinterpret_cast<const uint8_t*>(&nextInodeVal + 1)));
}

InodeNumber FsOverlay::scanForNextInodeNumber() {
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

    auto dir = optional<overlay::OverlayDir>{};
    try {
      dir = loadOverlayDir(dirInodeNumber);
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
    doFormatSubdirPath(MutableStringPiece{subdir.data(), subdir.size()}, n);
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
  return InodeNumber{maxInode.get() + 1};
}

void FsOverlay::readExistingOverlay(int infoFD) {
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

void FsOverlay::initNewOverlay() {
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
    doFormatSubdirPath(MutableStringPiece{subdirPath.data(), 2}, n);
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
}

void FsOverlay::ensureTmpDirectoryIsCreated() {
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

optional<overlay::OverlayDir> FsOverlay::loadOverlayDir(
    InodeNumber inodeNumber) {
  return deserializeOverlayDir(inodeNumber);
}

void FsOverlay::saveOverlayDir(
    InodeNumber inodeNumber,
    const overlay::OverlayDir& odir) {
  // Ask thrift to serialize it.
  auto serializedData = CompactSerializer::serialize<std::string>(odir);

  // Add header to the overlay directory.
  auto header = FsOverlay::createHeader(kHeaderIdentifierDir, kHeaderVersion);

  std::array<struct iovec, 2> iov;
  iov[0].iov_base = header.data();
  iov[0].iov_len = header.size();
  iov[1].iov_base = const_cast<char*>(serializedData.data());
  iov[1].iov_len = serializedData.size();
  (void)createOverlayFileImpl(inodeNumber, iov.data(), iov.size());
}

InodePath FsOverlay::getFilePath(InodeNumber inodeNumber) {
  InodePath outPath;
  auto& outPathArray = outPath.rawData();
  formatSubdirPath(MutableStringPiece{outPathArray.data(), 2}, inodeNumber);
  outPathArray[2] = '/';
  auto index =
      folly::uint64ToBufferUnsafe(inodeNumber.get(), outPathArray.data() + 3);
  DCHECK_LT(index + 3, outPathArray.size());
  outPathArray[index + 3] = '\0';
  return outPath;
}

std::optional<overlay::OverlayDir> FsOverlay::deserializeOverlayDir(
    InodeNumber inodeNumber) {
  // Open the file.  Return std::nullopt if the file does not exist.
  auto path = FsOverlay::getFilePath(inodeNumber);
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

  StringPiece contents{serializedData};
  FsOverlay::validateHeader(
      inodeNumber, contents, FsOverlay::kHeaderIdentifierDir);
  contents.advance(FsOverlay::kHeaderLength);

  return CompactSerializer::deserialize<overlay::OverlayDir>(contents);
}

std::array<uint8_t, FsOverlay::kHeaderLength> FsOverlay::createHeader(
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

folly::File FsOverlay::openFile(
    InodeNumber inodeNumber,
    folly::StringPiece headerId) {
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

  validateHeader(inodeNumber, contents, headerId);
  return file;
}

folly::File FsOverlay::openFileNoVerify(InodeNumber inodeNumber) {
  auto path = FsOverlay::getFilePath(inodeNumber);

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
    array<char, tmpPrefix.size() + FsOverlay::kMaxDecimalInodeNumberLength + 1>;

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

folly::File FsOverlay::createOverlayFileImpl(
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

folly::File FsOverlay::createOverlayFile(
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

folly::File FsOverlay::createOverlayFile(
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

void FsOverlay::validateHeader(
    InodeNumber inodeNumber,
    folly::StringPiece contents,
    folly::StringPiece headerId) {
  if (contents.size() < kHeaderLength) {
    // Something wrong with the file (may be corrupted)
    folly::throwSystemErrorExplicit(
        EIO,
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

  // Eden used to store timestamps in the Overlay entry's header, but they're
  // no longer used. Read them anyway and throw an exception on error.
  cursor.readBE<uint64_t>(); // atime.tv_sec
  cursor.readBE<uint64_t>(); // atime.tv_nsec
  cursor.readBE<uint64_t>(); // ctime.tv_sec
  cursor.readBE<uint64_t>(); // ctime.tv_nsec
  cursor.readBE<uint64_t>(); // mtime.tv_sec
  cursor.readBE<uint64_t>(); // mtime.tv_nsec
}

void FsOverlay::removeOverlayFile(InodeNumber inodeNumber) {
  auto path = getFilePath(inodeNumber);
  int result = ::unlinkat(dirFile_.fd(), path.c_str(), 0);
  if (result == 0) {
    XLOG(DBG4) << "removed overlay data for inode " << inodeNumber;
  } else if (errno != ENOENT) {
    folly::throwSystemError(
        "error unlinking overlay file: ", RelativePathPiece{path});
  }
}

void FsOverlay::writeNextInodeNumber(InodeNumber nextInodeNumber) {
  auto nextInodeNumberPath =
      localDir_ + PathComponentPiece{kNextInodeNumberFile};

  folly::writeFileAtomic(
      nextInodeNumberPath.value().c_str(),
      ByteRange(
          reinterpret_cast<const uint8_t*>(&nextInodeNumber),
          reinterpret_cast<const uint8_t*>(&nextInodeNumber + 1)));
}

bool FsOverlay::hasOverlayData(InodeNumber inodeNumber) {
  // TODO: It might be worth maintaining a memory-mapped set to rapidly
  // query whether the overlay has an entry for a particular inode.  As it is,
  // this function requires a syscall to see if the overlay has an entry.
  auto path = FsOverlay::getFilePath(inodeNumber);
  struct stat st;
  if (0 == fstatat(dirFile_.fd(), path.c_str(), &st, AT_SYMLINK_NOFOLLOW)) {
    return S_ISREG(st.st_mode);
  } else {
    return false;
  }
}

InodePath::InodePath() noexcept : path_{'\0'} {}

const char* InodePath::c_str() const noexcept {
  return path_.data();
}

InodePath::operator RelativePathPiece() const noexcept {
  return RelativePathPiece{folly::StringPiece{c_str()}};
}

std::array<char, InodePath::kMaxPathLength>& InodePath::rawData() noexcept {
  return path_;
}

} // namespace eden
} // namespace facebook
