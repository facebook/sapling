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
#include "eden/fs/model/TreeEntry.h"

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
constexpr size_t kIsRestrictedSize = sizeof(uint8_t);
constexpr size_t kAclRootStateSize = sizeof(uint8_t);
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

overlay::OverlayEntry makeEntryWithAclRootState(
    int32_t mode,
    int64_t inodeNumber,
    AclRootState state) {
  auto entry = makeEntry(mode, inodeNumber);
  entry.isRestricted() = state == AclRootState::RestrictedAclRoot;
  entry.aclRootState() = static_cast<int32_t>(state);
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

void expectAclRootStateTail(
    const std::string& data,
    size_t& offset,
    bool expectedIsRestricted,
    AclRootState expectedState) {
  EXPECT_EQ(expectedIsRestricted, static_cast<uint8_t>(data[offset]) != 0);
  offset += kIsRestrictedSize;
  EXPECT_EQ(
      static_cast<uint8_t>(expectedState), static_cast<uint8_t>(data[offset]));
  offset += kAclRootStateSize;
}

std::string makeOldFormatAddWalFrame(
    folly::StringPiece name,
    int32_t mode,
    int64_t inodeNumber,
    folly::StringPiece hash = {}) {
  auto nameLen = static_cast<uint16_t>(name.size());
  auto hashLen = static_cast<uint8_t>(hash.size());
  uint32_t entryLen = sizeof(uint8_t) + sizeof(uint16_t) + nameLen +
      sizeof(int32_t) + sizeof(int64_t) + sizeof(uint8_t) + hashLen;

  std::string bytes;
  bytes.reserve(sizeof(uint32_t) + entryLen);
  bytes.append(reinterpret_cast<const char*>(&entryLen), sizeof(entryLen));
  bytes.push_back(static_cast<char>(WalOpType::ADD));
  bytes.append(reinterpret_cast<const char*>(&nameLen), sizeof(nameLen));
  bytes.append(name.data(), name.size());
  bytes.append(reinterpret_cast<const char*>(&mode), sizeof(mode));
  bytes.append(
      reinterpret_cast<const char*>(&inodeNumber), sizeof(inodeNumber));
  bytes.push_back(static_cast<char>(hashLen));
  bytes.append(hash.data(), hash.size());
  return bytes;
}

// Write an arbitrary blob into the WAL file for `parent`, replacing any
// existing contents. Used to construct malformed/torn WAL files for the
// load tests below without depending on appendWalEntry's invariants.
void writeRawWal(
    const folly::test::TemporaryDirectory& testDir,
    InodeNumber parent,
    folly::StringPiece bytes) {
  auto walPath = FsFileContentStore::getWalPath(parent);
  auto fullPath =
      canonicalPath(testDir.path().string()) + RelativePathPiece{walPath};
  ASSERT_TRUE(folly::writeFile(bytes, fullPath.c_str()));
}

} // namespace

TEST_F(FsInodeCatalogWalTest, appendWalEntry_writesAddEntryWithHash) {
  const InodeNumber parent{1};
  auto entry = makeEntryWithHash(0100644, 42, std::string(20, '\xab'));
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"foo"}, &entry);

  auto bytes = readWal(parent);
  // entryLen header.
  ASSERT_GE(bytes.size(), kEntryLenSize);
  uint32_t entryLen = readU32(bytes, 0);
  EXPECT_EQ(bytes.size(), kEntryLenSize + entryLen);

  size_t off = kEntryLenSize;
  EXPECT_EQ(
      static_cast<uint8_t>(WalOpType::ADD), static_cast<uint8_t>(bytes[off]));
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
  off += 20;
  expectAclRootStateTail(bytes, off, false, AclRootState::Unknown);
  EXPECT_EQ(off, bytes.size());
}

TEST_F(FsInodeCatalogWalTest, appendWalEntry_writesAddEntryWithoutHash) {
  const InodeNumber parent{2};
  auto entry = makeEntry(0100644, 7);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"bar"}, &entry);

  auto bytes = readWal(parent);
  ASSERT_GE(bytes.size(), kEntryLenSize);
  uint32_t entryLen = readU32(bytes, 0);
  EXPECT_EQ(bytes.size(), kEntryLenSize + entryLen);

  // hashLen byte at the tail must be zero, no trailing hash bytes.
  size_t off = kEntryLenSize + kOpByteSize + kNameLenSize + kTestNameSize +
      kModeSize + kInodeNumberSize;
  EXPECT_EQ(0, static_cast<uint8_t>(bytes[off]));
  off += kHashLenSize;
  expectAclRootStateTail(bytes, off, false, AclRootState::Unknown);
  EXPECT_EQ(off, bytes.size());
}

TEST_F(FsInodeCatalogWalTest, appendWalEntry_writesAclRootState) {
  const InodeNumber parent{9};
  auto entry = makeEntryWithAclRootState(
      S_IFDIR | 0755, 8, AclRootState::RestrictedAclRoot);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"acl"}, &entry);

  auto bytes = readWal(parent);
  ASSERT_GE(bytes.size(), kEntryLenSize);

  size_t off = kEntryLenSize + kOpByteSize + kNameLenSize + kTestNameSize +
      kModeSize + kInodeNumberSize;
  EXPECT_EQ(0, static_cast<uint8_t>(bytes[off]));
  off += kHashLenSize;
  expectAclRootStateTail(bytes, off, true, AclRootState::RestrictedAclRoot);
  EXPECT_EQ(off, bytes.size());
}

TEST_F(FsInodeCatalogWalTest, appendWalEntry_writesRemoveEntry) {
  const InodeNumber parent{3};
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"baz"}, nullptr);

  auto bytes = readWal(parent);
  // REMOVE payload = opByte + nameLen + name.
  constexpr size_t kRemovePayloadSize =
      kOpByteSize + kNameLenSize + kTestNameSize;
  EXPECT_EQ(kEntryLenSize + kRemovePayloadSize, bytes.size());
  EXPECT_EQ(kRemovePayloadSize, readU32(bytes, 0));
  EXPECT_EQ(
      static_cast<uint8_t>(WalOpType::REMOVE),
      static_cast<uint8_t>(bytes[kEntryLenSize]));
  EXPECT_EQ(kTestNameSize, readU16(bytes, kEntryLenSize + kOpByteSize));
  EXPECT_EQ(
      "baz",
      bytes.substr(kEntryLenSize + kOpByteSize + kNameLenSize, kTestNameSize));
}

TEST_F(FsInodeCatalogWalTest, appendWalEntry_writesMaterializeEntry) {
  const InodeNumber parent{4};
  store_->appendWalEntry(
      parent, WalOpType::MATERIALIZE, PathComponentPiece{"qux"}, nullptr);

  auto bytes = readWal(parent);
  // MATERIALIZE has the same shape as REMOVE: just a name, no payload.
  constexpr size_t kMaterializePayloadSize =
      kOpByteSize + kNameLenSize + kTestNameSize;
  EXPECT_EQ(kEntryLenSize + kMaterializePayloadSize, bytes.size());
  EXPECT_EQ(
      static_cast<uint8_t>(WalOpType::MATERIALIZE),
      static_cast<uint8_t>(bytes[kEntryLenSize]));
}

TEST_F(FsInodeCatalogWalTest, appendWalEntry_appendsAcrossMultipleCalls) {
  const InodeNumber parent{5};
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"a"}, nullptr);
  auto sizeAfterFirst = readWal(parent).size();
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"b"}, nullptr);
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
      parent, WalOpType::REMOVE, PathComponentPiece{"x"}, nullptr);

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
      parent, WalOpType::REMOVE, PathComponentPiece{"x"}, nullptr);
  EXPECT_TRUE(store_->hasWal(parent));
}

TEST_F(FsInodeCatalogWalTest, removeWal_missingFileIsNotAnError) {
  EXPECT_NO_THROW(store_->removeWal(InodeNumber{202}));
}

TEST_F(FsInodeCatalogWalTest, removeWal_removesAfterAppend) {
  const InodeNumber parent{203};
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"x"}, nullptr);
  ASSERT_TRUE(store_->hasWal(parent));
  store_->removeWal(parent);
  EXPECT_FALSE(store_->hasWal(parent));
}

TEST_F(FsInodeCatalogWalTest, hasWal_togglesWithAppendAndRemove) {
  const InodeNumber parent{204};
  EXPECT_FALSE(store_->hasWal(parent));
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"x"}, nullptr);
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
        ino, WalOpType::REMOVE, PathComponentPiece{"x"}, nullptr);
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
      real, WalOpType::REMOVE, PathComponentPiece{"x"}, nullptr);

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
      ino, WalOpType::REMOVE, PathComponentPiece{"x"}, nullptr);

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

TEST_F(FsInodeCatalogWalTest, loadWalDelta_emptyForMissingFile) {
  EXPECT_TRUE(store_->loadWalDelta(InodeNumber{300}).delta.empty());
}

TEST_F(FsInodeCatalogWalTest, loadWalDelta_collapsesAddThenRemove) {
  const InodeNumber parent{301};
  auto entry = makeEntry(0100644, 1);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"x"}, &entry);
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"x"}, nullptr);

  auto delta = store_->loadWalDelta(parent).delta;
  ASSERT_EQ(1u, delta.size());
  EXPECT_EQ(WalOpType::REMOVE, delta.at("x").type);
}

TEST_F(FsInodeCatalogWalTest, loadWalDelta_collapsesAddThenMaterialize) {
  const InodeNumber parent{302};
  auto entry = makeEntryWithHash(0100644, 1, std::string(20, '\xab'));
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"x"}, &entry);
  store_->appendWalEntry(
      parent, WalOpType::MATERIALIZE, PathComponentPiece{"x"}, nullptr);

  auto delta = store_->loadWalDelta(parent).delta;
  ASSERT_EQ(1u, delta.size());
  // ADD survives but its hash is cleared in place.
  EXPECT_EQ(WalOpType::ADD, delta.at("x").type);
  EXPECT_FALSE(delta.at("x").entry.hash().has_value());
  EXPECT_EQ(0100644, *delta.at("x").entry.mode());
  EXPECT_EQ(1, *delta.at("x").entry.inodeNumber());
}

TEST_F(
    FsInodeCatalogWalTest,
    loadWalDelta_materializeAloneRecordedAsMaterialize) {
  const InodeNumber parent{303};
  store_->appendWalEntry(
      parent, WalOpType::MATERIALIZE, PathComponentPiece{"y"}, nullptr);

  auto delta = store_->loadWalDelta(parent).delta;
  ASSERT_EQ(1u, delta.size());
  EXPECT_EQ(WalOpType::MATERIALIZE, delta.at("y").type);
}

TEST_F(FsInodeCatalogWalTest, loadWalDelta_removeThenAddBecomesAdd) {
  const InodeNumber parent{304};
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"x"}, nullptr);
  auto entry = makeEntry(0100644, 99);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"x"}, &entry);

  auto delta = store_->loadWalDelta(parent).delta;
  ASSERT_EQ(1u, delta.size());
  EXPECT_EQ(WalOpType::ADD, delta.at("x").type);
  EXPECT_EQ(99, *delta.at("x").entry.inodeNumber());
}

TEST_F(FsInodeCatalogWalTest, loadWalDelta_preservesAclRootState) {
  const InodeNumber parent{309};
  auto entry =
      makeEntryWithAclRootState(S_IFDIR | 0755, 100, AclRootState::AclRoot);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"x"}, &entry);

  auto delta = store_->loadWalDelta(parent).delta;
  ASSERT_EQ(1u, delta.size());
  const auto& loaded = delta.at("x").entry;
  EXPECT_FALSE(*loaded.isRestricted());
  EXPECT_EQ(
      static_cast<int32_t>(AclRootState::AclRoot), *loaded.aclRootState());
}

TEST_F(FsInodeCatalogWalTest, loadWalDelta_acceptsLegacyAddWithoutAclTail) {
  const InodeNumber parent{310};
  ASSERT_NO_FATAL_FAILURE(writeRawWal(
      testDir_, parent, makeOldFormatAddWalFrame("x", S_IFDIR | 0755, 100)));

  auto delta = store_->loadWalDelta(parent).delta;
  ASSERT_EQ(1u, delta.size());
  const auto& loaded = delta.at("x").entry;
  EXPECT_EQ(S_IFDIR | 0755, *loaded.mode());
  EXPECT_EQ(100, *loaded.inodeNumber());
  EXPECT_FALSE(*loaded.isRestricted());
  EXPECT_FALSE(loaded.aclRootState().has_value());
}

TEST_F(
    FsInodeCatalogWalTest,
    loadWalDelta_caseInsensitiveRemoveThenAddPreservesAddCasing) {
  const InodeNumber parent{330};
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"foo"}, nullptr);
  auto entry = makeEntry(0100644, 99);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"FOO"}, &entry);

  auto delta = store_->loadWalDelta(parent, CaseSensitivity::Insensitive).delta;
  ASSERT_EQ(1u, delta.size());
  auto it = delta.begin();
  EXPECT_EQ("FOO", it->first);
  EXPECT_EQ(WalOpType::ADD, it->second.type);
  EXPECT_EQ(99, *it->second.entry.inodeNumber());
}

TEST_F(FsInodeCatalogWalTest, loadWalDelta_caseInsensitiveAddThenRemoveWins) {
  const InodeNumber parent{331};
  auto entry = makeEntry(0100644, 99);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"FOO"}, &entry);
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"foo"}, nullptr);

  auto delta = store_->loadWalDelta(parent, CaseSensitivity::Insensitive).delta;
  ASSERT_EQ(1u, delta.size());
  auto it = delta.begin();
  EXPECT_EQ("foo", it->first);
  EXPECT_EQ(WalOpType::REMOVE, it->second.type);
}

TEST_F(FsInodeCatalogWalTest, loadWalDelta_repeatedNamesYieldFinalState) {
  const InodeNumber parent{305};
  auto e1 = makeEntry(0100644, 1);
  auto e2 = makeEntry(0100644, 2);
  auto e3 = makeEntry(0100644, 3);
  store_->appendWalEntry(parent, WalOpType::ADD, PathComponentPiece{"x"}, &e1);
  store_->appendWalEntry(parent, WalOpType::ADD, PathComponentPiece{"x"}, &e2);
  store_->appendWalEntry(parent, WalOpType::ADD, PathComponentPiece{"x"}, &e3);

  auto delta = store_->loadWalDelta(parent).delta;
  ASSERT_EQ(1u, delta.size());
  EXPECT_EQ(3, *delta.at("x").entry.inodeNumber());
}

TEST_F(FsInodeCatalogWalTest, loadWalDelta_truncatedTailIsDiscarded) {
  const InodeNumber parent{306};
  auto entry = makeEntry(0100644, 1);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"keep"}, &entry);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"drop"}, &entry);
  auto walPath = FsFileContentStore::getWalPath(parent);
  auto fullPath =
      canonicalPath(testDir_.path().string()) + RelativePathPiece{walPath};
  std::string bytes;
  ASSERT_TRUE(folly::readFile(fullPath.c_str(), bytes));
  ASSERT_TRUE(
      folly::writeFile(bytes.substr(0, bytes.size() - 3), fullPath.c_str()));

  auto delta = store_->loadWalDelta(parent).delta;
  ASSERT_EQ(1u, delta.size());
  EXPECT_TRUE(delta.count("keep"));
  EXPECT_FALSE(delta.count("drop"));
}

TEST_F(FsInodeCatalogWalTest, loadWalDelta_unknownOpIsSkipped) {
  // Forward-compat: an unknown opcode WITH a valid entryLen frame is
  // skipped (advanced by entryLen), not fatal. A subsequent valid REMOVE
  // in the same WAL must still be applied.
  const InodeNumber parent{307};
  // Frame: entryLen=3 covers op(1) + nameLen(2) + name(0). op=0xff is
  // outside the WalOpType enum, so the parser must hit the default case
  // in the switch and skip-by-entryLen.
  std::string corrupt;
  uint32_t bad = 3;
  corrupt.append(reinterpret_cast<const char*>(&bad), sizeof(bad));
  corrupt.push_back(static_cast<char>(0xff)); // op
  uint16_t nameLen = 0;
  corrupt.append(reinterpret_cast<const char*>(&nameLen), sizeof(nameLen));
  ASSERT_NO_FATAL_FAILURE(writeRawWal(testDir_, parent, corrupt));

  // Append a valid REMOVE after the unknown-op entry.
  store_->appendWalEntry(parent, WalOpType::REMOVE, "y"_pc, nullptr);

  auto result = store_->loadWalDelta(parent);
  ASSERT_EQ(1u, result.delta.size());
  EXPECT_EQ(WalOpType::REMOVE, result.delta.at("y").type);
  // The unknown-op skip counts toward parseErrors, not rawEntriesParsed.
  // Only the valid REMOVE that follows is counted as a successfully-decoded
  // entry — keeping the OverlayStats::walEntriesReplayed and
  // wal_parse_failure counters from double-counting the same entry.
  EXPECT_EQ(1u, result.parseErrors);
  EXPECT_EQ(1u, result.rawEntriesParsed);
}

TEST_F(FsInodeCatalogWalTest, loadWalDelta_materializeAfterRemoveLeavesRemove) {
  // Regression for the divergence between replayWal and loadWalDelta on
  // the byte-stream [REMOVE x][MATERIALIZE x]. replayWal applies REMOVE
  // first (deleting "x"), then no-ops on MATERIALIZE because "x" is gone.
  // loadWalDelta must produce an equivalent net delta — REMOVE — instead
  // of letting MATERIALIZE clobber the prior REMOVE and silently
  // resurrecting the entry on the direct-serialization load path.
  const InodeNumber parent{401};
  store_->appendWalEntry(parent, WalOpType::REMOVE, "x"_pc, nullptr);
  store_->appendWalEntry(parent, WalOpType::MATERIALIZE, "x"_pc, nullptr);

  auto delta = store_->loadWalDelta(parent).delta;
  ASSERT_EQ(1u, delta.size());
  auto it = delta.find("x");
  ASSERT_NE(delta.end(), it);
  EXPECT_EQ(WalOpType::REMOVE, it->second.type);
}

TEST_F(FsInodeCatalogWalTest, replayWal_missingFileReturnsZero) {
  overlay::OverlayDir dir;
  EXPECT_EQ(0u, store_->replayWal(InodeNumber{100}, dir).rawEntriesParsed);
  EXPECT_TRUE(dir.entries_ref()->empty());
}

TEST_F(FsInodeCatalogWalTest, replayWal_roundTripsAddRemoveMaterialize) {
  const InodeNumber parent{101};
  auto add = makeEntry(0100644, 200);
  auto addWithHash = makeEntryWithHash(0100644, 201, std::string(20, '\xab'));
  store_->appendWalEntry(parent, WalOpType::ADD, PathComponentPiece{"a"}, &add);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"b"}, &addWithHash);
  store_->appendWalEntry(
      parent, WalOpType::MATERIALIZE, PathComponentPiece{"b"}, nullptr);
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"a"}, nullptr);

  overlay::OverlayDir dir;
  // 4 raw WAL entries; ADD/REMOVE/MATERIALIZE collapse to 2 unique names
  // but rawEntriesParsed reports the 4 as-written.
  EXPECT_EQ(4u, store_->replayWal(parent, dir).rawEntriesParsed);
  // "a" was added then removed.
  EXPECT_EQ(0u, dir.entries_ref()->count("a"));
  // "b" was added with a hash, then materialized clears the hash.
  ASSERT_EQ(1u, dir.entries_ref()->count("b"));
  const auto& b = dir.entries_ref()->at("b");
  EXPECT_EQ(0100644, *b.mode());
  EXPECT_EQ(201, *b.inodeNumber());
  EXPECT_FALSE(b.hash().has_value());
}

TEST_F(FsInodeCatalogWalTest, replayWal_preservesAclRootState) {
  const InodeNumber parent{109};
  auto entry = makeEntryWithAclRootState(
      S_IFDIR | 0755, 202, AclRootState::RestrictedAclRoot);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"x"}, &entry);

  overlay::OverlayDir dir;
  EXPECT_EQ(1u, store_->replayWal(parent, dir).rawEntriesParsed);
  ASSERT_EQ(1u, dir.entries_ref()->count("x"));
  const auto& loaded = dir.entries_ref()->at("x");
  EXPECT_TRUE(*loaded.isRestricted());
  EXPECT_EQ(
      static_cast<int32_t>(AclRootState::RestrictedAclRoot),
      *loaded.aclRootState());
}

TEST_F(FsInodeCatalogWalTest, replayWal_acceptsLegacyAddWithoutAclTail) {
  const InodeNumber parent{110};
  ASSERT_NO_FATAL_FAILURE(writeRawWal(
      testDir_, parent, makeOldFormatAddWalFrame("x", S_IFDIR | 0755, 202)));

  overlay::OverlayDir dir;
  EXPECT_EQ(1u, store_->replayWal(parent, dir).rawEntriesParsed);
  ASSERT_EQ(1u, dir.entries_ref()->count("x"));
  const auto& loaded = dir.entries_ref()->at("x");
  EXPECT_EQ(S_IFDIR | 0755, *loaded.mode());
  EXPECT_EQ(202, *loaded.inodeNumber());
  EXPECT_FALSE(*loaded.isRestricted());
  EXPECT_FALSE(loaded.aclRootState().has_value());
}

TEST_F(FsInodeCatalogWalTest, replayWal_overwritesExistingDirEntries) {
  const InodeNumber parent{102};
  // Pre-populate the dir as if the base file already had this entry.
  overlay::OverlayDir dir;
  auto stale = makeEntry(0100600, 1);
  (*dir.entries_ref())["x"] = stale;

  // WAL ADD with new metadata for the same name should overwrite.
  auto fresh = makeEntry(0100644, 999);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"x"}, &fresh);

  EXPECT_EQ(1u, store_->replayWal(parent, dir).rawEntriesParsed);
  ASSERT_EQ(1u, dir.entries_ref()->count("x"));
  EXPECT_EQ(0100644, *dir.entries_ref()->at("x").mode());
  EXPECT_EQ(999, *dir.entries_ref()->at("x").inodeNumber());
}

TEST_F(FsInodeCatalogWalTest, replayWal_caseInsensitiveAddRekeysEntry) {
  const InodeNumber parent{108};
  overlay::OverlayDir dir;
  (*dir.entries_ref())["foo"] = makeEntry(0100644, 1);

  auto fresh = makeEntry(0100644, 999);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"FOO"}, &fresh);

  EXPECT_EQ(
      1u,
      store_->replayWal(parent, dir, CaseSensitivity::Insensitive)
          .rawEntriesParsed);
  EXPECT_EQ(0u, dir.entries_ref()->count("foo"));
  ASSERT_EQ(1u, dir.entries_ref()->count("FOO"));
  EXPECT_EQ(999, *dir.entries_ref()->at("FOO").inodeNumber());
}

TEST_F(FsInodeCatalogWalTest, replayWal_truncatedTailIsDiscarded) {
  const InodeNumber parent{103};
  auto entry = makeEntry(0100644, 1);
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"keep"}, &entry);
  // Append a second entry, then truncate the file inside that second entry.
  store_->appendWalEntry(
      parent, WalOpType::ADD, PathComponentPiece{"drop"}, &entry);
  auto walPath = FsFileContentStore::getWalPath(parent);
  auto fullPath =
      canonicalPath(testDir_.path().string()) + RelativePathPiece{walPath};
  std::string bytes;
  ASSERT_TRUE(folly::readFile(fullPath.c_str(), bytes));
  ASSERT_TRUE(
      folly::writeFile(bytes.substr(0, bytes.size() - 3), fullPath.c_str()));

  overlay::OverlayDir dir;
  auto result = store_->replayWal(parent, dir);
  EXPECT_EQ(1u, result.rawEntriesParsed);
  // Torn tail surfaces through replayWal's parseErrors so cold-path callers
  // can bump OverlayStats::walParseFailure.
  EXPECT_GT(result.parseErrors, 0u);
  EXPECT_EQ(1u, dir.entries_ref()->count("keep"));
  EXPECT_EQ(0u, dir.entries_ref()->count("drop"));
}

TEST_F(FsInodeCatalogWalTest, replayWal_zeroEntryLenStops) {
  const InodeNumber parent{104};
  // Four zero bytes: a zero entryLen, indistinguishable from a sparse file
  // tail. Replay must stop without applying anything.
  ASSERT_NO_FATAL_FAILURE(writeRawWal(testDir_, parent, std::string(4, '\0')));
  overlay::OverlayDir dir;
  EXPECT_EQ(0u, store_->replayWal(parent, dir).rawEntriesParsed);
}

TEST_F(FsInodeCatalogWalTest, replayWal_unknownOpIsSkipped) {
  // Forward-compat: an unknown opcode with a valid entryLen frame is
  // skipped via replayWal too (it inherits loadWalDelta's parser).
  const InodeNumber parent{105};
  // Write valid REMOVE first; replay should still apply it after skipping
  // the unknown-op entry that follows.
  store_->appendWalEntry(
      parent, WalOpType::REMOVE, PathComponentPiece{"good"}, nullptr);

  // Append a valid frame for an unknown opcode: entryLen=3 covers
  // op(1) + nameLen(2) + name(0).
  auto walPath = FsFileContentStore::getWalPath(parent);
  auto fullPath =
      canonicalPath(testDir_.path().string()) + RelativePathPiece{walPath};
  std::string existing;
  ASSERT_TRUE(folly::readFile(fullPath.c_str(), existing));
  std::string corrupt;
  uint32_t entryLen = 3;
  corrupt.append(reinterpret_cast<const char*>(&entryLen), sizeof(entryLen));
  corrupt.push_back(static_cast<char>(0xff)); // op
  uint16_t nameLen = 0;
  corrupt.append(reinterpret_cast<const char*>(&nameLen), sizeof(nameLen));
  ASSERT_TRUE(folly::writeFile(existing + corrupt, fullPath.c_str()));

  overlay::OverlayDir dir;
  (*dir.entries_ref())["good"] = makeEntry(0100644, 1);
  // 1 raw entry parsed (REMOVE); unknown-op skip counts as parseError
  // and surfaces through replayWal so cold paths see the same signal as
  // loadWalDelta.
  auto result = store_->replayWal(parent, dir);
  EXPECT_EQ(1u, result.rawEntriesParsed);
  EXPECT_EQ(1u, result.parseErrors);
  EXPECT_EQ(0u, dir.entries_ref()->count("good"));
}

TEST_F(FsInodeCatalogWalTest, replayWal_truncatedInsideFieldsStopsCleanly) {
  // Construct an entry whose declared entryLen exceeds the bytes that
  // follow. Replay must not parse past the buffer and must apply zero
  // entries.
  const InodeNumber parent{106};
  std::string corrupt;
  // entryLen = 100, but only 1 byte (the opByte) follows.
  uint32_t bad = 100;
  corrupt.append(reinterpret_cast<const char*>(&bad), sizeof(bad));
  corrupt.push_back(static_cast<char>(WalOpType::ADD));
  ASSERT_NO_FATAL_FAILURE(writeRawWal(testDir_, parent, corrupt));

  overlay::OverlayDir dir;
  EXPECT_EQ(0u, store_->replayWal(parent, dir).rawEntriesParsed);
}

#endif
