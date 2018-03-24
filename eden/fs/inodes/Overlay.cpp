/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Overlay.h"
#include <boost/filesystem.hpp>
#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/gen-cpp2/overlay_types.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

using apache::thrift::CompactSerializer;
using folly::ByteRange;
using folly::fbvector;
using folly::File;
using folly::MutableStringPiece;
using folly::Optional;
using folly::StringPiece;
using std::make_unique;
using std::string;
using std::unique_ptr;

/* Relative to the localDir, the metaFile holds the serialized rendition
 * of the overlay_ data.  We use thrift CompactSerialization for this.
 */
constexpr StringPiece kMetaDir{"overlay"};
constexpr StringPiece kMetaFile{"dirdata"};
constexpr StringPiece kInfoFile{"info"};

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

/* Relative to the localDir, the overlay tree is where we create the
 * materialized directory structure; directories and files are created
 * here. */
constexpr StringPiece kOverlayTree{"tree"};

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
}

void Overlay::initOverlay() {
  // Read the overlay version file.  If it does not exist, create it.
  //
  // First check for an old-format overlay directory, before we wrote out
  // version numbers.  This is only to warn developers if they try to use
  // eden with an existing older client.  We can probably delete this check in
  // a few weeks.
  if (isOldFormatOverlay()) {
    throw std::runtime_error(
        "The eden overlay format has been upgraded. "
        "This version of eden cannot use the old overlay directory at " +
        localDir_.value());
  }

  // Read the info file.
  auto infoPath = localDir_ + PathComponentPiece{kInfoFile};
  int fd = folly::openNoInt(infoPath.value().c_str(), O_RDONLY);
  if (fd >= 0) {
    // This is an existing overlay directory.
    // Read the info file and make sure we are compatible with its version.
    infoFile_ = File{fd, true};
    readExistingOverlay(infoFile_.fd());
  } else if (errno != ENOENT) {
    folly::throwSystemError(
        "error reading eden overlay info file ", infoPath.stringPiece());
  } else {
    // This is a brand new overlay directory.
    initNewOverlay();
    infoFile_ = File{infoPath.value().c_str(), O_RDONLY};
  }

  if (!infoFile_.try_lock()) {
    folly::throwSystemError("failed to acquire overlay lock on ", infoPath);
  }
}

bool Overlay::isOldFormatOverlay() const {
  auto oldDir = localDir_ + PathComponentPiece{kOverlayTree};
  struct stat s;
  if (lstat(oldDir.value().c_str(), &s) == 0) {
    return true;
  }
  return false;
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
}

Optional<TreeInode::Dir> Overlay::loadOverlayDir(
    InodeNumber inodeNumber,
    InodeMap* inodeMap) const {
  TreeInode::Dir result;
  auto dirData = deserializeOverlayDir(inodeNumber, result.timeStamps);
  if (!dirData.hasValue()) {
    return folly::none;
  }
  const auto& dir = dirData.value();

  bool shouldMigrateToNewFormat = false;

  for (auto& iter : dir.entries) {
    const auto& name = iter.first;
    const auto& value = iter.second;

    bool isMaterialized = !value.__isset.hash || value.hash.empty();
    InodeNumber ino;
    if (value.inodeNumber) {
      ino = InodeNumber::fromThrift(value.inodeNumber);
    } else {
      ino = inodeMap->allocateInodeNumber();
      shouldMigrateToNewFormat = true;
    }

    if (isMaterialized) {
      result.entries.emplace(PathComponentPiece{name}, value.mode, ino);
    } else {
      auto hash = Hash{folly::ByteRange{folly::StringPiece{value.hash}}};
      result.entries.emplace(PathComponentPiece{name}, value.mode, ino, hash);
    }
  }

  if (shouldMigrateToNewFormat) {
    saveOverlayDir(inodeNumber, result);
  }

  return folly::Optional<TreeInode::Dir>(std::move(result));
}

void Overlay::saveOverlayDir(InodeNumber inodeNumber, const TreeInode::Dir& dir)
    const {
  // TODO: T20282158 clean up access of child inode information.
  //
  // Translate the data to the thrift equivalents
  overlay::OverlayDir odir;

  for (auto& entIter : dir.entries) {
    const auto& entName = entIter.first;
    const auto& ent = entIter.second;

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
  auto header =
      createHeader(kHeaderIdentifierDir, kHeaderVersion, dir.timeStamps);

  auto iov = header.getIov();
  iov.push_back(
      {const_cast<char*>(serializedData.data()), serializedData.size()});

  // And update the file on disk
  folly::writeFileAtomic(
      getFilePath(inodeNumber).stringPiece(), iov.data(), iov.size());
}

void Overlay::removeOverlayData(InodeNumber inodeNumber) const {
  auto path = getFilePath(inodeNumber);
  if (::unlink(path.value().c_str()) != 0 && errno != ENOENT) {
    folly::throwSystemError("error unlinking overlay file: ", path);
  }
}

InodeNumber Overlay::getMaxRecordedInode() {
  // TODO: We should probably store the max inode number in the header file
  // during graceful unmount.  When opening an overlay we can then simply read
  // back the max inode number from this file if the overlay was shut down
  // cleanly last time.
  //
  // We would only then need to do a scan if the overlay was not cleanly shut
  // down.
  //
  // For now we always do a scan.

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
  while (!toProcess.empty()) {
    auto dirInodeNumber = toProcess.back();
    toProcess.pop_back();

    InodeTimestamps timeStamps;
    auto dir = deserializeOverlayDir(dirInodeNumber, timeStamps);
    if (!dir.hasValue()) {
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

  return maxInode;
}

const AbsolutePath& Overlay::getLocalDir() const {
  return localDir_;
}

AbsolutePath Overlay::getFilePath(InodeNumber inodeNumber) const {
  std::array<char, 2> subdir;
  formatSubdirPath(
      MutableStringPiece{subdir.data(), subdir.size()}, inodeNumber.get());
  auto numberStr = folly::to<string>(inodeNumber);
  return localDir_ +
      PathComponentPiece{StringPiece{subdir.data(), subdir.size()}} +
      PathComponentPiece{numberStr};
}

Optional<overlay::OverlayDir> Overlay::deserializeOverlayDir(
    InodeNumber inodeNumber,
    InodeTimestamps& timeStamps) const {
  auto path = getFilePath(inodeNumber);

  // Read the file and de-serialize it into data
  std::string serializedData;
  if (!folly::readFile(path.value().c_str(), serializedData)) {
    int err = errno;
    if (err == ENOENT) {
      // There is no overlay here
      return folly::none;
    }
    folly::throwSystemErrorExplicit(err, "failed to read ", path);
  }

  // Removing header and deserializing the contents
  if (serializedData.size() < kHeaderLength) {
    // Something Wrong with the file(may be corrupted)
    folly::throwSystemErrorExplicit(
        EIO,
        "Overlay file ",
        path,
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

// Overlay version number is currently 32 bit,
// so making version uint32_t instead of uint16_t
folly::IOBuf Overlay::createHeader(
    StringPiece identifier,
    uint32_t version,
    const InodeTimestamps& timestamps) {
  folly::IOBuf header(folly::IOBuf::CREATE, kHeaderLength);
  folly::io::Appender appender(&header, 0);
  appender.push(identifier);
  appender.writeBE(version);
  auto atime = timestamps.atime.toTimespec();
  auto ctime = timestamps.ctime.toTimespec();
  auto mtime = timestamps.mtime.toTimespec();
  appender.writeBE<uint64_t>(atime.tv_sec);
  appender.writeBE<uint64_t>(atime.tv_nsec);
  appender.writeBE<uint64_t>(ctime.tv_sec);
  appender.writeBE<uint64_t>(ctime.tv_nsec);
  appender.writeBE<uint64_t>(mtime.tv_sec);
  appender.writeBE<uint64_t>(mtime.tv_nsec);
  auto paddingSize = kHeaderLength - header.length();
  appender.ensure(paddingSize);
  memset(appender.writableData(), 0, paddingSize);
  appender.append(paddingSize);

  return header;
}

// Helper function to open,validate,
// get file pointer of an overlay file
folly::File Overlay::openFile(
    folly::StringPiece filePath,
    folly::StringPiece headerId,
    InodeTimestamps& timeStamps) {
  // Open the overlay file
  folly::File file(filePath, O_RDWR);

  // Read the contents
  std::string contents;
  folly::readFile(file.fd(), contents, kHeaderLength);

  StringPiece header{contents};
  parseHeader(header, headerId, timeStamps);
  return file;
}

// Helper function to  add header to the materialized file
void Overlay::addHeaderToOverlayFile(int fd, timespec ctime) {
  InodeTimestamps ts{ctime};
  auto header = createHeader(kHeaderIdentifierFile, kHeaderVersion, ts);

  auto data = header.coalesce();
  auto wrote = folly::writeFull(fd, data.data(), data.size());

  if (wrote == -1) {
    folly::throwSystemError("writeNoInt failed");
  }
  if (wrote != data.size()) {
    folly::throwSystemError(
        "writeNoInt wrote only ", wrote, " of ", data.size(), " bytes");
  }
}

// Helper function to create an overlay file
folly::File Overlay::createOverlayFile(
    InodeNumber childNumber,
    timespec ctime) {
  auto filePath = getFilePath(childNumber);
  folly::File file(filePath.c_str(), O_RDWR | O_CREAT | O_EXCL, 0600);

  SCOPE_FAIL {
    ::unlink(filePath.c_str());
  };

  addHeaderToOverlayFile(file.fd(), ctime);
  return file;
}

void Overlay::parseHeader(
    folly::StringPiece header,
    folly::StringPiece headerId,
    InodeTimestamps& timestamps) {
  folly::IOBuf buf(folly::IOBuf::WRAP_BUFFER, ByteRange{header});
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
// Helper function to update timestamps into overlay file
void Overlay::updateTimestampToHeader(
    int fd,
    const InodeTimestamps& timestamps) {
  // Create a string piece with timestamps
  std::array<uint64_t, 6> buf;
  folly::IOBuf iobuf(folly::IOBuf::WRAP_BUFFER, buf.data(), sizeof(buf));
  iobuf.clear();

  folly::io::Appender appender(&iobuf, 0);
  auto atime = timestamps.atime.toTimespec();
  auto ctime = timestamps.ctime.toTimespec();
  auto mtime = timestamps.mtime.toTimespec();
  appender.writeBE<uint64_t>(atime.tv_sec);
  appender.writeBE<uint64_t>(atime.tv_nsec);
  appender.writeBE<uint64_t>(ctime.tv_sec);
  appender.writeBE<uint64_t>(ctime.tv_nsec);
  appender.writeBE<uint64_t>(mtime.tv_sec);
  appender.writeBE<uint64_t>(mtime.tv_nsec);

  // replace the timestamps of current header with the new timestamps
  auto newHeader = iobuf.coalesce();
  auto wrote = folly::pwriteNoInt(
      fd,
      newHeader.data(),
      newHeader.size(),
      kHeaderIdentifierDir.size() + sizeof(kHeaderVersion));
  if (wrote == -1) {
    folly::throwSystemError("pwriteNoInt failed");
  }
  if (wrote != newHeader.size()) {
    folly::throwSystemError(
        "writeNoInt wrote only ", wrote, " of ", newHeader.size(), " bytes");
  }
}
} // namespace eden
} // namespace facebook
