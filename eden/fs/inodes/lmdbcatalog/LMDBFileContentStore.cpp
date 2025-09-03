/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/lmdbcatalog/LMDBFileContentStore.h"

#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/io/IOBuf.h>

#include "eden/common/utils/FileUtils.h"
#include "eden/fs/inodes/InodeNumber.h"

namespace facebook::eden {

constexpr folly::StringPiece kInfoFile{"info"};

/**
 * 4-byte magic identifier to put at the start of the info file.
 * This merely helps confirm that we are in fact reading an overlay info file
 */
constexpr folly::StringPiece kInfoHeaderMagic{"\xed\xe0\x00\x01"};

/**
 * A version number for the overlay directory format.
 *
 * If we change the overlay storage format in the future we can bump this
 * version number to help identify when eden is reading overlay data created
 by
 * an older version of the code.
 */
constexpr uint32_t kOverlayVersion = 1;
constexpr size_t kInfoHeaderSize =
    kInfoHeaderMagic.size() + sizeof(kOverlayVersion);

class StructuredLogger;

LMDBFileContentStore::LMDBFileContentStore(
    AbsolutePathPiece path,
    std::shared_ptr<StructuredLogger> logger)
    : path_{path}, store_{path, std::move(logger)} {}

bool LMDBFileContentStore::initialize(
    bool createIfNonExisting,
    bool bypassLockFile) {
  // Read the info file.
  auto infoPath = path_ + PathComponentPiece{kInfoFile};
  int fd = folly::openNoInt(infoPath.value().c_str(), O_RDONLY | O_CLOEXEC);
  bool overlayCreated{false};
  if (fd >= 0) {
    // This is an existing overlay directory.
    // Read the info file and make sure we are compatible with its version.
    infoFile_ = folly::File{fd, /* ownsFd */ true};
    validateExistingOverlay(infoFile_.fd());
  } else if (errno != ENOENT) {
    folly::throwSystemError(
        "error reading eden overlay info file ", infoPath.view());
  } else {
    if (!createIfNonExisting) {
      folly::throwSystemError("overlay does not exist at ", path_.view());
    }
    // This is a brand new overlay directory.
    // For now we just write a simple header, with a magic number to identify
    // this as an eden overlay file, and the version number of the overlay
    // format.
    std::array<uint8_t, kInfoHeaderSize> infoHeader;
    memcpy(infoHeader.data(), kInfoHeaderMagic.data(), kInfoHeaderMagic.size());
    auto version = folly::Endian::big(kOverlayVersion);
    memcpy(
        infoHeader.data() + kInfoHeaderMagic.size(), &version, sizeof(version));

    writeFileAtomic(
        infoPath, folly::ByteRange(infoHeader.data(), infoHeader.size()))
        .value();
    infoFile_ = folly::File{infoPath.value().c_str(), O_RDONLY | O_CLOEXEC};
    overlayCreated = true;
  }

  if (!infoFile_.try_lock() && !bypassLockFile) {
    folly::throwSystemError(
        "failed to acquire overlay lock on ", infoPath.view());
  }

  initialized_ = true;
  return overlayCreated;
}

void LMDBFileContentStore::validateExistingOverlay(int infoFD) {
  // Read the info file header
  std::array<uint8_t, kInfoHeaderSize> infoHeader;
  auto sizeRead = folly::readFull(infoFD, infoHeader.data(), infoHeader.size());
  folly::checkUnixError(
      sizeRead, "error reading from overlay info file in ", path_.view());
  if (sizeRead != infoHeader.size()) {
    throw_<std::runtime_error>(
        "truncated info file in overlay directory ", path_);
  }
  // Verify the magic value is correct
  if (memcmp(
          infoHeader.data(),
          kInfoHeaderMagic.data(),
          kInfoHeaderMagic.size()) != 0) {
    throw_<std::runtime_error>("bad data in overlay info file for ", path_);
  }
  // Extract the version number
  uint32_t version;
  memcpy(
      &version, infoHeader.data() + kInfoHeaderMagic.size(), sizeof(version));
  version = folly::Endian::big(version);

  // Make sure we understand this version number
  if (version != kOverlayVersion) {
    throw_<std::runtime_error>(
        "Unsupported eden overlay format ", version, " in ", path_);
  }
}

void LMDBFileContentStore::close() {
  store_.close();
  infoFile_.close();
}

bool LMDBFileContentStore::initialized() const {
  return initialized_ && bool(infoFile_);
}

struct statfs LMDBFileContentStore::statFs() const {
  struct statfs fs = {};
  fstatfs(infoFile_.fd(), &fs);
  return fs;
}

std::variant<folly::File, InodeNumber> LMDBFileContentStore::createOverlayFile(
    InodeNumber inodeNumber,
    folly::ByteRange contents) {
  std::array<struct iovec, 1> iov;
  iov[0].iov_base = const_cast<uint8_t*>(contents.data());
  iov[0].iov_len = contents.size();
  store_.saveBlob(inodeNumber, iov.data(), iov.size());
  return inodeNumber;
}

std::variant<folly::File, InodeNumber> LMDBFileContentStore::createOverlayFile(
    InodeNumber inodeNumber,
    const folly::IOBuf& contents) {
  // In the common case where there is just one element in the chain, use the
  // ByteRange version of createOverlayFile() to avoid having to allocate the
  // iovec array on the heap.
  if (contents.next() == &contents) {
    return createOverlayFile(
        inodeNumber, folly::ByteRange{contents.data(), contents.length()});
  }

  folly::fbvector<struct iovec> iov;
  contents.appendToIov(&iov);
  store_.saveBlob(inodeNumber, iov.data(), iov.size());
  return inodeNumber;
}

std::string LMDBFileContentStore::readOverlayFile(InodeNumber inodeNumber) {
  return store_.loadBlob(inodeNumber);
}

void LMDBFileContentStore::removeOverlayFile(InodeNumber inodeNumber) {
  store_.removeBlob(inodeNumber);
}

std::variant<folly::File, InodeNumber> LMDBFileContentStore::openFile(
    InodeNumber inodeNumber,
    folly::StringPiece) {
  return openFileNoVerify(inodeNumber);
}

std::variant<folly::File, InodeNumber> LMDBFileContentStore::openFileNoVerify(
    InodeNumber inodeNumber) {
  if (!store_.hasBlob(inodeNumber)) {
    folly::throwSystemErrorExplicit(
        ENOENT,
        fmt::format(
            "failed to read overlay file for inode {} in {}",
            inodeNumber,
            path_.view()));
  }
  return inodeNumber;
}

bool LMDBFileContentStore::hasOverlayFile(InodeNumber inodeNumber) {
  return store_.hasBlob(inodeNumber);
}

FileOffset LMDBFileContentStore::allocateOverlayFile(
    InodeNumber inodeNumber,
    FileOffset offset,
    FileOffset length) {
  return store_.allocateBlob(inodeNumber, offset, length);
}

FileOffset LMDBFileContentStore::pwriteOverlayFile(
    InodeNumber inodeNumber,
    const struct iovec* iov,
    int iovcnt,
    FileOffset offset) {
  return store_.pwriteBlob(inodeNumber, iov, iovcnt, offset);
}

FileOffset LMDBFileContentStore::truncateOverlayFile(
    InodeNumber inodeNumber,
    FileOffset length) {
  return store_.truncateBlob(inodeNumber, length);
}

FileOffset LMDBFileContentStore::preadOverlayFile(
    InodeNumber inodeNumber,
    void* buf,
    size_t n,
    FileOffset offset) {
  return store_.preadBlob(inodeNumber, buf, n, offset);
}

FileOffset LMDBFileContentStore::getOverlayFileSize(InodeNumber inodeNumber) {
  return store_.getBlobSize(inodeNumber);
}
} // namespace facebook::eden
