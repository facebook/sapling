/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include <gtest/gtest.h>
#include <sys/stat.h>
#include <cstdint>
#include <cstring>
#include <string>

#include <folly/FileUtil.h>
#include <folly/testing/TestUtil.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/fscatalog/InodePath.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"

using namespace facebook::eden;

TEST(
    FsInodeCatalogWalPathTest,
    getWalPath_producesShardedPathWithDotWalSuffix) {
  // Inode 0xab is sharded by its low byte, so the shard dir is "ab" and the
  // filename is its decimal form with the ".wal" suffix.
  auto path = FsFileContentStore::getWalPath(InodeNumber{0xab});
  EXPECT_STREQ("ab/171.wal", path.c_str());
}

TEST(FsInodeCatalogWalPathTest, getWalPath_handlesMaxUint64Inode) {
  // The maximum inode number must still fit within WalPath::kMaxPathLength.
  // Low byte of UINT64_MAX is 0xff so the shard dir is "ff".
  auto path = FsFileContentStore::getWalPath(InodeNumber{UINT64_MAX});
  EXPECT_STREQ("ff/18446744073709551615.wal", path.c_str());
}

class FsInodeCatalogWalTest : public ::testing::Test {
 protected:
  void SetUp() override {
    store_ = std::make_unique<FsFileContentStore>(
        canonicalPath(testDir_.path().string()));
    store_->initialize(/*createIfNonExisting=*/true);
  }

  void TearDown() override {
    if (store_) {
      store_->close();
      store_.reset();
    }
  }

  /// Read the raw bytes of the WAL file for `parent`.
  std::string readWal(InodeNumber parent) const {
    auto walPath = FsFileContentStore::getWalPath(parent);
    auto fullPath =
        canonicalPath(testDir_.path().string()) + RelativePathPiece{walPath};
    std::string out;
    if (!folly::readFile(fullPath.c_str(), out)) {
      return {};
    }
    return out;
  }

  folly::test::TemporaryDirectory testDir_;
  std::unique_ptr<FsFileContentStore> store_;
};

namespace {

// Wire-format field sizes; mirrors appendWalEntry in FsInodeCatalog.cpp.
constexpr size_t kEntryLenSize = sizeof(uint32_t);
constexpr size_t kOpByteSize = sizeof(uint8_t);
constexpr size_t kNameLenSize = sizeof(uint16_t);
constexpr size_t kModeSize = sizeof(int32_t);
constexpr size_t kInodeNumberSize = sizeof(int64_t);
constexpr size_t kHashLenSize = sizeof(uint8_t);
// 3-character names ("foo", "bar", ...) are used by the assertions below.
constexpr size_t kTestNameSize = 3;

overlay::OverlayEntry makeEntry(int32_t mode, int64_t inodeNumber) {
  overlay::OverlayEntry entry;
  entry.mode() = mode;
  entry.inodeNumber() = inodeNumber;
  return entry;
}

overlay::OverlayEntry
makeEntryWithHash(int32_t mode, int64_t inodeNumber, std::string hash) {
  overlay::OverlayEntry entry = makeEntry(mode, inodeNumber);
  entry.hash() = std::move(hash);
  return entry;
}

// Decode helpers — operate on raw WAL bytes without depending on replayWal.
uint32_t readU32(const std::string& data, size_t offset) {
  uint32_t v = 0;
  std::memcpy(&v, data.data() + offset, sizeof(v));
  return v;
}

uint16_t readU16(const std::string& data, size_t offset) {
  uint16_t v = 0;
  std::memcpy(&v, data.data() + offset, sizeof(v));
  return v;
}

int32_t readI32(const std::string& data, size_t offset) {
  int32_t v = 0;
  std::memcpy(&v, data.data() + offset, sizeof(v));
  return v;
}

int64_t readI64(const std::string& data, size_t offset) {
  int64_t v = 0;
  std::memcpy(&v, data.data() + offset, sizeof(v));
  return v;
}

} // namespace

TEST_F(FsInodeCatalogWalTest, appendWalEntry_writesAddEntryWithHash) {
  const InodeNumber parent{1};
  auto entry = makeEntryWithHash(0100644, 42, std::string(20, '\xab'));
  store_->appendWalEntry(
      parent,
      FsFileContentStore::WalOpType::ADD,
      PathComponentPiece{"foo"},
      &entry);

  auto bytes = readWal(parent);
  // entryLen header.
  ASSERT_GE(bytes.size(), kEntryLenSize);
  uint32_t entryLen = readU32(bytes, 0);
  EXPECT_EQ(bytes.size(), kEntryLenSize + entryLen);

  size_t off = kEntryLenSize;
  EXPECT_EQ(
      static_cast<uint8_t>(FsFileContentStore::WalOpType::ADD),
      static_cast<uint8_t>(bytes[off]));
  off += kOpByteSize;
  EXPECT_EQ(kTestNameSize, readU16(bytes, off));
  off += kNameLenSize;
  EXPECT_EQ("foo", bytes.substr(off, kTestNameSize));
  off += kTestNameSize;
  EXPECT_EQ(0100644, readI32(bytes, off));
  off += kModeSize;
  EXPECT_EQ(42, readI64(bytes, off));
  off += kInodeNumberSize;
  EXPECT_EQ(20, static_cast<uint8_t>(bytes[off]));
  off += kHashLenSize;
  EXPECT_EQ(std::string(20, '\xab'), bytes.substr(off, 20));
}

TEST_F(FsInodeCatalogWalTest, appendWalEntry_writesAddEntryWithoutHash) {
  const InodeNumber parent{2};
  auto entry = makeEntry(0100644, 7);
  store_->appendWalEntry(
      parent,
      FsFileContentStore::WalOpType::ADD,
      PathComponentPiece{"bar"},
      &entry);

  auto bytes = readWal(parent);
  ASSERT_GE(bytes.size(), kEntryLenSize);
  uint32_t entryLen = readU32(bytes, 0);
  EXPECT_EQ(bytes.size(), kEntryLenSize + entryLen);

  // hashLen byte at the tail must be zero, no trailing hash bytes.
  size_t off = kEntryLenSize + kOpByteSize + kNameLenSize + kTestNameSize +
      kModeSize + kInodeNumberSize;
  EXPECT_EQ(0, static_cast<uint8_t>(bytes[off]));
  EXPECT_EQ(off + kHashLenSize, bytes.size());
}

TEST_F(FsInodeCatalogWalTest, appendWalEntry_writesRemoveEntry) {
  const InodeNumber parent{3};
  store_->appendWalEntry(
      parent,
      FsFileContentStore::WalOpType::REMOVE,
      PathComponentPiece{"baz"},
      nullptr);

  auto bytes = readWal(parent);
  // REMOVE payload = opByte + nameLen + name.
  constexpr size_t kRemovePayloadSize =
      kOpByteSize + kNameLenSize + kTestNameSize;
  EXPECT_EQ(kEntryLenSize + kRemovePayloadSize, bytes.size());
  EXPECT_EQ(kRemovePayloadSize, readU32(bytes, 0));
  EXPECT_EQ(
      static_cast<uint8_t>(FsFileContentStore::WalOpType::REMOVE),
      static_cast<uint8_t>(bytes[kEntryLenSize]));
  EXPECT_EQ(kTestNameSize, readU16(bytes, kEntryLenSize + kOpByteSize));
  EXPECT_EQ(
      "baz",
      bytes.substr(kEntryLenSize + kOpByteSize + kNameLenSize, kTestNameSize));
}

TEST_F(FsInodeCatalogWalTest, appendWalEntry_writesMaterializeEntry) {
  const InodeNumber parent{4};
  store_->appendWalEntry(
      parent,
      FsFileContentStore::WalOpType::MATERIALIZE,
      PathComponentPiece{"qux"},
      nullptr);

  auto bytes = readWal(parent);
  // MATERIALIZE has the same shape as REMOVE: just a name, no payload.
  constexpr size_t kMaterializePayloadSize =
      kOpByteSize + kNameLenSize + kTestNameSize;
  EXPECT_EQ(kEntryLenSize + kMaterializePayloadSize, bytes.size());
  EXPECT_EQ(
      static_cast<uint8_t>(FsFileContentStore::WalOpType::MATERIALIZE),
      static_cast<uint8_t>(bytes[kEntryLenSize]));
}

TEST_F(FsInodeCatalogWalTest, appendWalEntry_appendsAcrossMultipleCalls) {
  const InodeNumber parent{5};
  store_->appendWalEntry(
      parent,
      FsFileContentStore::WalOpType::REMOVE,
      PathComponentPiece{"a"},
      nullptr);
  auto sizeAfterFirst = readWal(parent).size();
  store_->appendWalEntry(
      parent,
      FsFileContentStore::WalOpType::REMOVE,
      PathComponentPiece{"b"},
      nullptr);
  auto bytes = readWal(parent);

  // The second entry must be appended after the first, not overwrite it.
  EXPECT_EQ(2 * sizeAfterFirst, bytes.size());
  // The second entry begins at sizeAfterFirst with its own length prefix.
  // Payload = opByte + nameLen + 1-char name "b".
  constexpr size_t kSingleCharRemovePayload = kOpByteSize + kNameLenSize + 1;
  EXPECT_EQ(kSingleCharRemovePayload, readU32(bytes, sizeAfterFirst));
}

TEST_F(FsInodeCatalogWalTest, appendWalEntry_createsWalFileWithMode0600) {
  const InodeNumber parent{6};
  store_->appendWalEntry(
      parent,
      FsFileContentStore::WalOpType::REMOVE,
      PathComponentPiece{"x"},
      nullptr);

  auto walPath = FsFileContentStore::getWalPath(parent);
  auto fullPath =
      canonicalPath(testDir_.path().string()) + RelativePathPiece{walPath};
  struct stat st{};
  ASSERT_EQ(0, ::stat(fullPath.c_str(), &st));
  EXPECT_EQ(0600, st.st_mode & 0777);
}

TEST_F(FsInodeCatalogWalTest, hasWal_falseWhenMissing) {
  EXPECT_FALSE(store_->hasWal(InodeNumber{200}));
}

TEST_F(FsInodeCatalogWalTest, hasWal_trueAfterAppend) {
  const InodeNumber parent{201};
  store_->appendWalEntry(
      parent,
      FsFileContentStore::WalOpType::REMOVE,
      PathComponentPiece{"x"},
      nullptr);
  EXPECT_TRUE(store_->hasWal(parent));
}

TEST_F(FsInodeCatalogWalTest, removeWal_missingFileIsNotAnError) {
  EXPECT_NO_THROW(store_->removeWal(InodeNumber{202}));
}

TEST_F(FsInodeCatalogWalTest, removeWal_removesAfterAppend) {
  const InodeNumber parent{203};
  store_->appendWalEntry(
      parent,
      FsFileContentStore::WalOpType::REMOVE,
      PathComponentPiece{"x"},
      nullptr);
  ASSERT_TRUE(store_->hasWal(parent));
  store_->removeWal(parent);
  EXPECT_FALSE(store_->hasWal(parent));
}

TEST_F(FsInodeCatalogWalTest, hasWal_togglesWithAppendAndRemove) {
  const InodeNumber parent{204};
  EXPECT_FALSE(store_->hasWal(parent));
  store_->appendWalEntry(
      parent,
      FsFileContentStore::WalOpType::REMOVE,
      PathComponentPiece{"x"},
      nullptr);
  EXPECT_TRUE(store_->hasWal(parent));
  store_->removeWal(parent);
  EXPECT_FALSE(store_->hasWal(parent));
}
TEST_F(FsInodeCatalogWalTest, hasWal_falseForSymlinkAtWalPath) {
  // hasWal uses fstatat + S_ISREG, so a stray symlink planted at the WAL
  // path must report false rather than be followed (or accepted as a WAL).
  const InodeNumber parent{205};
  ASSERT_FALSE(store_->hasWal(parent));

  // Plant a dangling symlink where the WAL would live. The shard
  // directory already exists (created by ensureShardDirectories at mount
  // setup), so symlinkat into it succeeds.
  auto walPath = FsFileContentStore::getWalPath(parent);
  auto fullPath =
      canonicalPath(testDir_.path().string()) + RelativePathPiece{walPath};
  ASSERT_EQ(0, ::symlink("/nonexistent-wal-target", fullPath.c_str()));

  EXPECT_FALSE(store_->hasWal(parent));
}

TEST_F(FsInodeCatalogWalTest, scanForWalFiles_emptyStore) {
  EXPECT_TRUE(store_->scanForWalFiles().empty());
}

TEST_F(FsInodeCatalogWalTest, scanForWalFiles_returnsInodesAcrossShards) {
  // Pick three inode numbers whose low byte differs so they land in
  // distinct shard directories.
  const InodeNumber a{0x101};
  const InodeNumber b{0x202};
  const InodeNumber c{0x303};
  for (auto ino : {a, b, c}) {
    store_->appendWalEntry(
        ino,
        FsFileContentStore::WalOpType::REMOVE,
        PathComponentPiece{"x"},
        nullptr);
  }

  auto found = store_->scanForWalFiles();
  std::sort(found.begin(), found.end(), [](auto x, auto y) {
    return x.get() < y.get();
  });
  ASSERT_EQ(3u, found.size());
  EXPECT_EQ(a, found[0]);
  EXPECT_EQ(b, found[1]);
  EXPECT_EQ(c, found[2]);
}

TEST_F(FsInodeCatalogWalTest, scanForWalFiles_ignoresUnparsableNames) {
  // Make sure at least one shard directory exists by writing a real WAL.
  const InodeNumber real{0xab};
  store_->appendWalEntry(
      real,
      FsFileContentStore::WalOpType::REMOVE,
      PathComponentPiece{"x"},
      nullptr);

  auto root = canonicalPath(testDir_.path().string());
  auto stray = root + RelativePathPiece{"ab/notanumber.wal"};
  ASSERT_TRUE(folly::writeFile(folly::StringPiece{""}, stray.c_str()));

  auto found = store_->scanForWalFiles();
  ASSERT_EQ(1u, found.size());
  EXPECT_EQ(real, found[0]);
}

TEST_F(FsInodeCatalogWalTest, scanForWalFiles_coexistsWithOverlayFiles) {
  // The shard dir holds the overlay file (no .wal suffix) at "<inode>" and
  // the WAL file at "<inode>.wal". scanForWalFiles must only enumerate the
  // latter.
  const InodeNumber ino{0xcd};
  store_->appendWalEntry(
      ino,
      FsFileContentStore::WalOpType::REMOVE,
      PathComponentPiece{"x"},
      nullptr);

  auto root = canonicalPath(testDir_.path().string());
  auto overlayStub = root + RelativePathPiece{"cd/205"};
  ASSERT_TRUE(folly::writeFile(folly::StringPiece{""}, overlayStub.c_str()));

  auto found = store_->scanForWalFiles();
  ASSERT_EQ(1u, found.size());
  EXPECT_EQ(ino, found[0]);
}

TEST_F(FsInodeCatalogWalTest, scanForWalFiles_skipsZeroInode) {
  // A stray "00/0.wal" would crash startup if returned: InodeNumber{0}
  // trips an internal assert. The scanner must reject and warn-skip
  // instead.
  auto root = canonicalPath(testDir_.path().string());
  auto bogus = root + RelativePathPiece{"00/0.wal"};
  ASSERT_TRUE(folly::writeFile(folly::StringPiece{""}, bogus.c_str()));

  auto found = store_->scanForWalFiles();
  EXPECT_TRUE(found.empty());
}

TEST_F(FsInodeCatalogWalTest, scanForWalFiles_skipsLeadingZeroNames) {
  // The catalog's writers always use minimal-width decimal, so "05.wal"
  // can only be junk. Returning it would also yield duplicate inode 5
  // alongside any real "5.wal".
  auto root = canonicalPath(testDir_.path().string());
  auto junk = root + RelativePathPiece{"05/05.wal"};
  ASSERT_TRUE(folly::writeFile(folly::StringPiece{""}, junk.c_str()));

  auto found = store_->scanForWalFiles();
  EXPECT_TRUE(found.empty());
}

TEST_F(FsInodeCatalogWalTest, scanForWalFiles_skipsWrongShardPlacement) {
  // Inode 0x171 hashes to shard 0x71, not 0x00. A WAL file dropped in
  // the wrong shard would be invisible to subsequent replayWal /
  // removeWal calls (those use getWalPath which reconstructs the shard
  // from inode & 0xff). Treat as corruption: skip.
  auto root = canonicalPath(testDir_.path().string());
  auto wrong = root + RelativePathPiece{"00/369.wal"}; // 369 == 0x171
  ASSERT_TRUE(folly::writeFile(folly::StringPiece{""}, wrong.c_str()));

  auto found = store_->scanForWalFiles();
  EXPECT_TRUE(found.empty());
}

TEST_F(FsInodeCatalogWalTest, scanForWalFiles_skipsNonRegularEntries) {
  // A directory or symlink named "<inode>.wal" is corruption. The
  // scanner must drop it so callers don't try to read it as a WAL.
  auto root = canonicalPath(testDir_.path().string());

  // Directory shaped like a WAL.
  auto bogusDir = root + RelativePathPiece{"00/512.wal"}; // 512 == 0x200
  ASSERT_EQ(0, ::mkdir(bogusDir.c_str(), 0700))
      << "mkdir failed: " << folly::errnoStr(errno);
  // 512 hashes to shard 0x00, so wrong-shard isn't filtering this out.

  auto found = store_->scanForWalFiles();
  EXPECT_TRUE(found.empty());
}

#endif
